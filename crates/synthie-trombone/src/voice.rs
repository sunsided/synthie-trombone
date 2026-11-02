//! [`Trombone`]: the complete voice — glottis, tract, noise and resampling —
//! behind a small, real-time safe API.

use crate::INTERNAL_RATE;
use crate::glottis::{DEFAULT_ATTACK, DEFAULT_RELEASE, Glottis};
use crate::math::midi_to_hz;
use crate::noise::{BandPass, Rng, Simplex};
use crate::phoneme::{Articulation, Phoneme};
use crate::tract::{Constriction, MOVEMENT_SPEED, N, NOSE_LEN, Tract};
use core::f32::consts::PI;

/// Internal samples per parameter block.
const BLOCK: u32 = 512;
/// Constriction slot used by [`Trombone::articulate`] for the main closure.
pub const SLOT_MAIN: usize = 0;
/// Constriction slot used by [`Trombone::articulate`] for lip rounding.
pub const SLOT_LIPS: usize = 1;

/// One Pink Trombone voice.
///
/// The model runs at a fixed [`INTERNAL_RATE`] (tract at twice that) and is
/// resampled to the output rate, so formants and timing do not depend on the
/// host sample rate. Parameters update every 512 internal samples and are
/// interpolated in between, independent of the host's buffer size.
///
/// No method allocates; [`process`](Self::process) is safe to call from an
/// audio callback.
#[derive(Clone, Debug)]
pub struct Trombone {
    glottis: Glottis,
    tract: Tract,
    simplex: Simplex,
    rng: Rng,
    frication: BandPass,
    block_pos: u32,

    sample_rate: f32,
    internal_rate: f32,
    tract_length: f32,
    step: f64,
    frac: f64,
    prev: f32,
    cur: f32,

    dc_x: f32,
    dc_y: f32,
    dc_r: f32,
    gain: f32,
}

impl Trombone {
    /// A voice for the given output sample rate with the default seed.
    ///
    /// # Panics
    ///
    /// Panics if `sample_rate` is not positive and finite.
    #[must_use]
    pub fn new(sample_rate: f32) -> Self {
        Self::with_seed(sample_rate, 0x5EED_7B0E)
    }

    /// A voice with an explicit noise seed. Equal seeds render equal audio.
    ///
    /// # Panics
    ///
    /// Panics if `sample_rate` is not positive and finite.
    #[must_use]
    pub fn with_seed(sample_rate: f32, seed: u32) -> Self {
        assert!(
            sample_rate.is_finite() && sample_rate > 0.0,
            "sample_rate must be positive"
        );
        let mut rng = Rng::new(seed);
        let simplex = Simplex::new(&mut rng);
        Self {
            glottis: Glottis::new(),
            tract: Tract::new(),
            simplex,
            rng,
            frication: BandPass::new(1000.0, 0.5, INTERNAL_RATE),
            block_pos: 0,
            sample_rate,
            internal_rate: INTERNAL_RATE,
            tract_length: 1.0,
            step: f64::from(INTERNAL_RATE) / f64::from(sample_rate),
            frac: 1.0,
            prev: 0.0,
            cur: 0.0,
            dc_x: 0.0,
            dc_y: 0.0,
            dc_r: 1.0 - 2.0 * PI * 10.0 / sample_rate,
            gain: 1.0,
        }
    }

    /// Output sample rate.
    #[must_use]
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Silence all sound immediately; articulation and settings are kept.
    pub fn reset(&mut self) {
        self.glottis.reset();
        self.tract.reset_waves();
        self.frication.reset();
        self.block_pos = 0;
        self.frac = 1.0;
        self.prev = 0.0;
        self.cur = 0.0;
        self.dc_x = 0.0;
        self.dc_y = 0.0;
    }

    // ----- voicing and pitch -------------------------------------------

    /// Start phonating (the original's "touching the voicebox").
    pub fn note_on(&mut self) {
        self.glottis.voiced = true;
    }

    /// Stop phonating; the voice fades out.
    pub fn note_off(&mut self) {
        self.glottis.voiced = false;
    }

    /// Start phonating at a MIDI note.
    pub fn note_on_midi(&mut self, note: f32) {
        self.set_note(note);
        self.note_on();
    }

    /// Set the voicing gate directly.
    pub fn set_voiced(&mut self, voiced: bool) {
        self.glottis.voiced = voiced;
    }

    /// Whether the voicing gate is on.
    #[must_use]
    pub fn is_voiced(&self) -> bool {
        self.glottis.voiced
    }

    /// Phonate continuously, ignoring the gate, like the original web toy.
    pub fn set_always_voice(&mut self, on: bool) {
        self.glottis.always_voice = on;
    }

    /// Target pitch in Hz (clamped to 20‥2000).
    pub fn set_frequency(&mut self, hz: f32) {
        if hz.is_finite() {
            self.glottis.target_frequency = hz.clamp(20.0, 2000.0);
        }
    }

    /// Target pitch as a (fractional) MIDI note.
    pub fn set_note(&mut self, note: f32) {
        self.set_frequency(midi_to_hz(note));
    }

    /// Target pitch in Hz.
    #[must_use]
    pub fn target_frequency(&self) -> f32 {
        self.glottis.target_frequency
    }

    /// Current pitch in Hz, after glide, vibrato and jitter.
    #[must_use]
    pub fn frequency(&self) -> f32 {
        self.glottis.frequency()
    }

    /// Glottal tenseness, `0` (breathy whisper) to `1` (pressed).
    pub fn set_tenseness(&mut self, tenseness: f32) {
        if tenseness.is_finite() {
            self.glottis.target_tenseness = tenseness.clamp(0.0, 1.0);
        }
    }

    /// Glottal tenseness.
    #[must_use]
    pub fn tenseness(&self) -> f32 {
        self.glottis.target_tenseness
    }

    /// Vibrato depth (relative pitch, default `0.005`) and rate in Hz.
    pub fn set_vibrato(&mut self, amount: f32, hz: f32) {
        if amount.is_finite() && hz.is_finite() {
            self.glottis.vibrato_amount = amount.clamp(0.0, 0.5);
            self.glottis.vibrato_frequency = hz.clamp(0.0, 30.0);
        }
    }

    /// The original's large, slow pitch wobble.
    pub fn set_wobble(&mut self, on: bool) {
        self.glottis.wobble = on;
    }

    /// Glide speed: relative pitch change per 10.7 ms block (default `0.1`).
    /// Larger is faster; `f32::INFINITY` jumps.
    pub fn set_glide(&mut self, speed: f32) {
        if !speed.is_nan() {
            self.glottis.glide = speed.max(0.0);
        }
    }

    /// Voicing onset and offset rates, in intensity per second. Defaults are
    /// the original's slow, singing-like rates; speech wants about `30`.
    pub fn set_voicing_rates(&mut self, attack: f32, release: f32) {
        if attack.is_finite() && release.is_finite() {
            self.glottis.attack = attack.max(0.1);
            self.glottis.release = release.max(0.1);
        }
    }

    /// Restore the original's voicing rates.
    pub fn reset_voicing_rates(&mut self) {
        self.glottis.attack = DEFAULT_ATTACK;
        self.glottis.release = DEFAULT_RELEASE;
    }

    /// Vocal tract length relative to an adult male (`1.0`, about 17 cm).
    /// About `0.85` for an adult woman, `0.7` for a child; clamped to
    /// `0.5‥1.6`. Shorter tracts raise all formants. CPU cost scales with
    /// `1 / length`.
    pub fn set_tract_length(&mut self, length: f32) {
        if !length.is_finite() {
            return;
        }
        let length = length.clamp(0.5, 1.6);
        if length.to_bits() == self.tract_length.to_bits() {
            return;
        }
        self.tract_length = length;
        self.internal_rate = INTERNAL_RATE / length;
        self.step = f64::from(self.internal_rate) / f64::from(self.sample_rate);
        self.glottis.set_rate(self.internal_rate);
        self.tract.set_rate(self.internal_rate);
        self.frication = BandPass::new(1000.0, 0.5, self.internal_rate);
    }

    /// Vocal tract length (see [`set_tract_length`](Self::set_tract_length)).
    #[must_use]
    pub fn tract_length(&self) -> f32 {
        self.tract_length
    }

    /// Small random pitch jitter, `1.0` natural (default), `0.0` perfectly
    /// steady (robotic).
    pub fn set_jitter(&mut self, amount: f32) {
        if amount.is_finite() {
            self.glottis.jitter = amount.clamp(0.0, 5.0);
        }
    }

    /// Glottal intensity, `0‥1` (rises while voiced, falls when not).
    #[must_use]
    pub fn intensity(&self) -> f32 {
        self.glottis.intensity
    }

    // ----- articulation ------------------------------------------------

    /// Tongue body position (clamped to [`TONGUE_INDEX`](crate::TONGUE_INDEX))
    /// and height (clamped to [`TONGUE_DIAMETER`](crate::TONGUE_DIAMETER)).
    pub fn set_tongue(&mut self, index: f32, diameter: f32) {
        if index.is_finite() && diameter.is_finite() {
            self.tract.set_tongue(index, diameter);
        }
    }

    /// Tongue body `(index, diameter)`.
    #[must_use]
    pub fn tongue(&self) -> (f32, f32) {
        (self.tract.tongue_index, self.tract.tongue_diameter)
    }

    /// Place, move, or release (`None`) a constriction. Slots
    /// `0..MAX_CONSTRICTIONS`; out-of-range slots are ignored.
    pub fn set_constriction(&mut self, slot: usize, c: Option<Constriction>) {
        self.tract.set_constriction(slot, c);
    }

    /// The live constriction in `slot`.
    #[must_use]
    pub fn constriction(&self, slot: usize) -> Option<Constriction> {
        self.tract.constriction(slot)
    }

    /// Velum target, `0` (closed, oral) to `1` (open, nasal).
    pub fn set_velum(&mut self, openness: f32) {
        if openness.is_finite() {
            self.tract.set_velum(openness);
        }
    }

    /// Current velum openness, `0‥1`.
    #[must_use]
    pub fn velum(&self) -> f32 {
        self.tract.velum_openness()
    }

    /// Articulator speed multiplier (`1.0` is the original's).
    pub fn set_articulation_speed(&mut self, factor: f32) {
        if factor.is_finite() {
            self.tract.movement_speed = MOVEMENT_SPEED * factor.clamp(0.05, 20.0);
        }
    }

    /// Move to an articulation. Uses [`SLOT_MAIN`] and [`SLOT_LIPS`]; leaves
    /// voicing and pitch alone (see [`Articulation::voiced`]).
    pub fn articulate(&mut self, a: &Articulation) {
        if let Some((index, diameter)) = a.tongue {
            self.set_tongue(index, diameter);
        }
        self.tract.set_constriction(SLOT_MAIN, a.constriction);
        self.tract.set_constriction(SLOT_LIPS, a.lips);
        self.tract.set_velum(a.velum);
    }

    /// Move to a phoneme's articulation (see [`articulate`](Self::articulate)).
    pub fn articulate_phoneme(&mut self, p: Phoneme) {
        self.articulate(&p.articulation());
    }

    // ----- inspection --------------------------------------------------

    /// Current oral tract diameters, glottis to lips.
    #[must_use]
    pub fn diameters(&self) -> &[f32; N] {
        &self.tract.diameter
    }

    /// Diameters the tract is moving towards.
    #[must_use]
    pub fn target_diameters(&self) -> &[f32; N] {
        &self.tract.target
    }

    /// Nasal tract diameters, velum to nostrils.
    #[must_use]
    pub fn nose_diameters(&self) -> &[f32; NOSE_LEN] {
        &self.tract.nose_diameter
    }

    /// Recent peak pressure per oral cell (slowly decaying), for display.
    #[must_use]
    pub fn amplitudes(&self) -> &[f32; N] {
        self.tract.amplitudes()
    }

    /// Recent peak pressure per nasal cell.
    #[must_use]
    pub fn nose_amplitudes(&self) -> &[f32; NOSE_LEN] {
        self.tract.nose_amplitudes()
    }

    // ----- audio -------------------------------------------------------

    /// Output gain (default `1.0`).
    pub fn set_gain(&mut self, gain: f32) {
        if gain.is_finite() {
            self.gain = gain.max(0.0);
        }
    }

    /// One output sample.
    #[allow(clippy::cast_possible_truncation)]
    pub fn process(&mut self) -> f32 {
        while self.frac >= 1.0 {
            self.prev = self.cur;
            self.cur = self.internal_sample();
            self.frac -= 1.0;
        }
        let x = self.prev + (self.cur - self.prev) * self.frac as f32;
        self.frac += self.step;

        // DC blocker: the glottal flow derivative is not quite zero-mean.
        let y = x - self.dc_x + self.dc_r * self.dc_y;
        self.dc_x = x;
        self.dc_y = y;
        y * self.gain
    }

    /// Fill `out` with mono samples.
    pub fn render(&mut self, out: &mut [f32]) {
        for s in out {
            *s = self.process();
        }
    }

    #[allow(clippy::cast_precision_loss)]
    fn internal_sample(&mut self) -> f32 {
        if self.block_pos == 0 {
            let dt = BLOCK as f32 / self.internal_rate;
            self.glottis.update_block(dt, &self.simplex);
            self.tract.update_block(dt);
        }
        let lambda1 = self.block_pos as f32 / BLOCK as f32;
        let lambda2 = (self.block_pos as f32 + 0.5) / BLOCK as f32;
        self.block_pos = (self.block_pos + 1) % BLOCK;

        let glottal = self
            .glottis
            .step(lambda1, self.rng.bipolar(), &self.simplex);
        let frication = self.frication.process(self.rng.bipolar()) * self.glottis.noise_modulator();
        let out = self.tract.step(glottal, frication, lambda1)
            + self.tract.step(glottal, frication, lambda2);
        let out = out * 0.125;
        if out.is_finite() {
            out
        } else {
            // Never let a NaN circulate in the waveguide.
            self.tract.reset_waves();
            self.glottis.reset();
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use crate::lpc;

    use super::*;
    use crate::tract::Constriction;
    use std::vec;
    use std::vec::Vec;

    fn render(t: &mut Trombone, seconds: f32) -> Vec<f32> {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let mut buf = vec![0.0; (seconds * t.sample_rate()) as usize];
        t.render(&mut buf);
        buf
    }

    fn rms(x: &[f32]) -> f32 {
        #[allow(clippy::cast_precision_loss)]
        let n = x.len() as f32;
        (x.iter().map(|v| v * v).sum::<f32>() / n).sqrt()
    }

    #[test]
    fn silent_until_voiced() {
        let mut t = Trombone::new(48_000.0);
        assert!(render(&mut t, 0.5).iter().all(|s| s.abs() < 1e-3));
    }

    #[test]
    fn voiced_output_is_audible_and_bounded() {
        for sr in [8_000.0, 22_050.0, 44_100.0, 48_000.0, 96_000.0, 192_000.0] {
            let mut t = Trombone::new(sr);
            t.note_on();
            let out = render(&mut t, 1.0);
            assert!(out.iter().all(|s| s.is_finite() && s.abs() < 1.5), "{sr}");
            assert!(rms(&out[out.len() / 2..]) > 0.02, "{sr}: too quiet");
        }
    }

    #[test]
    fn fades_out_after_note_off() {
        let mut t = Trombone::new(48_000.0);
        t.note_on();
        render(&mut t, 0.5);
        t.note_off();
        render(&mut t, 0.5);
        assert!(rms(&render(&mut t, 0.1)) < 1e-3);
    }

    #[test]
    fn same_seed_same_audio() {
        let mut a = Trombone::with_seed(44_100.0, 42);
        let mut b = Trombone::with_seed(44_100.0, 42);
        a.note_on();
        b.note_on();
        assert_eq!(render(&mut a, 0.3), render(&mut b, 0.3));
    }

    #[test]
    fn unvoiced_fricative_hisses() {
        let mut t = Trombone::new(48_000.0);
        t.articulate_phoneme(Phoneme::S);
        let out = render(&mut t, 0.5);
        assert!(rms(&out[out.len() / 2..]) > 0.003, "{}", rms(&out));
    }

    #[test]
    fn releasing_a_closure_makes_a_burst() {
        let mut t = Trombone::new(48_000.0);
        t.set_constriction(SLOT_MAIN, Some(Constriction::new(41.0, 0.0)));
        render(&mut t, 0.5);
        let before = rms(&render(&mut t, 0.05));
        t.set_constriction(SLOT_MAIN, None);
        let after = render(&mut t, 0.2);
        let peak = after.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            peak > 10.0 * before.max(1e-4),
            "peak {peak}, before {before}"
        );
    }

    #[test]
    fn velum_opens_and_closes() {
        let mut t = Trombone::new(48_000.0);
        t.set_velum(1.0);
        render(&mut t, 0.5);
        assert!(t.velum() > 0.95);
        t.set_velum(0.0);
        render(&mut t, 0.5);
        assert!(t.velum() < 0.05);
    }

    #[test]
    fn rejects_garbage_parameters() {
        let mut t = Trombone::new(48_000.0);
        t.set_frequency(f32::NAN);
        t.set_tongue(f32::INFINITY, 2.0);
        t.set_constriction(0, Some(Constriction::new(f32::NAN, f32::NAN)));
        t.set_constriction(99, Some(Constriction::new(20.0, 0.0)));
        t.set_tenseness(f32::NAN);
        t.note_on();
        assert!(render(&mut t, 0.3).iter().all(|s| s.is_finite()));
    }

    /// F1 and F2 of a sustained phoneme, by LPC.
    fn formants(p: Phoneme, tract_length: f32) -> (f32, f32) {
        let sr = 16_000.0;
        let mut t = Trombone::new(sr);
        t.set_tract_length(tract_length);
        t.set_frequency(100.0);
        t.set_vibrato(0.0, 0.0);
        t.articulate_phoneme(p);
        t.note_on();
        render(&mut t, 0.6);
        let f = lpc::formants(&render(&mut t, 0.4), sr, 5000.0);
        (f[0], f[1])
    }

    fn vowel(p: Phoneme) -> (f32, f32) {
        formants(p, 1.0)
    }

    #[test]
    fn shorter_tract_raises_formants() {
        let (m1, m2) = formants(Phoneme::Aa, 1.0);
        let (c1, c2) = formants(Phoneme::Aa, 0.7);
        // Formants scale with 1 / length: about +43 % for 0.7.
        assert!(
            c1 > m1 * 1.3 && c2 > m2 * 1.3,
            "man {m1}/{m2}, child {c1}/{c2}"
        );
    }

    #[test]
    fn tract_length_one_is_the_default() {
        let mut a = Trombone::with_seed(44_100.0, 3);
        let mut b = Trombone::with_seed(44_100.0, 3);
        b.set_tract_length(0.8);
        b.set_tract_length(1.0);
        a.note_on();
        b.note_on();
        assert_eq!(render(&mut a, 0.2), render(&mut b, 0.2));
    }

    #[test]
    fn vowels_have_the_right_formant_layout() {
        let (i1, i2) = vowel(Phoneme::Iy);
        let (a1, a2) = vowel(Phoneme::Aa);
        let (u1, u2) = vowel(Phoneme::Uw);
        std::println!("IY {i1}/{i2}  AA {a1}/{a2}  UW {u1}/{u2}");
        // The vowel triangle: /a/ is open (high F1), /i/ front (high F2),
        // /u/ close and back (low F1, low F2).
        assert!(a1 > i1 && a1 > u1, "F1: AA {a1} IY {i1} UW {u1}");
        assert!(i2 > a2 && i2 > u2, "F2: IY {i2} AA {a2} UW {u2}");
    }
}
