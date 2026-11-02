//! Voice character: [`VoiceSettings`] and ready-made [`Preset`]s.

use crate::Trombone;

/// Everything that makes one speaker sound different from another.
///
/// Apply it to a [`Trombone`] with [`apply`](Self::apply); the speech
/// front end takes its intonation from [`pitch`](Self::pitch),
/// [`expressiveness`](Self::expressiveness), [`rate`](Self::rate) and
/// [`nasality`](Self::nasality). [`Talker`](crate::Talker) does both.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct VoiceSettings {
    /// Speaking pitch in Hz (the presets span 62 to 360).
    pub pitch: f32,
    /// Vocal tract length relative to the model's native tract (about
    /// 17 cm); shorter raises every formant and sounds like a smaller
    /// speaker (`0.86` bright, `0.72` child). `0.5‥1.6`.
    pub tract_length: f32,
    /// Glottal tenseness: `0` whisper, `0.4` breathy, `0.65` modal, `0.9`
    /// pressed.
    pub tenseness: f32,
    /// Pitch movement on accents and phrase ends: `0` monotone, `1` lively.
    pub expressiveness: f32,
    /// Speaking rate multiplier.
    pub rate: f32,
    /// Vibrato depth, relative pitch.
    pub vibrato: f32,
    /// Vibrato rate in Hz.
    pub vibrato_rate: f32,
    /// Random pitch jitter: `0` robotic, `1` natural, `2+` shaky.
    pub jitter: f32,
    /// Constant velum opening during vowels: a nasal, whiny voice. `0‥1`.
    pub nasality: f32,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Preset::Deep.settings()
    }
}

impl VoiceSettings {
    /// Configure `voice` with these settings (voice quality only; pitch is
    /// set per segment while speaking).
    pub fn apply(&self, voice: &mut Trombone) {
        voice.set_tract_length(self.tract_length);
        voice.set_tenseness(self.tenseness);
        voice.set_vibrato(self.vibrato, self.vibrato_rate);
        voice.set_jitter(self.jitter);
        voice.set_frequency(self.pitch);
    }

    /// Intonation settings for the speech compiler.
    #[cfg(feature = "alloc")]
    #[must_use]
    pub fn prosody(&self) -> crate::speech::Prosody {
        crate::speech::Prosody {
            pitch: self.pitch,
            expressiveness: self.expressiveness,
            rate: self.rate,
            tenseness: self.tenseness,
            nasality: self.nasality,
        }
    }

    /// The same voice at a different pitch, keeping everything else.
    #[must_use]
    pub fn with_pitch(mut self, hz: f32) -> Self {
        self.pitch = hz;
        self
    }

    /// The same voice speaking faster (`> 1`) or slower.
    #[must_use]
    pub fn with_rate(mut self, rate: f32) -> Self {
        self.rate = rate;
        self
    }
}

/// Ready-made voices. Start from one and tweak the [`VoiceSettings`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Preset {
    /// Low and full: the model's native tract at about 105 Hz.
    Deep,
    /// Higher pitch, shorter tract, livelier intonation.
    Bright,
    /// Child: high pitch, short tract.
    Child,
    /// Old: breathy, slow, with a tremor.
    Elder,
    /// Huge, slow and deep: a long tract and a low pitch.
    Giant,
    /// Small, squeaky, fast and excitable: cartoon sidekicks and gremlins.
    Critter,
    /// Monotone, steady, pressed.
    Robot,
    /// No voicing, only breath.
    Whisper,
    /// A computer singing: bright, pressed, perfectly steady pitch, no
    /// vibrato. Made for [`sing`](crate::sing) with hard pitch steps.
    Android,
}

impl Preset {
    /// Every preset.
    pub const ALL: [Preset; 9] = [
        Preset::Deep,
        Preset::Bright,
        Preset::Child,
        Preset::Elder,
        Preset::Giant,
        Preset::Critter,
        Preset::Robot,
        Preset::Whisper,
        Preset::Android,
    ];

    /// Lowercase name, e.g. `"critter"`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Preset::Deep => "deep",
            Preset::Bright => "bright",
            Preset::Child => "child",
            Preset::Elder => "elder",
            Preset::Giant => "giant",
            Preset::Critter => "critter",
            Preset::Robot => "robot",
            Preset::Whisper => "whisper",
            Preset::Android => "android",
        }
    }

    /// Look a preset up by name (case-insensitive).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|p| p.name().eq_ignore_ascii_case(name))
    }

    /// The preset's settings.
    #[must_use]
    pub const fn settings(self) -> VoiceSettings {
        let base = VoiceSettings {
            pitch: 105.0,
            tract_length: 1.0,
            tenseness: 0.65,
            expressiveness: 0.5,
            rate: 1.0,
            vibrato: 0.005,
            vibrato_rate: 5.5,
            jitter: 1.0,
            nasality: 0.0,
        };
        match self {
            Preset::Deep => base,
            Preset::Bright => VoiceSettings {
                pitch: 210.0,
                tract_length: 0.86,
                tenseness: 0.6,
                expressiveness: 0.7,
                rate: 1.05,
                vibrato: 0.006,
                vibrato_rate: 5.8,
                ..base
            },
            Preset::Child => VoiceSettings {
                pitch: 290.0,
                tract_length: 0.72,
                tenseness: 0.6,
                expressiveness: 0.85,
                rate: 1.1,
                vibrato_rate: 6.0,
                jitter: 1.2,
                nasality: 0.05,
                ..base
            },
            Preset::Elder => VoiceSettings {
                pitch: 118.0,
                tract_length: 1.02,
                tenseness: 0.42,
                expressiveness: 0.45,
                rate: 0.8,
                vibrato: 0.03,
                vibrato_rate: 5.0,
                jitter: 2.5,
                nasality: 0.1,
            },
            Preset::Giant => VoiceSettings {
                pitch: 62.0,
                tract_length: 1.4,
                tenseness: 0.75,
                expressiveness: 0.4,
                rate: 0.75,
                vibrato: 0.004,
                vibrato_rate: 4.0,
                jitter: 0.8,
                ..base
            },
            Preset::Critter => VoiceSettings {
                pitch: 360.0,
                tract_length: 0.6,
                tenseness: 0.75,
                expressiveness: 1.4,
                rate: 1.4,
                vibrato: 0.012,
                vibrato_rate: 7.5,
                jitter: 1.5,
                nasality: 0.15,
            },
            Preset::Robot => VoiceSettings {
                pitch: 95.0,
                tract_length: 0.95,
                tenseness: 0.85,
                expressiveness: 0.0,
                rate: 0.95,
                vibrato: 0.0,
                vibrato_rate: 0.0,
                jitter: 0.0,
                ..base
            },
            Preset::Android => VoiceSettings {
                pitch: 220.0,
                tract_length: 0.84,
                tenseness: 0.8,
                expressiveness: 0.2,
                rate: 1.0,
                vibrato: 0.0,
                vibrato_rate: 0.0,
                jitter: 0.0,
                nasality: 0.06,
            },
            Preset::Whisper => VoiceSettings {
                pitch: 140.0,
                tract_length: 0.9,
                tenseness: 0.0,
                expressiveness: 0.3,
                rate: 0.9,
                vibrato: 0.0,
                ..base
            },
        }
    }
}

impl From<Preset> for VoiceSettings {
    fn from(p: Preset) -> Self {
        p.settings()
    }
}

impl core::fmt::Display for Preset {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.name())
    }
}
