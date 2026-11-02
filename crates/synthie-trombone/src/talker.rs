//! [`Talker`]: a voice you tune once and then hand sentences to.

use crate::Trombone;
use crate::preset::{Preset, VoiceSettings};
use crate::speech::{Speaker, Utterance};
use alloc::collections::VecDeque;
use alloc::vec::Vec;

/// Seconds rendered after the last segment so the voice can fade out.
const TAIL: f32 = 0.25;

/// A tuned voice that speaks queued sentences.
///
/// Use it two ways:
///
/// - **Bake:** [`bake`](Self::bake) renders a sentence to a mono buffer, to
///   play with any audio engine (kira, rodio, a game toolkit's
///   `play_samples`) or to save. This is the easy path for games.
/// - **Stream:** [`say`](Self::say) queues text and [`render`](Self::render)
///   produces samples as they are needed, for audio callbacks and live
///   dialogue.
///
/// ```
/// use synthie_trombone::{Preset, Talker};
///
/// let mut goblin = Talker::new(44_100.0, Preset::Critter);
/// let line: Vec<f32> = goblin.bake("Shiny! Mine, all mine!");
/// assert!(!line.is_empty());
/// ```
///
/// Compiling text allocates. When streaming from a real-time callback,
/// compile with [`compile`](Self::compile) elsewhere and pass the result to
/// [`say_utterance`](Self::say_utterance). Finished utterances are freed
/// where `render` runs.
#[derive(Clone, Debug)]
pub struct Talker {
    voice: Trombone,
    speaker: Speaker,
    settings: VoiceSettings,
    queue: VecDeque<Utterance>,
}

impl Talker {
    /// A talker at `sample_rate` with the given voice.
    ///
    /// # Panics
    ///
    /// Panics if `sample_rate` is not positive and finite.
    #[must_use]
    pub fn new(sample_rate: f32, settings: impl Into<VoiceSettings>) -> Self {
        let mut voice = Trombone::new(sample_rate);
        Speaker::prepare(&mut voice);
        let settings = settings.into();
        settings.apply(&mut voice);
        Self {
            voice,
            speaker: Speaker::new(),
            settings,
            queue: VecDeque::new(),
        }
    }

    /// A talker with one of the built-in voices.
    #[must_use]
    pub fn with_preset(sample_rate: f32, preset: Preset) -> Self {
        Self::new(sample_rate, preset)
    }

    /// Output sample rate.
    #[must_use]
    pub fn sample_rate(&self) -> f32 {
        self.voice.sample_rate()
    }

    /// The voice settings.
    #[must_use]
    pub fn settings(&self) -> &VoiceSettings {
        &self.settings
    }

    /// Change the voice. Affects sentences compiled from now on.
    pub fn set_settings(&mut self, settings: impl Into<VoiceSettings>) {
        self.settings = settings.into();
        self.settings.apply(&mut self.voice);
    }

    /// The underlying voice, for direct articulation control.
    pub fn voice_mut(&mut self) -> &mut Trombone {
        &mut self.voice
    }

    /// Compile text with this talker's voice, without queueing it.
    #[must_use]
    pub fn compile(&self, text: &str) -> Utterance {
        Utterance::from_text(text, &self.settings.prosody())
    }

    /// Queue text to be spoken after anything already queued.
    pub fn say(&mut self, text: &str) {
        let u = self.compile(text);
        self.say_utterance(u);
    }

    /// Queue a compiled utterance.
    pub fn say_utterance(&mut self, utterance: Utterance) {
        if !utterance.is_empty() {
            self.queue.push_back(utterance);
        }
    }

    /// Stop now and forget the queue; the voice fades out.
    pub fn interrupt(&mut self) {
        self.queue.clear();
        self.speaker.stop(&mut self.voice);
    }

    /// Whether anything is being said or waiting to be said.
    #[must_use]
    pub fn is_speaking(&self) -> bool {
        self.speaker.is_speaking() || !self.queue.is_empty()
    }

    /// One output sample.
    pub fn process(&mut self) -> f32 {
        if !self.speaker.is_speaking()
            && let Some(next) = self.queue.pop_front()
        {
            self.speaker.speak(next);
        }
        self.speaker.process(&mut self.voice)
    }

    /// Fill `out` with mono samples.
    pub fn render(&mut self, out: &mut [f32]) {
        for s in out {
            *s = self.process();
        }
    }

    /// Render `text` to a mono buffer at [`sample_rate`](Self::sample_rate),
    /// from silence, including a short fade-out tail. Drops anything queued.
    ///
    /// The level follows the voice (a whisper is quiet); see
    /// [`normalize`] for a uniform peak.
    #[must_use]
    pub fn bake(&mut self, text: &str) -> Vec<f32> {
        let u = self.compile(text);
        self.bake_utterance(u)
    }

    /// [`bake`](Self::bake) for a compiled utterance.
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn bake_utterance(&mut self, utterance: Utterance) -> Vec<f32> {
        self.queue.clear();
        self.speaker.stop(&mut self.voice);
        self.voice.reset();
        let len = ((utterance.duration() + TAIL) * self.sample_rate()) as usize;
        let mut out = alloc::vec![0.0; len];
        self.speaker.speak(utterance);
        self.speaker.render(&mut self.voice, &mut out);
        out
    }
}

/// Scale `samples` so the loudest one reaches `peak` (e.g. `0.9`). Silence
/// is left alone.
pub fn normalize(samples: &mut [f32], peak: f32) {
    let max = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if max > 1e-6 {
        let gain = peak / max;
        for s in samples {
            *s *= gain;
        }
    }
}

/// Render `text` in one go: `bake(text, Preset::Bright, 44_100.0)`.
#[must_use]
pub fn bake(text: &str, voice: impl Into<VoiceSettings>, sample_rate: f32) -> Vec<f32> {
    Talker::new(sample_rate, voice).bake(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(x: &[f32]) -> f32 {
        #[allow(clippy::cast_precision_loss)]
        let n = x.len().max(1) as f32;
        (x.iter().map(|v| v * v).sum::<f32>() / n).sqrt()
    }

    #[test]
    fn every_preset_speaks() {
        for p in Preset::ALL {
            let out = bake("Hello there, how are you?", p, 22_050.0);
            assert!(out.iter().all(|s| s.is_finite()), "{p}");
            assert!(rms(&out) > 0.002, "{p} too quiet: {}", rms(&out));
            assert!(out.iter().all(|s| s.abs() < 2.0), "{p} too loud");
        }
    }

    #[test]
    fn faster_presets_are_shorter() {
        let giant = bake("The quick brown fox.", Preset::Giant, 16_000.0);
        let critter = bake("The quick brown fox.", Preset::Critter, 16_000.0);
        assert!(critter.len() * 3 < giant.len() * 2);
    }

    #[test]
    fn bake_is_repeatable() {
        let mut t = Talker::new(16_000.0, Preset::Bright);
        let a = t.bake("Again.");
        let b = t.bake("Again.");
        assert_eq!(a.len(), b.len());
    }

    #[test]
    fn queued_sentences_play_in_order_then_stop() {
        let mut t = Talker::new(16_000.0, Preset::Robot);
        t.say("One.");
        t.say("Two.");
        assert!(t.is_speaking());
        let mut buf = alloc::vec![0.0; 16_000 * 4];
        t.render(&mut buf);
        assert!(!t.is_speaking());
        t.say("Three.");
        t.interrupt();
        assert!(!t.is_speaking());
    }

    #[test]
    fn presets_round_trip_by_name() {
        for p in Preset::ALL {
            assert_eq!(Preset::from_name(p.name()), Some(p));
        }
        assert_eq!(Preset::from_name("CRITTER"), Some(Preset::Critter));
    }

    #[test]
    fn normalize_hits_the_peak() {
        let mut x = [0.1, -0.4, 0.2];
        normalize(&mut x, 0.8);
        assert!((x[1] + 0.8).abs() < 1e-6);
        let mut silent = [0.0; 4];
        normalize(&mut silent, 0.8);
        assert_eq!(silent, [0.0; 4]);
    }
}
