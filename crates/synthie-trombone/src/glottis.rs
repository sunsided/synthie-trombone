//! Glottal source: Liljencrants–Fant (LF) pulse train plus aspiration noise.
//!
//! Runs at [`INTERNAL_RATE`](crate::INTERNAL_RATE). Parameters change once
//! per block and are interpolated per pulse, as in the original.

use crate::INTERNAL_RATE;
use crate::math::{exp, floor_i64, lerp, ln, powf, sin, sqrt};
use crate::noise::{BandPass, Simplex};
use core::f32::consts::PI;

/// The original's onset: 0.13 per 512-sample block.
pub(crate) const DEFAULT_ATTACK: f32 = 0.13 * INTERNAL_RATE / 512.0;
/// The original's offset: 0.05 per 512-sample block.
pub(crate) const DEFAULT_RELEASE: f32 = 0.05 * INTERNAL_RATE / 512.0;

/// The vocal folds.
#[derive(Clone, Debug)]
pub(crate) struct Glottis {
    /// Voicing gate: the folds are pushed together and vibrate.
    pub voiced: bool,
    /// Keep phonating regardless of the gate (the original's default).
    pub always_voice: bool,
    /// Large, slow random pitch drift (the original's "auto wobble").
    pub wobble: bool,
    pub target_tenseness: f32,
    pub target_frequency: f32,
    pub vibrato_amount: f32,
    pub vibrato_frequency: f32,
    /// Glissando speed: relative pitch change per 512-sample block.
    pub glide: f32,
    /// Scale of the small random pitch jitter (`1.0` natural, `0.0` robotic).
    pub jitter: f32,
    /// Voicing onset rate, intensity per second.
    pub attack: f32,
    /// Voicing offset rate, intensity per second.
    pub release: f32,

    pub intensity: f32,
    pub loudness: f32,
    smooth_frequency: f32,
    old_frequency: f32,
    new_frequency: f32,
    old_tenseness: f32,
    new_tenseness: f32,

    time_in_waveform: f32,
    waveform_length: f32,
    total_time: f64,

    aspiration: BandPass,
    /// Seconds per sample at the current internal rate.
    dt: f32,
    rate: f32,

    // LF waveform shape for the current pulse.
    alpha: f32,
    e0: f32,
    epsilon: f32,
    shift: f32,
    delta: f32,
    te: f32,
    omega: f32,
}

impl Glottis {
    pub(crate) fn new() -> Self {
        let mut g = Self {
            voiced: false,
            always_voice: false,
            wobble: false,
            target_tenseness: 0.6,
            target_frequency: 140.0,
            vibrato_amount: 0.005,
            vibrato_frequency: 6.0,
            glide: 0.1,
            jitter: 1.0,
            attack: DEFAULT_ATTACK,
            release: DEFAULT_RELEASE,
            intensity: 0.0,
            loudness: 1.0,
            smooth_frequency: 140.0,
            old_frequency: 140.0,
            new_frequency: 140.0,
            old_tenseness: 0.6,
            new_tenseness: 0.6,
            time_in_waveform: 0.0,
            waveform_length: 1.0 / 140.0,
            total_time: 0.0,
            aspiration: BandPass::new(500.0, 0.5, INTERNAL_RATE),
            dt: 1.0 / INTERNAL_RATE,
            rate: INTERNAL_RATE,
            alpha: 0.0,
            e0: 0.0,
            epsilon: 0.0,
            shift: 0.0,
            delta: 0.0,
            te: 0.0,
            omega: 0.0,
        };
        g.setup_waveform(0.0);
        g
    }

    pub(crate) fn reset(&mut self) {
        let keep = (
            self.voiced,
            self.always_voice,
            self.wobble,
            self.target_tenseness,
            self.target_frequency,
            self.vibrato_amount,
            self.vibrato_frequency,
            self.glide,
            self.attack,
            self.release,
            self.jitter,
        );
        let rate = self.rate;
        *self = Self::new();
        self.set_rate(rate);
        (
            self.voiced,
            self.always_voice,
            self.wobble,
            self.target_tenseness,
            self.target_frequency,
            self.vibrato_amount,
            self.vibrato_frequency,
            self.glide,
            self.attack,
            self.release,
            self.jitter,
        ) = keep;
        self.aspiration.reset();
    }

    /// Change the sample rate the folds run at (for vocal tract length).
    pub(crate) fn set_rate(&mut self, rate: f32) {
        if rate.to_bits() != self.rate.to_bits() {
            self.rate = rate;
            self.dt = 1.0 / rate;
            self.aspiration = BandPass::new(500.0, 0.5, rate);
        }
    }

    /// Current pitch in Hz, after glide and vibrato.
    pub(crate) fn frequency(&self) -> f32 {
        self.new_frequency
    }

    /// One glottal sample. `white` is a white noise sample, `lambda` the
    /// position within the current parameter block.
    pub(crate) fn step(&mut self, lambda: f32, white: f32, simplex: &Simplex) -> f32 {
        let dt = self.dt;
        if self.time_in_waveform > self.waveform_length {
            self.time_in_waveform -= self.waveform_length;
            self.setup_waveform(lambda);
        }
        let voice = self.lf_waveform(self.time_in_waveform / self.waveform_length);
        let noise = self.aspiration.process(white);
        let aspiration = self.intensity
            * (1.0 - sqrt(self.target_tenseness.clamp(0.0, 1.0)))
            * self.noise_modulator()
            * noise
            * (0.2 + 0.02 * simplex.sample(self.total_time * 1.99));
        self.time_in_waveform += dt;
        self.total_time += f64::from(dt);
        voice + aspiration
    }

    /// Breath turbulence follows the glottal cycle when voiced and is steady
    /// when not.
    pub(crate) fn noise_modulator(&self) -> f32 {
        let phase = 2.0 * PI * self.time_in_waveform / self.waveform_length;
        let voiced = 0.1 + 0.2 * sin(phase).max(0.0);
        let ti = self.target_tenseness * self.intensity;
        ti * voiced + (1.0 - ti) * 0.3
    }

    /// Per-block update; `dt` is the block length in seconds.
    pub(crate) fn update_block(&mut self, dt: f32, simplex: &Simplex) {
        let blocks = dt * INTERNAL_RATE / 512.0;
        if self.voiced || self.always_voice {
            self.intensity += self.attack * dt;
        } else {
            self.intensity -= self.release * dt;
        }
        self.intensity = self.intensity.clamp(0.0, 1.0);

        let t = self.total_time + f64::from(dt);

        // Pitch: exponential glide towards the target, then vibrato.
        let rate = 1.0 + self.glide * blocks;
        if self.intensity == 0.0 {
            self.smooth_frequency = self.target_frequency;
        } else if self.target_frequency > self.smooth_frequency {
            self.smooth_frequency = self.target_frequency.min(self.smooth_frequency * rate);
        } else if self.target_frequency < self.smooth_frequency {
            self.smooth_frequency = self.target_frequency.max(self.smooth_frequency / rate);
        }
        self.old_frequency = self.new_frequency;
        self.new_frequency = (self.smooth_frequency * (1.0 + self.vibrato(t, simplex))).max(10.0);

        // Tenseness with a little drift.
        self.old_tenseness = self.new_tenseness;
        let mut tense = self.target_tenseness
            + 0.1 * simplex.sample(t * 0.46)
            + 0.05 * simplex.sample(t * 0.36);
        tense = tense.max(0.0);
        if self.always_voice && !self.voiced {
            // The original's soft attack while "untouched".
            tense += (3.0 - self.target_tenseness) * (1.0 - self.intensity);
        }
        self.new_tenseness = tense;
    }

    fn vibrato(&self, t: f64, simplex: &Simplex) -> f32 {
        let cycles = t * f64::from(self.vibrato_frequency);
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
        let phase = (cycles - floor_i64(cycles) as f64) as f32;
        let mut v = self.vibrato_amount * sin(2.0 * PI * phase);
        v += self.jitter * 0.02 * simplex.sample(t * 4.07);
        v += self.jitter * 0.04 * simplex.sample(t * 2.15);
        if self.wobble {
            v += 0.2 * simplex.sample(t * 0.98);
            v += 0.4 * simplex.sample(t * 0.5);
        }
        v
    }

    /// Precompute the LF model for one pulse (normalised to `T = 1`, `Ee = 1`).
    fn setup_waveform(&mut self, lambda: f32) {
        let frequency = lerp(self.old_frequency, self.new_frequency, lambda);
        let tenseness = lerp(self.old_tenseness, self.new_tenseness, lambda);
        self.waveform_length = 1.0 / frequency;
        self.loudness = powf(tenseness.max(0.0), 0.25);

        let rd = (3.0 * (1.0 - tenseness)).clamp(0.5, 2.7);
        let ra = -0.01 + 0.048 * rd;
        let rk = 0.224 + 0.118 * rd;
        let rg = (rk / 4.0) * (0.5 + 1.2 * rk) / (0.11 * rd - ra * (0.5 + 1.2 * rk));

        let ta = ra;
        let tp = 1.0 / (2.0 * rg);
        let te = tp + tp * rk;

        let epsilon = 1.0 / ta;
        let shift = exp(-epsilon * (1.0 - te));
        let delta = 1.0 - shift;

        let rhs_integral = ((1.0 / epsilon) * (shift - 1.0) + (1.0 - te) * shift) / delta;
        let total_lower_integral = rhs_integral - (te - tp) / 2.0;
        let total_upper_integral = -total_lower_integral;

        let omega = PI / tp;
        let s = sin(omega * te);
        // E0·e^(α·Te)·s = −1 and E0·e^(α·Tp/2)·Tp·2/π = upper integral.
        let y = -PI * s * total_upper_integral / (tp * 2.0);
        let alpha = ln(y) / (tp / 2.0 - te);
        let e0 = -1.0 / (s * exp(alpha * te));

        self.alpha = alpha;
        self.e0 = e0;
        self.epsilon = epsilon;
        self.shift = shift;
        self.delta = delta;
        self.te = te;
        self.omega = omega;
    }

    fn lf_waveform(&self, t: f32) -> f32 {
        let out = if t > self.te {
            (-exp(-self.epsilon * (t - self.te)) + self.shift) / self.delta
        } else {
            self.e0 * exp(self.alpha * t) * sin(self.omega * t)
        };
        out * self.intensity * self.loudness
    }
}
