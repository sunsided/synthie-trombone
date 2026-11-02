//! Text-to-speech front end.
//!
//! [`Utterance::from_text`] turns English text into a timed articulation
//! track; a [`Speaker`] plays it on a [`Trombone`] from the audio thread.
//!
//! Text goes through:
//!
//! 1. **Normalisation:** lower case, numbers spelled out, punctuation kept as
//!    phrase breaks.
//! 2. **Letter to sound:** a small exceptions dictionary for common and
//!    irregular words, then context-sensitive English spelling rules
//!    (digraphs, magic *e*, soft *c*/*g*, vowel reduction). Write ARPAbet in
//!    brackets to bypass the rules: `"[HH AX L OW1] world"`.
//! 3. **Timing:** per-manner durations, lengthened under stress and at phrase
//!    ends; stops become closure, burst and aspiration; diphthongs glide.
//! 4. **Coarticulation:** consonants take the tongue body of the next vowel.
//! 5. **Intonation:** declining pitch over each phrase, accents on stressed
//!    syllables, a fall at `.` and a rise at `?`.
//!
//! Compiling allocates; do it off the audio thread and hand the
//! [`Utterance`] to the [`Speaker`].

#![allow(clippy::match_same_arms, clippy::if_same_then_else)] // rule tables

use crate::Trombone;
use crate::math::lerp;
use crate::phoneme::{Articulation, Manner, Phoneme};
use alloc::string::String;
use alloc::vec::Vec;

/// Voice settings for compiling speech.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct Prosody {
    /// Baseline pitch in Hz.
    pub pitch: f32,
    /// Pitch excursion of accents and phrase ends, relative (`0.0` is a
    /// monotone robot, `1.0` is lively).
    pub expressiveness: f32,
    /// Speaking rate multiplier (`1.0` is about 4.5 syllables per second).
    pub rate: f32,
    /// Glottal tenseness while speaking.
    pub tenseness: f32,
    /// Velum opening during vowels and approximants (`0` oral, `1` very
    /// nasal).
    pub nasality: f32,
}

impl Default for Prosody {
    fn default() -> Self {
        Self {
            pitch: 110.0,
            expressiveness: 0.5,
            rate: 1.0,
            tenseness: 0.65,
            nasality: 0.0,
        }
    }
}

/// One timed target of an [`Utterance`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    /// The phoneme this segment realises.
    pub phoneme: Phoneme,
    /// Tract target (tongue always filled in).
    pub articulation: Articulation,
    /// Length in seconds.
    pub duration: f32,
    /// Pitch at the start and end, Hz.
    pub pitch: (f32, f32),
    /// Glottal tenseness.
    pub tenseness: f32,
    /// Whether the folds vibrate.
    pub voiced: bool,
}

/// A compiled, ready-to-play articulation track.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Utterance {
    segments: Vec<Segment>,
}

/// A phoneme with its lexical context, the output of [`phonemize`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Token {
    /// A sound; `stressed` marks the accented syllable of a content word.
    Phone {
        /// The sound.
        phoneme: Phoneme,
        /// Primary stress.
        stressed: bool,
    },
    /// Boundary between words.
    WordBreak,
    /// Phrase break from punctuation.
    Break(Break),
}

/// Kinds of phrase break.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Break {
    /// `,` `;` `:` — short pause, pitch held.
    Comma,
    /// `.` — pitch falls.
    Period,
    /// `?` — pitch rises.
    Question,
    /// `!` — emphatic fall.
    Exclamation,
}

impl Utterance {
    /// Compile English text.
    #[must_use]
    pub fn from_text(text: &str, prosody: &Prosody) -> Self {
        Self::from_tokens(&phonemize(text), prosody)
    }

    /// Compile a token stream (see [`phonemize`]).
    #[must_use]
    pub fn from_tokens(tokens: &[Token], prosody: &Prosody) -> Self {
        compile(tokens, prosody)
    }

    /// The segments, in order.
    #[must_use]
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// Total length in seconds.
    #[must_use]
    pub fn duration(&self) -> f32 {
        self.segments.iter().map(|s| s.duration).sum()
    }

    /// Whether there is nothing to say.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }
}

/// Plays [`Utterance`]s on a [`Trombone`]. Never allocates.
#[derive(Clone, Debug, Default)]
pub struct Speaker {
    utterance: Utterance,
    index: usize,
    elapsed: f32,
    started: bool,
}

impl Speaker {
    /// An idle speaker.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Start speaking `utterance`, interrupting anything in progress. Returns
    /// the previous utterance so the caller can drop it off the audio thread.
    pub fn speak(&mut self, utterance: Utterance) -> Utterance {
        self.index = 0;
        self.elapsed = 0.0;
        self.started = false;
        core::mem::replace(&mut self.utterance, utterance)
    }

    /// Stop speaking (the voice fades out). Returns the utterance.
    pub fn stop(&mut self, voice: &mut Trombone) -> Utterance {
        voice.set_voiced(false);
        voice.articulate_phoneme(Phoneme::Sil);
        self.index = usize::MAX;
        core::mem::take(&mut self.utterance)
    }

    /// Whether an utterance is playing.
    #[must_use]
    pub fn is_speaking(&self) -> bool {
        self.index < self.utterance.segments.len()
    }

    /// Index and segment being spoken.
    #[must_use]
    pub fn current(&self) -> Option<(usize, &Segment)> {
        self.utterance
            .segments
            .get(self.index)
            .map(|s| (self.index, s))
    }

    /// The utterance being spoken (or last spoken).
    #[must_use]
    pub fn utterance(&self) -> &Utterance {
        &self.utterance
    }

    /// Configure `voice` for speech: fast voicing, nimble articulators,
    /// quick pitch tracking, no wobble. Call once before speaking.
    pub fn prepare(voice: &mut Trombone) {
        voice.set_voicing_rates(40.0, 40.0);
        voice.set_articulation_speed(2.2);
        voice.set_glide(f32::INFINITY);
        voice.set_wobble(false);
        voice.set_always_voice(false);
    }

    /// Advance one output sample and render it.
    pub fn process(&mut self, voice: &mut Trombone) -> f32 {
        if let Some(seg) = self.utterance.segments.get(self.index) {
            if !self.started {
                self.started = true;
                voice.articulate(&seg.articulation);
                voice.set_voiced(seg.voiced);
                voice.set_tenseness(seg.tenseness);
            }
            let t = (self.elapsed / seg.duration).min(1.0);
            voice.set_frequency(lerp(seg.pitch.0, seg.pitch.1, t));
            self.elapsed += 1.0 / voice.sample_rate();
            if self.elapsed >= seg.duration {
                self.elapsed -= seg.duration;
                self.index += 1;
                self.started = false;
                if self.index == self.utterance.segments.len() {
                    voice.set_voiced(false);
                    voice.articulate_phoneme(Phoneme::Sil);
                }
            }
        }
        voice.process()
    }

    /// Render a block.
    pub fn render(&mut self, voice: &mut Trombone, out: &mut [f32]) {
        for s in out {
            *s = self.process(voice);
        }
    }
}

// ===== text → tokens ======================================================

/// Convert English text (with optional `[ARPAbet]` spans) to tokens.
#[must_use]
pub fn phonemize(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut chars = text.chars().peekable();

    let flush = |word: &mut String, out: &mut Vec<Token>| {
        if !word.is_empty() {
            push_word(word, out);
            word.clear();
        }
    };

    while let Some(c) = chars.next() {
        match c {
            '[' => {
                flush(&mut word, &mut out);
                let mut span = String::new();
                for c in chars.by_ref() {
                    if c == ']' {
                        break;
                    }
                    span.push(c);
                }
                push_arpabet(&span, &mut out);
            }
            c if c.is_alphabetic() || c == '\'' => {
                word.extend(c.to_lowercase());
            }
            c if c.is_ascii_digit() => {
                flush(&mut word, &mut out);
                let mut n = String::from(c);
                while let Some(&d) = chars.peek() {
                    if d.is_ascii_digit() {
                        n.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
                for w in number_words(&n).split(' ').filter(|w| !w.is_empty()) {
                    push_word(w, &mut out);
                }
            }
            _ => {
                flush(&mut word, &mut out);
                let brk = match c {
                    ',' | ';' | ':' | '—' | '–' => Some(Break::Comma),
                    '.' | '\n' => Some(Break::Period),
                    '?' => Some(Break::Question),
                    '!' => Some(Break::Exclamation),
                    _ => None,
                };
                if let Some(b) = brk {
                    // Collapse runs like "?!" or "..." into the first.
                    if !matches!(out.last(), Some(Token::Break(_))) {
                        if matches!(out.last(), Some(Token::WordBreak)) {
                            out.pop();
                        }
                        out.push(Token::Break(b));
                    }
                }
            }
        }
    }
    flush(&mut word, &mut out);
    if matches!(out.last(), Some(Token::WordBreak)) {
        out.pop();
    }
    out
}

fn push_arpabet(span: &str, out: &mut Vec<Token>) {
    let mut any = false;
    let mut stressed_seen = false;
    for sym in span.split_whitespace() {
        if let Some(p) = Phoneme::from_symbol(sym) {
            let stressed = sym.ends_with('1');
            stressed_seen |= stressed;
            out.push(Token::Phone {
                phoneme: p,
                stressed,
            });
            any = true;
        }
    }
    if any {
        if !stressed_seen {
            stress_first_vowel(out);
        }
        out.push(Token::WordBreak);
    }
}

fn push_word(word: &str, out: &mut Vec<Token>) {
    let word = word.trim_matches('\'');
    if word.is_empty() {
        return;
    }
    let start = out.len();
    let (phones, function_word) = match lookup(word) {
        Some(p) => (parse_arpabet(p), FUNCTION_WORDS.contains(&word)),
        None => (letter_to_sound(word), false),
    };
    for p in phones {
        out.push(Token::Phone {
            phoneme: p,
            stressed: false,
        });
    }
    if out.len() > start && !function_word {
        stress_first_vowel(&mut out[start..]);
    }
    out.push(Token::WordBreak);
}

fn stress_first_vowel(tokens: &mut [Token]) {
    // Within the current word: walk back to the last break.
    let start = tokens
        .iter()
        .rposition(|t| !matches!(t, Token::Phone { .. }))
        .map_or(0, |i| i + 1);
    for t in &mut tokens[start..] {
        if let Token::Phone { phoneme, stressed } = t
            && is_vowel(*phoneme)
        {
            *stressed = true;
            return;
        }
    }
}

fn parse_arpabet(s: &str) -> Vec<Phoneme> {
    s.split_whitespace()
        .filter_map(Phoneme::from_symbol)
        .collect()
}

fn is_vowel(p: Phoneme) -> bool {
    matches!(p.manner(), Manner::Vowel | Manner::Diphthong)
}

/// Words that do not take an accent.
const FUNCTION_WORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "of", "to", "in", "on", "at", "for", "from", "with",
    "by", "as", "is", "are", "was", "were", "be", "it", "its", "it's", "i", "you", "he", "she",
    "we", "they", "me", "him", "her", "us", "them", "my", "your", "his", "our", "their", "that",
    "this", "than", "then", "so", "if", "do", "does", "has", "have", "had", "can", "will",
];

/// Irregular and very common words.
#[allow(clippy::too_many_lines)] // a dictionary
fn lookup(word: &str) -> Option<&'static str> {
    Some(match word {
        "a" => "AX",
        "an" => "AX N",
        "the" => "DH AX",
        "and" => "AE N D",
        "or" => "AO R",
        "of" => "AH V",
        "to" => "T UW",
        "too" | "two" => "T UW",
        "in" => "IH N",
        "is" => "IH Z",
        "it" => "IH T",
        "its" | "it's" => "IH T S",
        "as" => "AE Z",
        "was" => "W AA Z",
        "were" => "W ER",
        "are" => "AA R",
        "be" => "B IY",
        "been" => "B IH N",
        "i" => "AY",
        "i'm" => "AY M",
        "you" => "Y UW",
        "your" | "you're" => "Y AO R",
        "he" => "HH IY",
        "she" => "SH IY",
        "we" => "W IY",
        "me" => "M IY",
        "my" => "M AY",
        "by" => "B AY",
        "they" => "DH EY",
        "them" => "DH EH M",
        "their" | "there" | "they're" => "DH EH R",
        "this" => "DH IH S",
        "that" => "DH AE T",
        "these" => "DH IY Z",
        "those" => "DH OW Z",
        "then" => "DH EH N",
        "than" => "DH AE N",
        "though" => "DH OW",
        "with" => "W IH DH",
        "what" => "W AH T",
        "who" => "HH UW",
        "where" => "W EH R",
        "why" => "W AY",
        "how" => "HH AW",
        "do" => "D UW",
        "does" => "D AH Z",
        "done" => "D AH N",
        "don't" => "D OW N T",
        "have" => "HH AE V",
        "has" => "HH AE Z",
        "his" => "HH IH Z",
        "her" => "HH ER",
        "give" => "G IH V",
        "live" => "L IH V",
        "love" => "L AH V",
        "come" => "K AH M",
        "some" => "S AH M",
        "one" => "W AH N",
        "once" => "W AH N S",
        "said" => "S EH D",
        "says" => "S EH Z",
        "here" => "HH IY R",
        "know" => "N OW",
        "through" => "TH R UW",
        "thought" => "TH AO T",
        "enough" => "IH N AH F",
        "people" => "P IY P AX L",
        "good" => "G UH D",
        "could" => "K UH D",
        "would" => "W UH D",
        "should" => "SH UH D",
        "no" => "N OW",
        "so" => "S OW",
        "go" => "G OW",
        "oh" => "OW",
        "yes" => "Y EH S",
        "eye" => "AY",
        "only" => "OW N L IY",
        "any" => "EH N IY",
        "many" => "M EH N IY",
        "very" => "V EH R IY",
        "every" => "EH V R IY",
        "again" => "AX G EH N",
        "about" => "AX B AW T",
        "hello" => "HH AX L OW",
        "world" => "W ER L D",
        "computer" => "K AX M P Y UW T ER",
        "machine" => "M AX SH IY N",
        "music" => "M Y UW Z IH K",
        "voice" => "V OY S",
        "pink" => "P IH NG K",
        "trombone" => "T R AA M B OW N",
        "synthie" => "S IH N TH IY",
        "daisy" => "D EY Z IY",
        "bicycle" => "B AY S IH K AX L",
        "zero" => "Z IH R OW",
        "three" => "TH R IY",
        "four" => "F AO R",
        "five" => "F AY V",
        "six" => "S IH K S",
        "seven" => "S EH V AX N",
        "eight" => "EY T",
        "nine" => "N AY N",
        "ten" => "T EH N",
        "eleven" => "IH L EH V AX N",
        "twelve" => "T W EH L V",
        "thirteen" => "TH ER T IY N",
        "fifteen" => "F IH F T IY N",
        "twenty" => "T W EH N T IY",
        "thirty" => "TH ER T IY",
        "forty" => "F AO R T IY",
        "fifty" => "F IH F T IY",
        "hundred" => "HH AH N D R AX D",
        "thousand" => "TH AW Z AX N D",
        "million" => "M IH L Y AX N",
        _ => return None,
    })
}

fn number_words(digits: &str) -> String {
    const ONES: [&str; 20] = [
        "zero",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
    ];
    const TENS: [&str; 10] = [
        "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
    ];

    fn below_thousand(n: usize, out: &mut String) {
        if n >= 100 {
            out.push_str(ONES[n / 100]);
            out.push_str(" hundred ");
        }
        let r = n % 100;
        if r >= 20 {
            out.push_str(TENS[r / 10]);
            out.push(' ');
            if !r.is_multiple_of(10) {
                out.push_str(ONES[r % 10]);
                out.push(' ');
            }
        } else if r > 0 || n == 0 {
            out.push_str(ONES[r]);
            out.push(' ');
        }
    }

    let mut out = String::new();
    match digits.parse::<usize>() {
        Ok(n) if digits.len() <= 9 && !(digits.len() > 1 && digits.starts_with('0')) => {
            let (m, rest) = (n / 1_000_000, n % 1_000_000);
            let (k, r) = (rest / 1000, rest % 1000);
            if m > 0 {
                below_thousand(m, &mut out);
                out.push_str("million ");
            }
            if k > 0 {
                below_thousand(k, &mut out);
                out.push_str("thousand ");
            }
            if r > 0 || n == 0 {
                below_thousand(r, &mut out);
            }
        }
        _ => {
            // Too long, or a code like "007": read digit by digit.
            for d in digits.bytes() {
                out.push_str(ONES[usize::from(d - b'0')]);
                out.push(' ');
            }
        }
    }
    out
}

/// English spelling rules for words not in the dictionary.
#[allow(clippy::too_many_lines)]
fn letter_to_sound(word: &str) -> Vec<Phoneme> {
    use Phoneme as P;

    let w: Vec<u8> = word.bytes().filter(u8::is_ascii_lowercase).collect();
    let n = w.len();
    let mut out = Vec::new();
    let at = |i: usize| -> u8 { w.get(i).copied().unwrap_or(0) };
    let is_v = |c: u8| matches!(c, b'a' | b'e' | b'i' | b'o' | b'u' | b'y');
    let is_c = |c: u8| c.is_ascii_lowercase() && !is_v(c);
    let starts = |i: usize, s: &str| w[i..].starts_with(s.as_bytes());
    // Magic e: vowel, one consonant, final e (or -es/-ed).
    let magic_e = |i: usize| -> bool {
        is_c(at(i + 1))
            && at(i + 2) == b'e'
            && (i + 3 == n || (i + 4 == n && matches!(at(i + 3), b's' | b'd')))
            && !matches!(at(i + 1), b'r' | b'w' | b'x')
    };
    let mut vowels_seen = 0;

    let mut i = 0;
    while i < n {
        let c = w[i];
        let next = at(i + 1);
        let first_syllable = vowels_seen == 0;
        // (phonemes, letters consumed)
        let (ph, len): (&[Phoneme], usize) = match c {
            // ---- multi-letter vowel spellings ----
            b'e' if starts(i, "eigh") => (&[P::Ey], 4),
            b'i' if starts(i, "igh") => (&[P::Ay], 3),
            b'o' if starts(i, "ough") => (&[P::Ao], 4),
            b'a' if starts(i, "augh") => (&[P::Ao], 4),
            b'a' if starts(i, "all") => (&[P::Ao, P::L], 3),
            b'e' if starts(i, "ee") => (&[P::Iy], 2),
            b'e' if starts(i, "ea") => (&[P::Iy], 2),
            b'e' if starts(i, "ei") => (&[P::Iy], 2),
            b'e' if starts(i, "ey") => (if i + 2 == n { &[P::Iy] } else { &[P::Ey] }, 2),
            b'e' if starts(i, "ew") => (&[P::Uw], 2),
            b'o' if starts(i, "oo") => (&[P::Uw], 2),
            b'o' if starts(i, "ou") => (&[P::Aw], 2),
            b'o' if starts(i, "ow") => (if i + 2 == n { &[P::Ow] } else { &[P::Aw] }, 2),
            b'o' if starts(i, "oa") => (&[P::Ow], 2),
            b'o' if starts(i, "oi") || starts(i, "oy") => (&[P::Oy], 2),
            b'a' if starts(i, "ai") || starts(i, "ay") => (&[P::Ey], 2),
            b'a' if starts(i, "au") || starts(i, "aw") => (&[P::Ao], 2),
            b'u' if starts(i, "ue") => (&[P::Uw], 2),
            b'i' if starts(i, "ie") => (if n <= 3 { &[P::Ay] } else { &[P::Iy] }, 2),
            // ---- r-coloured vowels ----
            b'e' | b'i' | b'u' if next == b'r' && !is_v(at(i + 2)) => (&[P::Er], 2),
            b'a' if next == b'r' && !is_v(at(i + 2)) => (
                if first_syllable {
                    &[P::Aa, P::R]
                } else {
                    &[P::Er]
                },
                2,
            ),
            b'o' if next == b'r' && !is_v(at(i + 2)) => (
                if first_syllable {
                    &[P::Ao, P::R]
                } else {
                    &[P::Er]
                },
                2,
            ),
            // ---- single vowels ----
            b'a' | b'e' | b'i' | b'o' | b'u' if magic_e(i) => (
                match c {
                    b'a' => &[P::Ey],
                    b'e' => &[P::Iy],
                    b'i' => &[P::Ay],
                    b'o' => &[P::Ow],
                    _ => &[P::Uw],
                },
                1,
            ),
            b'e' if i + 1 == n && n > 2 => (&[], 1),
            b'e' if i + 2 == n
                && next == b's'
                && n > 3
                && !matches!(at(i - 1), b's' | b'z' | b'x' | b'h') =>
            {
                (&[P::Z], 2)
            }
            b'e' if i + 2 == n && next == b'd' && n > 3 => (
                if matches!(at(i - 1), b't' | b'd') {
                    &[P::Ih, P::D]
                } else {
                    &[P::D]
                },
                2,
            ),
            b'a' => (if first_syllable { &[P::Ae] } else { &[P::Ax] }, 1),
            b'e' => (
                if first_syllable || i + 1 == n {
                    &[P::Eh]
                } else {
                    &[P::Ax]
                },
                1,
            ),
            b'i' => (&[P::Ih], 1),
            b'o' => (
                if i + 1 == n {
                    &[P::Ow]
                } else if first_syllable {
                    &[P::Aa]
                } else {
                    &[P::Ax]
                },
                1,
            ),
            b'u' => (if first_syllable { &[P::Ah] } else { &[P::Ax] }, 1),
            b'y' if i == 0 => (&[P::Y], 1),
            b'y' if i + 1 == n => (if n <= 3 { &[P::Ay] } else { &[P::Iy] }, 1),
            b'y' => (&[P::Ih], 1),
            // ---- consonant clusters ----
            b't' if starts(i, "tion") => (&[P::Sh, P::Ax, P::N], 4),
            b's' if starts(i, "sion") => (
                if is_v(at(i.wrapping_sub(1))) {
                    &[P::Zh, P::Ax, P::N]
                } else {
                    &[P::Sh, P::Ax, P::N]
                },
                4,
            ),
            b't' if starts(i, "ture") => (&[P::Ch, P::Er], 4),
            b't' if starts(i, "tch") => (&[P::Ch], 3),
            b't' if next == b'h' => (if i == 0 && n <= 4 { &[P::Dh] } else { &[P::Th] }, 2),
            b'c' if next == b'h' => (&[P::Ch], 2),
            b's' if next == b'h' => (&[P::Sh], 2),
            b'p' if next == b'h' => (&[P::F], 2),
            b'w' if next == b'h' => (&[P::W], 2),
            b'g' if next == b'h' => (if i == 0 { &[P::G] } else { &[] }, 2),
            b'c' if next == b'k' => (&[P::K], 2),
            b'n' if next == b'g' && !is_v(at(i + 2)) => (&[P::Ng], 2),
            b'n' if next == b'k' => (&[P::Ng, P::K], 2),
            b'q' if next == b'u' => (&[P::K, P::W], 2),
            b'k' if next == b'n' && i == 0 => (&[P::N], 2),
            b'w' if next == b'r' && i == 0 => (&[P::R], 2),
            b'm' if next == b'b' && i + 2 == n => (&[P::M], 2),
            b'l' if next == b'e' && i + 2 == n && n > 3 && is_c(at(i - 1)) => (&[P::Ax, P::L], 2),
            // ---- single consonants ----
            b'c' => (
                if matches!(next, b'e' | b'i' | b'y') {
                    &[P::S]
                } else {
                    &[P::K]
                },
                1,
            ),
            b'g' => (
                if matches!(next, b'y')
                    || (next == b'e' && i + 2 == n)
                    || starts(i, "ge") && i > 0 && magic_e(i - 1)
                {
                    &[P::Jh]
                } else {
                    &[P::G]
                },
                1,
            ),
            b'h' => (if is_v(next) { &[P::Hh] } else { &[] }, 1),
            b's' => (
                if i > 0 && i + 1 == n && is_voiced_letter(at(i - 1)) && at(i - 1) != b's' {
                    &[P::Z]
                } else if i > 0 && is_v(at(i - 1)) && is_v(next) && next != b'y' {
                    &[P::Z]
                } else {
                    &[P::S]
                },
                1,
            ),
            b'x' => (if i == 0 { &[P::Z] } else { &[P::K, P::S] }, 1),
            b'b' => (&[P::B], 1),
            b'd' => (&[P::D], 1),
            b'f' => (&[P::F], 1),
            b'j' => (&[P::Jh], 1),
            b'k' | b'q' => (&[P::K], 1),
            b'l' => (&[P::L], 1),
            b'm' => (&[P::M], 1),
            b'n' => (&[P::N], 1),
            b'p' => (&[P::P], 1),
            b'r' => (&[P::R], 1),
            b't' => (&[P::T], 1),
            b'v' => (&[P::V], 1),
            b'w' => (&[P::W], 1),
            b'z' => (&[P::Z], 1),
            _ => (&[], 1),
        };
        for &p in ph {
            // Double letters make one sound.
            if out.last() == Some(&p) && !is_vowel(p) {
                continue;
            }
            out.push(p);
        }
        if ph.iter().any(|&p| is_vowel(p) || p == P::Er) {
            vowels_seen += 1;
        }
        i += len;
    }
    out
}

fn is_voiced_letter(c: u8) -> bool {
    matches!(
        c,
        b'a' | b'e'
            | b'i'
            | b'o'
            | b'u'
            | b'y'
            | b'b'
            | b'd'
            | b'g'
            | b'l'
            | b'm'
            | b'n'
            | b'r'
            | b'v'
            | b'w'
    )
}

// ===== tokens → timed segments ============================================

/// Base durations in seconds at rate 1.
fn base_duration(p: Phoneme, stressed: bool) -> f32 {
    let d = match p.manner() {
        Manner::Vowel => 0.085,
        Manner::Diphthong => 0.15,
        Manner::Stop | Manner::Affricate => 0.06,
        Manner::Nasal => 0.065,
        Manner::Fricative => {
            if p.is_voiced() {
                0.07
            } else {
                0.1
            }
        }
        Manner::Approximant => 0.055,
        Manner::Aspirate => 0.06,
        Manner::Pause => 0.1,
    };
    if stressed && is_vowel(p) { d * 1.5 } else { d }
}

#[derive(Clone, Copy)]
struct Draft {
    phoneme: Phoneme,
    articulation: Articulation,
    duration: f32,
    voiced: bool,
    tenseness: Option<f32>,
    /// Relative pitch target at the end of the segment (1.0 = baseline).
    accent: f32,
    vowel: bool,
}

#[allow(clippy::too_many_lines)]
fn compile(tokens: &[Token], prosody: &Prosody) -> Utterance {
    let rate = prosody.rate.clamp(0.2, 5.0);
    let expr = prosody.expressiveness.clamp(0.0, 2.0);
    let tense = prosody.tenseness.clamp(0.0, 1.0);
    let nasality = prosody.nasality.clamp(0.0, 1.0);

    // Split into phrases at breaks; each phrase gets its own contour.
    let mut drafts: Vec<Draft> = Vec::new();
    let mut phrase_start = 0;
    let mut i = 0;

    let push_phrase_end = |drafts: &mut Vec<Draft>, start: usize, brk: Option<Break>| {
        shape_phrase(&mut drafts[start..], brk, expr);
    };

    while i < tokens.len() {
        match tokens[i] {
            Token::Phone { phoneme, stressed } => {
                let next_vowel_or_approx = tokens[i + 1..]
                    .iter()
                    .find_map(|t| match t {
                        Token::Phone { phoneme, .. } => Some(*phoneme),
                        _ => None,
                    })
                    .is_some_and(|p| is_vowel(p) || p.manner() == Manner::Approximant);
                expand(phoneme, stressed, next_vowel_or_approx, rate, &mut drafts);
            }
            Token::WordBreak => {}
            Token::Break(b) => {
                push_phrase_end(&mut drafts, phrase_start, Some(b));
                let pause = match b {
                    Break::Comma => 0.18,
                    _ => 0.38,
                } / rate;
                drafts.push(silence(pause));
                phrase_start = drafts.len();
            }
        }
        i += 1;
    }
    push_phrase_end(&mut drafts, phrase_start, Some(Break::Period));

    // Lengthen the last syllable of each phrase.
    for k in 1..drafts.len() {
        if drafts[k].phoneme == Phoneme::Sil
            && let Some(v) = drafts[..k].iter().rposition(|d| d.vowel)
        {
            drafts[v].duration *= 1.4;
        }
    }

    // Coarticulation: consonants borrow the tongue of the next vowel (or
    // the previous one at the end of an utterance).
    let mut next_tongue = None;
    for d in drafts.iter_mut().rev() {
        if d.vowel {
            next_tongue = d.articulation.tongue;
        } else if d.articulation.tongue.is_none() {
            d.articulation.tongue = next_tongue;
        }
    }
    let mut prev_tongue = Phoneme::Ax.articulation().tongue;
    for d in &mut drafts {
        match d.articulation.tongue {
            Some(t) => prev_tongue = Some(t),
            None => d.articulation.tongue = prev_tongue,
        }
    }

    // Pitch: start each segment where the last ended.
    let mut segments = Vec::with_capacity(drafts.len() + 1);
    let mut last = prosody.pitch;
    for mut d in drafts {
        if d.vowel || d.phoneme.manner() == Manner::Approximant {
            d.articulation.velum = d.articulation.velum.max(nasality);
        }
        let target = prosody.pitch * d.accent;
        segments.push(Segment {
            phoneme: d.phoneme,
            articulation: d.articulation,
            duration: d.duration,
            pitch: (last, target),
            tenseness: d.tenseness.unwrap_or(tense),
            voiced: d.voiced,
        });
        last = target;
    }
    // Trim a trailing pause to a short release.
    if let Some(s) = segments.last_mut()
        && s.phoneme == Phoneme::Sil
    {
        s.duration = 0.05;
    }
    Utterance { segments }
}

fn silence(duration: f32) -> Draft {
    Draft {
        phoneme: Phoneme::Sil,
        articulation: Phoneme::Sil.articulation(),
        duration,
        voiced: false,
        tenseness: None,
        accent: 1.0,
        vowel: false,
    }
}

fn expand(p: Phoneme, stressed: bool, before_vowel: bool, rate: f32, out: &mut Vec<Draft>) {
    let a = p.articulation();
    let dur = base_duration(p, stressed) / rate;
    let accent = if stressed { 1.0 } else { 0.0 };
    let draft = |phoneme: Phoneme, articulation: Articulation, duration: f32, voiced: bool| Draft {
        phoneme,
        articulation,
        duration,
        voiced,
        tenseness: articulation.tenseness,
        // Marks stress for `shape_phrase`; replaced by a pitch ratio there.
        accent,
        vowel: is_vowel(phoneme),
    };

    match p.manner() {
        Manner::Diphthong => {
            let (from, to) = p.glide().expect("diphthong");
            out.push(draft(p, from.articulation(), dur * 0.55, true));
            let mut tail = draft(p, to.articulation(), dur * 0.45, true);
            tail.accent = 0.0;
            out.push(tail);
        }
        Manner::Stop => {
            out.push(draft(p, a, dur, a.voiced));
            if !a.voiced && before_vowel {
                // Aspiration after the burst: open tract, breathy folds.
                let mut asp = Phoneme::Hh.articulation();
                asp.tongue = a.tongue;
                out.push(draft(p, asp, 0.035 / rate, true));
            }
        }
        Manner::Affricate => {
            out.push(draft(p, a, dur * 0.6, a.voiced));
            let fric = if a.voiced { Phoneme::Zh } else { Phoneme::Sh };
            out.push(draft(p, fric.articulation(), dur, a.voiced));
        }
        _ => out.push(draft(p, a, dur, a.voiced)),
    }
}

/// Intonation for one phrase: gentle declination, accents on stressed
/// vowels, and a boundary tone.
#[allow(clippy::cast_precision_loss)]
fn shape_phrase(drafts: &mut [Draft], brk: Option<Break>, expr: f32) {
    let total: f32 = drafts.iter().map(|d| d.duration).sum();
    if total <= 0.0 {
        return;
    }
    let last_vowel = drafts.iter().rposition(|d| d.vowel);
    let mut t = 0.0;
    for (k, d) in drafts.iter_mut().enumerate() {
        t += d.duration;
        let pos = t / total;
        let declination = 1.0 + expr * (0.12 - 0.22 * pos);
        let accent = if d.accent > 0.0 && d.vowel {
            1.0 + 0.18 * expr
        } else {
            1.0
        };
        let boundary = if Some(k) == last_vowel {
            match brk {
                Some(Break::Question) => 1.0 + 0.6 * expr,
                Some(Break::Period) => 1.0 - 0.18 * expr,
                Some(Break::Exclamation) => 1.0 - 0.25 * expr,
                Some(Break::Comma) => 1.0 + 0.08 * expr,
                None => 1.0,
            }
        } else {
            1.0
        };
        d.accent = declination * accent * boundary;
    }
    // After a question rise, keep the remaining consonants up.
    if let (Some(v), Some(Break::Question)) = (last_vowel, brk) {
        let top = drafts[v].accent;
        for d in &mut drafts[v + 1..] {
            d.accent = top;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Phoneme as P;
    use alloc::vec;

    fn phones(text: &str) -> Vec<Phoneme> {
        phonemize(text)
            .into_iter()
            .filter_map(|t| match t {
                Token::Phone { phoneme, .. } => Some(phoneme),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn dictionary_words() {
        assert_eq!(
            phones("hello world"),
            vec![P::Hh, P::Ax, P::L, P::Ow, P::W, P::Er, P::L, P::D]
        );
    }

    #[test]
    fn spelling_rules() {
        assert_eq!(phones("cat"), vec![P::K, P::Ae, P::T]);
        assert_eq!(phones("make"), vec![P::M, P::Ey, P::K]);
        assert_eq!(phones("time"), vec![P::T, P::Ay, P::M]);
        assert_eq!(phones("ship"), vec![P::Sh, P::Ih, P::P]);
        assert_eq!(phones("night"), vec![P::N, P::Ay, P::T]);
        assert_eq!(phones("city"), vec![P::S, P::Ih, P::T, P::Iy]);
        assert_eq!(phones("sing"), vec![P::S, P::Ih, P::Ng]);
        assert_eq!(phones("nation"), vec![P::N, P::Ae, P::Sh, P::Ax, P::N]);
        assert_eq!(phones("dogs"), vec![P::D, P::Aa, P::G, P::Z]);
        assert_eq!(phones("bell"), vec![P::B, P::Eh, P::L]);
        assert_eq!(phones("knight"), vec![P::N, P::Ay, P::T]);
        assert_eq!(phones("queen"), vec![P::K, P::W, P::Iy, P::N]);
        assert_eq!(phones("boat"), vec![P::B, P::Ow, P::T]);
    }

    #[test]
    fn arpabet_spans_bypass_rules() {
        assert_eq!(phones("[K AE1 T]"), vec![P::K, P::Ae, P::T]);
        assert_eq!(phones("[nonsense]"), vec![]);
    }

    #[test]
    fn numbers_are_spelled_out() {
        assert_eq!(number_words("0").trim(), "zero");
        assert_eq!(number_words("42").trim(), "forty two");
        assert_eq!(
            number_words("1984").trim(),
            "one thousand nine hundred eighty four"
        );
        assert_eq!(number_words("007").trim(), "zero zero seven");
        assert_ne!(phones("route 66"), vec![]);
    }

    #[test]
    fn punctuation_makes_breaks() {
        let t = phonemize("hi, you?! ok.");
        let breaks: Vec<_> = t
            .iter()
            .filter_map(|t| match t {
                Token::Break(b) => Some(*b),
                _ => None,
            })
            .collect();
        assert_eq!(breaks, vec![Break::Comma, Break::Question, Break::Period]);
    }

    #[test]
    fn content_words_are_stressed() {
        let stressed: Vec<_> = phonemize("the cat")
            .into_iter()
            .filter_map(|t| match t {
                Token::Phone {
                    phoneme,
                    stressed: true,
                } => Some(phoneme),
                _ => None,
            })
            .collect();
        assert_eq!(stressed, vec![P::Ae]);
    }

    #[test]
    fn questions_rise_statements_fall() {
        let p = Prosody::default();
        let q = Utterance::from_text("you are here?", &p);
        let s = Utterance::from_text("you are here.", &p);
        let last_vowel_pitch = |u: &Utterance| {
            u.segments()
                .iter()
                .rev()
                .find(|s| is_vowel(s.phoneme))
                .unwrap()
                .pitch
                .1
        };
        assert!(last_vowel_pitch(&q) > p.pitch * 1.1);
        assert!(last_vowel_pitch(&s) < p.pitch);
    }

    #[test]
    fn consonants_borrow_the_next_vowel_tongue() {
        let u = Utterance::from_text("[S IY]", &Prosody::default());
        let s = u.segments()[0];
        assert_eq!(s.phoneme, P::S);
        assert_eq!(s.articulation.tongue, P::Iy.articulation().tongue);
    }

    #[test]
    fn faster_rate_is_shorter() {
        let slow = Utterance::from_text(
            "pink trombone",
            &Prosody {
                rate: 0.5,
                ..Prosody::default()
            },
        );
        let fast = Utterance::from_text(
            "pink trombone",
            &Prosody {
                rate: 2.0,
                ..Prosody::default()
            },
        );
        assert!(slow.duration() > 3.0 * fast.duration());
    }

    #[test]
    fn speaker_plays_and_finishes() {
        let mut voice = Trombone::new(16_000.0);
        Speaker::prepare(&mut voice);
        let mut sp = Speaker::new();
        let u = Utterance::from_text("Hello, pink trombone!", &Prosody::default());
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let n = ((u.duration() + 0.3) * 16_000.0) as usize;
        sp.speak(u);
        assert!(sp.is_speaking());
        let mut buf = vec![0.0; n];
        sp.render(&mut voice, &mut buf);
        assert!(!sp.is_speaking());
        assert!(buf.iter().all(|s| s.is_finite()));
        let energy: f32 = buf.iter().map(|s| s * s).sum();
        assert!(energy > 1.0, "{energy}");
    }

    #[test]
    fn never_panics_on_odd_text() {
        for t in [
            "",
            "   ",
            "!!!",
            "[",
            "]]",
            "ÄÖÜ ß",
            "a",
            "e",
            "y",
            "x",
            "123456789012",
            "'''",
            "—",
        ] {
            let u = Utterance::from_text(t, &Prosody::default());
            assert!(
                u.segments()
                    .iter()
                    .all(|s| s.duration > 0.0 && s.duration.is_finite())
            );
        }
    }
}
