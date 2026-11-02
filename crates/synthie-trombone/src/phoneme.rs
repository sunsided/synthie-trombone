//! Phonemes as articulations: where the tongue sits, what closes the tract,
//! whether the velum is open and the folds vibrate.
//!
//! The set follows ARPAbet. Vowel positions were fitted by grid search to
//! Peterson & Barney's (1952) male F1/F2 averages (see the `tune_vowels`
//! example); consonants follow the original's tract diagram.

use crate::tract::Constriction;

/// How a phoneme is produced; drives timing in the speech front end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Manner {
    /// Open tract, voiced.
    Vowel,
    /// Vowel that glides from one target to another.
    Diphthong,
    /// Full closure, released with a burst.
    Stop,
    /// Full oral closure, velum open.
    Nasal,
    /// Narrow constriction, turbulent noise.
    Fricative,
    /// Stop released into a fricative.
    Affricate,
    /// Partial narrowing without turbulence.
    Approximant,
    /// Breathy glottal noise (`h`).
    Aspirate,
    /// Silence.
    Pause,
}

/// A target vocal-tract configuration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Articulation {
    /// Tongue body `(index, diameter)`, or `None` to keep the current one
    /// (the speech front end fills it from the neighbouring vowel).
    pub tongue: Option<(f32, f32)>,
    /// Main constriction (tongue tip, lips, or tongue back).
    pub constriction: Option<Constriction>,
    /// Lip rounding.
    pub lips: Option<Constriction>,
    /// Velum openness, `0` (oral) to `1` (nasal).
    pub velum: f32,
    /// Whether the vocal folds vibrate.
    pub voiced: bool,
    /// Glottal tenseness override; low values make breathy noise.
    pub tenseness: Option<f32>,
}

impl Articulation {
    const fn vowel(index: f32, diameter: f32) -> Self {
        Self {
            tongue: Some((index, diameter)),
            constriction: None,
            lips: None,
            velum: 0.0,
            voiced: true,
            tenseness: None,
        }
    }

    const fn rounded(mut self, lips: f32) -> Self {
        self.lips = Some(Constriction::new(LIPS, lips));
        self
    }

    const fn consonant(index: f32, diameter: f32, voiced: bool) -> Self {
        Self {
            tongue: None,
            constriction: Some(Constriction::new(index, diameter)),
            lips: None,
            velum: 0.0,
            voiced,
            tenseness: None,
        }
    }

    const fn nasal(mut self) -> Self {
        self.velum = 1.0;
        self
    }

    const fn with_tongue(mut self, index: f32, diameter: f32) -> Self {
        self.tongue = Some((index, diameter));
        self
    }
}

/// Lip cell for rounding and bilabials.
const LIPS: f32 = 41.0;
/// Upper teeth, for labiodentals and dentals.
const TEETH: f32 = 39.5;
/// Alveolar ridge.
const ALVEOLAR: f32 = 36.0;
/// Hard palate behind the ridge.
const POSTALVEOLAR: f32 = 33.0;
/// Soft palate.
const VELAR: f32 = 21.0;
/// Closed.
const SHUT: f32 = 0.0;
/// Narrow enough to hiss.
const HISS: f32 = 0.45;

macro_rules! phonemes {
    ($( $(#[$doc:meta])* $name:ident = $arpa:literal, $manner:ident; )*) => {
        /// An English phoneme (ARPAbet names).
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        #[allow(missing_docs)]
        pub enum Phoneme {
            $( $(#[$doc])* $name, )*
        }

        impl Phoneme {
            /// Every phoneme.
            pub const ALL: &'static [Phoneme] = &[$(Phoneme::$name,)*];

            /// ARPAbet symbol, e.g. `"AA"`.
            #[must_use]
            pub const fn symbol(self) -> &'static str {
                match self { $(Phoneme::$name => $arpa,)* }
            }

            /// Manner of articulation.
            #[must_use]
            pub const fn manner(self) -> Manner {
                match self { $(Phoneme::$name => Manner::$manner,)* }
            }

            /// Parse an ARPAbet symbol (case-insensitive, stress digits ignored).
            #[must_use]
            pub fn from_symbol(s: &str) -> Option<Self> {
                let s = s.trim_end_matches(|c: char| c.is_ascii_digit());
                Self::ALL.iter().copied().find(|p| p.symbol().eq_ignore_ascii_case(s))
            }
        }
    };
}

phonemes! {
    /// f**a**ther
    Aa = "AA", Vowel;
    /// c**a**t
    Ae = "AE", Vowel;
    /// b**u**t
    Ah = "AH", Vowel;
    /// th**ou**ght
    Ao = "AO", Vowel;
    /// **a**bout (schwa)
    Ax = "AX", Vowel;
    /// b**e**d
    Eh = "EH", Vowel;
    /// b**ir**d
    Er = "ER", Vowel;
    /// b**i**t
    Ih = "IH", Vowel;
    /// b**ee**t
    Iy = "IY", Vowel;
    /// b**oo**k
    Uh = "UH", Vowel;
    /// b**oo**t
    Uw = "UW", Vowel;
    /// b**i**te
    Ay = "AY", Diphthong;
    /// b**ou**t
    Aw = "AW", Diphthong;
    /// b**ai**t
    Ey = "EY", Diphthong;
    /// b**oa**t
    Ow = "OW", Diphthong;
    /// b**oy**
    Oy = "OY", Diphthong;
    P = "P", Stop;
    B = "B", Stop;
    T = "T", Stop;
    D = "D", Stop;
    K = "K", Stop;
    G = "G", Stop;
    M = "M", Nasal;
    N = "N", Nasal;
    /// si**ng**
    Ng = "NG", Nasal;
    F = "F", Fricative;
    V = "V", Fricative;
    /// **th**in
    Th = "TH", Fricative;
    /// **th**is
    Dh = "DH", Fricative;
    S = "S", Fricative;
    Z = "Z", Fricative;
    /// **sh**e
    Sh = "SH", Fricative;
    /// mea**s**ure
    Zh = "ZH", Fricative;
    /// **ch**in
    Ch = "CH", Affricate;
    /// **j**oy
    Jh = "JH", Affricate;
    L = "L", Approximant;
    R = "R", Approximant;
    W = "W", Approximant;
    Y = "Y", Approximant;
    Hh = "HH", Aspirate;
    /// Silence.
    Sil = "SIL", Pause;
}

impl Phoneme {
    /// The vowel targets of a diphthong, `None` for everything else.
    #[must_use]
    pub const fn glide(self) -> Option<(Phoneme, Phoneme)> {
        use Phoneme::{Aa, Ae, Ao, Aw, Ay, Eh, Ey, Ih, Iy, Ow, Oy, Uh};
        match self {
            Ay => Some((Aa, Iy)),
            Aw => Some((Ae, Uh)),
            Ey => Some((Eh, Ih)),
            Ow => Some((Ao, Uh)),
            Oy => Some((Ao, Iy)),
            _ => None,
        }
    }

    /// Whether the vocal folds vibrate.
    #[must_use]
    pub const fn is_voiced(self) -> bool {
        self.articulation().voiced
    }

    /// Target tract configuration. Diphthongs return their first target.
    #[must_use]
    #[allow(clippy::match_same_arms)] // a lookup table
    pub const fn articulation(self) -> Articulation {
        use Phoneme as P;
        match self {
            P::Iy => Articulation::vowel(28.0, 2.15).rounded(1.3),
            P::Ih => Articulation::vowel(29.0, 2.8).rounded(1.1),
            P::Eh => Articulation::vowel(28.0, 3.5).rounded(1.3),
            P::Ae => Articulation::vowel(22.0, 2.95),
            P::Aa => Articulation::vowel(16.5, 2.25),
            P::Ao => Articulation::vowel(16.0, 2.15).rounded(1.1),
            P::Ah => Articulation::vowel(18.0, 2.15),
            P::Ax => Articulation::vowel(20.0, 3.2),
            P::Uh => Articulation::vowel(22.0, 2.3).rounded(0.95),
            P::Uw => Articulation::vowel(23.0, 2.05).rounded(0.8),
            P::Er => Articulation::vowel(19.0, 2.6)
                .rounded(1.1)
                .with_constriction(30.0, 0.75),
            P::Ay => Phoneme::Aa.articulation(),
            P::Aw => Phoneme::Ae.articulation(),
            P::Ey => Phoneme::Eh.articulation(),
            P::Ow => Phoneme::Ao.articulation(),
            P::Oy => Phoneme::Ao.articulation(),

            P::P => Articulation::consonant(LIPS, SHUT, false),
            P::B => Articulation::consonant(LIPS, SHUT, true),
            P::T => Articulation::consonant(ALVEOLAR, SHUT, false),
            P::D => Articulation::consonant(ALVEOLAR, SHUT, true),
            P::K => Articulation::consonant(VELAR, SHUT, false).with_tongue(VELAR, 2.1),
            P::G => Articulation::consonant(VELAR, SHUT, true).with_tongue(VELAR, 2.1),
            P::M => Articulation::consonant(LIPS, SHUT, true).nasal(),
            P::N => Articulation::consonant(ALVEOLAR, SHUT, true).nasal(),
            P::Ng => Articulation::consonant(VELAR, SHUT, true)
                .with_tongue(VELAR, 2.1)
                .nasal(),
            P::F => Articulation::consonant(TEETH + 1.5, HISS + 0.05, false),
            P::V => Articulation::consonant(TEETH + 1.5, HISS + 0.05, true),
            P::Th => Articulation::consonant(TEETH, HISS + 0.1, false),
            P::Dh => Articulation::consonant(TEETH, HISS + 0.1, true),
            P::S => Articulation::consonant(ALVEOLAR + 1.0, HISS, false),
            P::Z => Articulation::consonant(ALVEOLAR + 1.0, HISS, true),
            P::Sh => Articulation::consonant(POSTALVEOLAR, HISS + 0.05, false).rounded(1.0),
            P::Zh => Articulation::consonant(POSTALVEOLAR, HISS + 0.05, true).rounded(1.0),
            P::Ch => Articulation::consonant(POSTALVEOLAR + 1.0, SHUT, false).rounded(1.0),
            P::Jh => Articulation::consonant(POSTALVEOLAR + 1.0, SHUT, true).rounded(1.0),
            P::L => Articulation::consonant(ALVEOLAR - 1.0, 0.9, true).with_tongue(16.0, 2.6),
            P::R => Articulation::consonant(31.5, 0.85, true)
                .with_tongue(19.0, 2.6)
                .rounded(1.1),
            P::W => Articulation::vowel(21.0, 2.1).rounded(0.75),
            P::Y => Articulation::vowel(27.5, 2.05).with_constriction(29.0, 0.95),
            P::Hh => Articulation {
                tongue: None,
                constriction: None,
                lips: None,
                velum: 0.0,
                voiced: true,
                tenseness: Some(0.0),
            },
            P::Sil => Articulation {
                tongue: None,
                constriction: None,
                lips: None,
                velum: 0.0,
                voiced: false,
                tenseness: None,
            },
        }
    }
}

impl Articulation {
    const fn with_constriction(mut self, index: f32, diameter: f32) -> Self {
        self.constriction = Some(Constriction::new(index, diameter));
        self
    }
}

impl core::fmt::Display for Phoneme {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.symbol())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_round_trip() {
        for &p in Phoneme::ALL {
            assert_eq!(Phoneme::from_symbol(p.symbol()), Some(p));
            assert_eq!(
                Phoneme::from_symbol(&p.symbol().to_ascii_lowercase()),
                Some(p)
            );
        }
        assert_eq!(Phoneme::from_symbol("AA1"), Some(Phoneme::Aa));
        assert_eq!(Phoneme::from_symbol("XX"), None);
    }

    #[test]
    fn diphthongs_glide_between_vowels() {
        for &p in Phoneme::ALL {
            match p.manner() {
                Manner::Diphthong => {
                    let (a, b) = p.glide().unwrap();
                    assert_eq!(a.manner(), Manner::Vowel);
                    assert_eq!(b.manner(), Manner::Vowel);
                }
                _ => assert!(p.glide().is_none()),
            }
        }
    }

    #[test]
    fn stops_and_nasals_close_the_tract() {
        for &p in Phoneme::ALL {
            if matches!(p.manner(), Manner::Stop | Manner::Nasal | Manner::Affricate) {
                let c = p.articulation().constriction.unwrap();
                assert!(c.diameter <= 0.3, "{p}");
                assert_eq!(
                    p.articulation().velum > 0.0,
                    p.manner() == Manner::Nasal,
                    "{p}"
                );
            }
        }
    }
}
