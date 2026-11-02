//! WebAssembly engine for the synthie-trombone web app.
//!
//! One [`Engine`] lives in an AudioWorklet: a [`Trombone`] played like an
//! instrument, a [`Speaker`] for typed text, and synthie's reverb. The main
//! thread drives it with setters and reads the tract shape back through the
//! `fill_*` methods for drawing.

use synthie::audio::fx::Reverb;
use synthie_trombone::speech::{Speaker, Utterance};
use synthie_trombone::{
    BLADE_START, Constriction, LIP_START, N, NOSE_LEN, NOSE_START, Preset, TIP_START, Trombone,
    VoiceSettings, midi_to_hz,
};
use wasm_bindgen::prelude::*;

/// Constriction slots the speaker leaves alone: one per touch.
const TOUCH_SLOTS: [usize; 2] = [2, 3];

/// The sound engine: one voice, speech, reverb.
#[wasm_bindgen]
pub struct Engine {
    voice: Trombone,
    speaker: Speaker,
    reverb: Reverb,
    reverb_mix: f32,
    settings: VoiceSettings,
    /// Instrument tenseness to restore after speaking.
    tenseness: f32,
    velum: f32,
    wobble: bool,
    /// Phonate continuously ("keep the voice on"); speech turns it off
    /// while it talks, so it is restored afterwards.
    drone: bool,
    was_speaking: bool,
    symbols: Vec<&'static str>,
    durations: Vec<f32>,
}

#[wasm_bindgen]
impl Engine {
    /// A silent engine at the given sample rate.
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new(sample_rate: f32) -> Engine {
        let settings = Preset::Deep.settings();
        let mut voice = Trombone::new(sample_rate);
        settings.apply(&mut voice);
        let mut reverb = Reverb::new();
        reverb.set_params(0.6, 0.45);
        Engine {
            voice,
            speaker: Speaker::new(),
            reverb,
            reverb_mix: 0.2,
            tenseness: settings.tenseness,
            settings,
            velum: 0.0,
            wobble: false,
            drone: false,
            was_speaking: false,
            symbols: Vec::new(),
            durations: Vec::new(),
        }
    }

    /// Fill `out` with mono samples.
    pub fn render(&mut self, out: &mut [f32]) {
        for s in out {
            let dry = self.speaker.process(&mut self.voice);
            if self.was_speaking && !self.speaker.is_speaking() {
                self.was_speaking = false;
                self.restore_instrument();
            }
            *s = (self.reverb.process(dry, self.reverb_mix) * 0.9).clamp(-1.0, 1.0);
        }
    }

    // ----- playing --------------------------------------------------------

    /// Start phonating at a (fractional) MIDI note.
    #[wasm_bindgen(js_name = noteOn)]
    pub fn note_on(&mut self, note: f32) {
        self.stop_speaking();
        self.voice.set_note(note);
        self.voice.note_on();
    }

    /// Stop phonating.
    #[wasm_bindgen(js_name = noteOff)]
    pub fn note_off(&mut self) {
        self.voice.note_off();
    }

    /// Change pitch without retriggering.
    #[wasm_bindgen(js_name = setNote)]
    pub fn set_note(&mut self, note: f32) {
        self.voice.set_note(note);
    }

    /// Tongue body position and height.
    #[wasm_bindgen(js_name = setTongue)]
    pub fn set_tongue(&mut self, index: f32, diameter: f32) {
        self.voice.set_tongue(index, diameter);
    }

    /// Place or move a finger (`touch` 0 or 1) on the tract.
    #[wasm_bindgen(js_name = setTouch)]
    pub fn set_touch(&mut self, touch: usize, index: f32, diameter: f32) {
        if let Some(&slot) = TOUCH_SLOTS.get(touch) {
            self.voice
                .set_constriction(slot, Some(Constriction::new(index, diameter)));
        }
    }

    /// Lift a finger.
    #[wasm_bindgen(js_name = releaseTouch)]
    pub fn release_touch(&mut self, touch: usize) {
        if let Some(&slot) = TOUCH_SLOTS.get(touch) {
            self.voice.set_constriction(slot, None);
        }
    }

    /// Shape the tract for a vowel or consonant (ARPAbet symbol).
    pub fn articulate(&mut self, symbol: &str) -> bool {
        match synthie_trombone::Phoneme::from_symbol(symbol) {
            Some(p) => {
                self.voice.articulate_phoneme(p);
                self.voice.set_velum(self.velum.max(p.articulation().velum));
                true
            }
            None => false,
        }
    }

    /// Velum opening, `0` (oral) to `1` (nasal).
    #[wasm_bindgen(js_name = setVelum)]
    pub fn set_velum(&mut self, openness: f32) {
        self.velum = openness;
        self.voice.set_velum(openness);
    }

    /// Glottal tenseness.
    #[wasm_bindgen(js_name = setTenseness)]
    pub fn set_tenseness(&mut self, tenseness: f32) {
        self.tenseness = tenseness;
        self.settings.tenseness = tenseness;
        if !self.speaker.is_speaking() {
            self.voice.set_tenseness(tenseness);
        }
    }

    /// Vibrato depth.
    #[wasm_bindgen(js_name = setVibrato)]
    pub fn set_vibrato(&mut self, amount: f32) {
        self.settings.vibrato = amount;
        self.voice.set_vibrato(amount, self.settings.vibrato_rate);
    }

    /// The original's slow pitch wobble.
    #[wasm_bindgen(js_name = setWobble)]
    pub fn set_wobble(&mut self, on: bool) {
        self.wobble = on;
        self.voice.set_wobble(on);
    }

    /// Phonate continuously, like the original web toy.
    #[wasm_bindgen(js_name = setDrone)]
    pub fn set_drone(&mut self, on: bool) {
        self.drone = on;
        if !self.speaker.is_speaking() {
            self.voice.set_always_voice(on);
        }
    }

    /// Reverb amount, `0‥1`.
    #[wasm_bindgen(js_name = setReverb)]
    pub fn set_reverb(&mut self, mix: f32) {
        self.reverb_mix = mix.clamp(0.0, 1.0);
    }

    // ----- voice ------------------------------------------------------------

    /// Names of the built-in voices.
    #[wasm_bindgen(js_name = presetNames)]
    #[must_use]
    pub fn preset_names() -> Vec<String> {
        Preset::ALL.iter().map(|p| p.name().to_string()).collect()
    }

    /// Switch to a built-in voice; returns its speaking pitch as a MIDI note.
    #[wasm_bindgen(js_name = setPreset)]
    pub fn set_preset(&mut self, name: &str) -> Option<f32> {
        let preset = Preset::from_name(name)?;
        self.settings = preset.settings();
        self.tenseness = self.settings.tenseness;
        self.settings.apply(&mut self.voice);
        Some(69.0 + 12.0 * (self.settings.pitch / 440.0).log2())
    }

    /// Current voice settings as `[pitch, tractLength, tenseness,
    /// expressiveness, rate, vibrato, jitter, nasality]`.
    #[must_use]
    pub fn settings(&self) -> Vec<f32> {
        let s = &self.settings;
        vec![
            s.pitch,
            s.tract_length,
            s.tenseness,
            s.expressiveness,
            s.rate,
            s.vibrato,
            s.jitter,
            s.nasality,
        ]
    }

    /// Vocal tract length relative to the native tract.
    #[wasm_bindgen(js_name = setTractLength)]
    pub fn set_tract_length(&mut self, length: f32) {
        self.settings.tract_length = length;
        self.voice.set_tract_length(length);
    }

    /// Random pitch jitter.
    #[wasm_bindgen(js_name = setJitter)]
    pub fn set_jitter(&mut self, amount: f32) {
        self.settings.jitter = amount;
        self.voice.set_jitter(amount);
    }

    /// Nasality while speaking.
    #[wasm_bindgen(js_name = setNasality)]
    pub fn set_nasality(&mut self, amount: f32) {
        self.settings.nasality = amount;
    }

    /// Speaking rate.
    #[wasm_bindgen(js_name = setRate)]
    pub fn set_rate(&mut self, rate: f32) {
        self.settings.rate = rate;
    }

    /// Intonation range.
    #[wasm_bindgen(js_name = setExpressiveness)]
    pub fn set_expressiveness(&mut self, amount: f32) {
        self.settings.expressiveness = amount;
    }

    // ----- speech -----------------------------------------------------------

    /// Speak `text` at `pitch` Hz; returns the number of segments.
    pub fn speak(&mut self, text: &str, pitch: f32) -> usize {
        let mut prosody = self.settings.prosody();
        prosody.pitch = pitch;
        let utterance = Utterance::from_text(text, &prosody);
        self.symbols = utterance
            .segments()
            .iter()
            .map(|s| s.phoneme.symbol())
            .collect();
        self.durations = utterance.segments().iter().map(|s| s.duration).collect();
        Speaker::prepare(&mut self.voice);
        self.speaker.speak(utterance);
        self.was_speaking = true;
        self.symbols.len()
    }

    /// ARPAbet symbols of the last utterance, one per segment.
    #[must_use]
    pub fn symbols(&self) -> Vec<String> {
        self.symbols.iter().map(|s| (*s).to_string()).collect()
    }

    /// Segment durations of the last utterance, in seconds.
    #[must_use]
    pub fn durations(&self) -> Vec<f32> {
        self.durations.clone()
    }

    /// Index of the segment being spoken, or `-1`.
    #[wasm_bindgen(js_name = speakingIndex)]
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    pub fn speaking_index(&self) -> i32 {
        self.speaker.current().map_or(-1, |(i, _)| i as i32)
    }

    /// Stop speaking.
    #[wasm_bindgen(js_name = stopSpeaking)]
    pub fn stop_speaking(&mut self) {
        if self.speaker.is_speaking() {
            self.speaker.stop(&mut self.voice);
        }
    }

    // ----- state for drawing -------------------------------------------------

    /// Copy the oral tract diameters into `out` (length [`Engine::cells`]).
    #[wasm_bindgen(js_name = fillDiameters)]
    pub fn fill_diameters(&self, out: &mut [f32]) {
        copy(self.voice.diameters(), out);
    }

    /// Copy the target diameters into `out`.
    #[wasm_bindgen(js_name = fillTargets)]
    pub fn fill_targets(&self, out: &mut [f32]) {
        copy(self.voice.target_diameters(), out);
    }

    /// Copy the recent pressure peaks per cell into `out`.
    #[wasm_bindgen(js_name = fillAmplitudes)]
    pub fn fill_amplitudes(&self, out: &mut [f32]) {
        copy(self.voice.amplitudes(), out);
    }

    /// Copy the nasal diameters into `out` (length [`Engine::nose_cells`]).
    #[wasm_bindgen(js_name = fillNose)]
    pub fn fill_nose(&self, out: &mut [f32]) {
        copy(self.voice.nose_diameters(), out);
    }

    /// Copy the nasal pressure peaks into `out`.
    #[wasm_bindgen(js_name = fillNoseAmplitudes)]
    pub fn fill_nose_amplitudes(&self, out: &mut [f32]) {
        copy(self.voice.nose_amplitudes(), out);
    }

    /// `[intensity, frequency, velum, tenseness, tongueIndex, tongueDiameter,
    /// tractLength]`.
    #[must_use]
    pub fn status(&self) -> Vec<f32> {
        let (ti, td) = self.voice.tongue();
        vec![
            self.voice.intensity(),
            self.voice.frequency(),
            self.voice.velum(),
            self.voice.tenseness(),
            ti,
            td,
            self.voice.tract_length(),
        ]
    }

    /// Oral tract cells.
    #[must_use]
    pub fn cells() -> usize {
        N
    }

    /// Nasal tract cells.
    #[wasm_bindgen(js_name = noseCells)]
    #[must_use]
    pub fn nose_cells() -> usize {
        NOSE_LEN
    }

    /// `[noseStart, bladeStart, tipStart, lipStart]` cell indices.
    #[must_use]
    pub fn landmarks() -> Vec<usize> {
        vec![NOSE_START, BLADE_START, TIP_START, LIP_START]
    }

    /// Hz for a (fractional) MIDI note.
    #[wasm_bindgen(js_name = midiToHz)]
    #[must_use]
    pub fn midi_to_hz(note: f32) -> f32 {
        midi_to_hz(note)
    }
}

impl Engine {
    fn restore_instrument(&mut self) {
        let v = &mut self.voice;
        v.reset_voicing_rates();
        v.set_articulation_speed(1.0);
        v.set_glide(0.1);
        v.set_wobble(self.wobble);
        v.set_tenseness(self.tenseness);
        v.set_velum(self.velum);
        v.set_always_voice(self.drone);
    }
}

fn copy(src: &[f32], out: &mut [f32]) {
    let n = src.len().min(out.len());
    out[..n].copy_from_slice(&src[..n]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speaks_and_returns_to_the_instrument() {
        let mut e = Engine::new(48_000.0);
        let n = e.speak("Hello there.", 110.0);
        assert!(n > 0 && e.symbols().len() == n);
        let mut buf = vec![0.0; 48_000 * 3];
        e.render(&mut buf);
        assert_eq!(e.speaking_index(), -1);
        assert!(buf.iter().any(|s| s.abs() > 0.01));
        assert!(buf.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn drone_comes_back_after_speaking() {
        let mut e = Engine::new(48_000.0);
        e.set_drone(true);
        let mut buf = vec![0.0; 48_000];
        e.render(&mut buf);
        assert!(e.status()[0] > 0.9, "drone voices before speaking");
        e.speak("Hi.", 110.0);
        let mut long = vec![0.0; 48_000 * 2];
        e.render(&mut long);
        assert_eq!(e.speaking_index(), -1);
        e.render(&mut buf);
        assert!(
            e.status()[0] > 0.9,
            "drone voices again after speaking: {}",
            e.status()[0]
        );
    }

    #[test]
    fn presets_report_their_pitch() {
        let mut e = Engine::new(48_000.0);
        let deep = e.set_preset("deep").unwrap();
        let critter = e.set_preset("critter").unwrap();
        assert!(critter > deep + 18.0, "deep {deep}, critter {critter}");
        assert!(e.set_preset("nope").is_none());
    }
}
