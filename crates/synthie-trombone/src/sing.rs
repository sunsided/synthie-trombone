//! Singing: melody notes with syllables, compiled into a timed articulation
//! track that a [`Speaker`](crate::speech::Speaker) plays.
//!
//! Notes usually come from a MIDI file ([`melody`]), with lyrics from the
//! file's lyric events or from a separate syllable list
//! ([`with_syllables`]). [`compile`] then times every syllable like a
//! singer would: onset consonants start before the beat so the vowel lands on
//! it, the vowel holds the note's pitch, coda consonants close the note, and
//! notes without a syllable carry the previous vowel to a new pitch.

use crate::midi::Track;
use crate::phoneme::{Manner, Phoneme};
use crate::preset::VoiceSettings;
use crate::speech::{Segment, Token, Utterance, phonemize};
use alloc::string::String;
use alloc::vec::Vec;

/// One melody note, optionally with the syllable sung on it.
#[derive(Clone, Debug, PartialEq)]
pub struct SungNote {
    /// Start in seconds.
    pub start: f64,
    /// Length in seconds.
    pub duration: f64,
    /// Pitch as a (fractional) MIDI note.
    pub key: f32,
    /// Syllable text as found in the lyrics (karaoke conventions: a trailing
    /// `-` continues the word, spaces and `/` `\` separate words). `None`, or
    /// a lone `-`, `_`, `+` or `~`, continues the previous vowel.
    pub syllable: Option<String>,
}

/// How to sing a score.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SingOptions {
    /// Semitones added to every note.
    pub transpose: f32,
    /// Tempo factor: `2.0` sings twice as fast.
    pub tempo: f32,
    /// Seconds of pitch slide into each new note; `0` jumps (the hard,
    /// auto-tuned style).
    pub portamento: f32,
    /// Silence before the first sound, in seconds.
    pub lead_in: f32,
}

impl Default for SingOptions {
    fn default() -> Self {
        Self {
            transpose: 0.0,
            tempo: 1.0,
            portamento: 0.0,
            lead_in: 0.25,
        }
    }
}

/// The melody of a MIDI track: one note at a time (the highest when notes
/// overlap), each with the lyric that starts with it.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn melody(track: &Track) -> Vec<SungNote> {
    let mut notes: Vec<SungNote> = Vec::new();
    for n in &track.notes {
        if let Some(last) = notes.last_mut() {
            if (n.start - last.start).abs() < 1e-4 {
                // Same onset: keep the higher note (the melody in a chord).
                continue;
            }
            // Monophonic: a new note ends the previous one.
            last.duration = last.duration.min(n.start - last.start);
        }
        notes.push(SungNote {
            start: n.start,
            duration: n.duration,
            key: f32::from(n.key),
            syllable: None,
        });
    }

    // Attach each lyric to the note it belongs to: the one starting closest
    // to it (within 60 ms), else the note sounding at that time.
    for lyric in track.sung_text() {
        let nearest = notes
            .iter()
            .enumerate()
            .min_by(|a, b| {
                (a.1.start - lyric.time)
                    .abs()
                    .total_cmp(&(b.1.start - lyric.time).abs())
            })
            .map(|(i, _)| i);
        let Some(mut i) = nearest else { break };
        if (notes[i].start - lyric.time).abs() > 0.06 {
            match notes
                .iter()
                .rposition(|n| n.start <= lyric.time && lyric.time < n.start + n.duration)
            {
                Some(j) => i = j,
                None => continue,
            }
        }
        let s = notes[i].syllable.get_or_insert_with(String::new);
        s.push_str(&lyric.text);
    }
    notes
}

/// Put `syllables` on the notes in order, one per note; extra notes become
/// melisma, extra syllables are dropped.
#[must_use]
pub fn with_syllables(mut notes: Vec<SungNote>, syllables: &[String]) -> Vec<SungNote> {
    for (note, syl) in notes.iter_mut().zip(syllables) {
        note.syllable = Some(syl.clone());
    }
    notes
}

/// Split lyrics text into syllables, one per note: whitespace separates
/// words, `-` splits a word into syllables (`ba-na-na`), and a lone `_` holds
/// the previous vowel over one more note. A word without hyphens takes one
/// note per vowel sound, as the phonemizer hears it.
#[must_use]
pub fn split_syllables(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for word in text.split_whitespace() {
        if word == "_" {
            out.push(String::from("_"));
            continue;
        }
        let split;
        let parts: Vec<&str> = if word.contains('-') {
            word.split('-').collect()
        } else {
            split = auto_split(word);
            split.iter().map(String::as_str).collect()
        };
        for (k, part) in parts.iter().enumerate() {
            if k + 1 < parts.len() {
                out.push(alloc::format!("{part}-"));
            } else {
                out.push(alloc::format!("{part} "));
            }
        }
    }
    out
}

/// Split a word into as many pieces as it has vowel sounds. Only the count
/// matters: the pieces are joined again before the word is phonemized.
fn auto_split(word: &str) -> Vec<String> {
    let nuclei = phonemize(word)
        .iter()
        .filter(|t| matches!(t, Token::Phone { phoneme, .. } if is_vowel(*phoneme)))
        .count()
        .max(1);
    let chars: Vec<char> = word.chars().collect();
    let k = nuclei.min(chars.len().max(1));
    (0..k)
        .map(|i| {
            chars[i * chars.len() / k..(i + 1) * chars.len() / k]
                .iter()
                .collect()
        })
        .collect()
}

fn is_melisma(s: Option<&str>) -> bool {
    match s {
        None => true,
        Some(s) => {
            let t = s.trim_matches(|c: char| c.is_whitespace() || c == '/' || c == '\\');
            t.is_empty() || matches!(t, "-" | "_" | "+" | "~")
        }
    }
}

fn is_vowel(p: Phoneme) -> bool {
    matches!(p.manner(), Manner::Vowel | Manner::Diphthong)
}

/// A sung syllable: consonants before the vowel, the vowel(s), consonants
/// after, sung over one or more notes.
#[derive(Debug, Default)]
struct Syllable {
    onset: Vec<Phoneme>,
    nucleus: Vec<Phoneme>,
    coda: Vec<Phoneme>,
    /// (start, duration, key) per note, the first carrying the syllable.
    notes: Vec<(f64, f64, f32)>,
}

/// Group notes into words and syllables, and give every syllable its
/// phonemes.
fn syllables(notes: &[SungNote]) -> Vec<Syllable> {
    // Karaoke files mark word ends with spaces; others with hyphens only.
    let spaced = notes
        .iter()
        .filter_map(|n| n.syllable.as_deref())
        .any(|s| s.starts_with(' ') || s.ends_with(' '));

    // Words: runs of (note index, syllable text), melisma notes attached
    // to the syllable before them.
    let mut words: Vec<Vec<(usize, String, Vec<usize>)>> = Vec::new();
    let mut continues = false;
    for (i, n) in notes.iter().enumerate() {
        let syl = n.syllable.as_deref();
        if is_melisma(syl) {
            if let Some(last) = words.last_mut().and_then(|w| w.last_mut()) {
                last.2.push(i);
            }
            continue;
        }
        let raw = syl.unwrap_or_default();
        let starts_word = raw.starts_with(|c: char| c.is_whitespace() || c == '/' || c == '\\');
        if words.is_empty() || !continues || (spaced && starts_word) {
            words.push(Vec::new());
        }
        let trimmed = raw.trim_matches(|c: char| c.is_whitespace() || c == '/' || c == '\\');
        continues = if spaced {
            !raw.ends_with(char::is_whitespace) && !trimmed.ends_with(['.', ',', '!', '?'])
        } else {
            trimmed.ends_with('-')
        };
        let letters = trimmed.trim_end_matches('-').to_string();
        words
            .last_mut()
            .expect("pushed above")
            .push((i, letters, Vec::new()));
    }

    let mut out = Vec::new();
    for word in words {
        let text: String = word.iter().map(|(_, s, _)| s.as_str()).collect();
        let phones: Vec<Phoneme> = phonemize(&text)
            .into_iter()
            .filter_map(|t| match t {
                Token::Phone { phoneme, .. } => Some(phoneme),
                _ => None,
            })
            .collect();
        let k = word.len();
        let vowels: Vec<usize> = (0..phones.len()).filter(|&i| is_vowel(phones[i])).collect();
        let mut sylls: Vec<Syllable> = (0..k).map(|_| Syllable::default()).collect();

        if vowels.is_empty() {
            // "Hm", "mm": hum the last voiced sound.
            if let Some(&p) = phones.iter().rev().find(|p| p.is_voiced()) {
                sylls[0].nucleus.push(p);
            }
        } else {
            // Nuclei per syllable: spread the vowels over the sung syllables.
            let v = vowels.len();
            let mut owner = alloc::vec![0usize; v];
            for (j, o) in owner.iter_mut().enumerate() {
                *o = (j * k / v).min(k - 1);
            }
            for (j, &vi) in vowels.iter().enumerate() {
                sylls[owner[j]].nucleus.push(phones[vi]);
            }
            // Syllables without a vowel of their own sustain the previous one.
            for s in 1..k {
                if sylls[s].nucleus.is_empty() {
                    let prev = *sylls[s - 1].nucleus.last().expect("first has a vowel");
                    sylls[s].nucleus.push(prev);
                }
            }
            // Consonants: before the first vowel → onset of the first
            // syllable; after the last → coda of its owner; between two
            // vowels of different syllables: one consonant goes to the next
            // onset, a cluster leaves its first consonant behind.
            sylls[owner[0]]
                .onset
                .extend(phones[..vowels[0]].iter().copied());
            for j in 0..v {
                let from = vowels[j] + 1;
                let to = vowels.get(j + 1).copied().unwrap_or(phones.len());
                let cons = &phones[from..to];
                if j + 1 == v || owner[j] == owner[j + 1] {
                    sylls[owner[j]].coda.extend(cons.iter().copied());
                } else {
                    let split = usize::from(cons.len() >= 2);
                    sylls[owner[j]].coda.extend(cons[..split].iter().copied());
                    sylls[owner[j + 1]]
                        .onset
                        .extend(cons[split..].iter().copied());
                }
            }
        }

        for (s, (i, _, melisma)) in sylls.iter_mut().zip(&word) {
            for &n in core::iter::once(i).chain(melisma) {
                let note = &notes[n];
                s.notes.push((note.start, note.duration, note.key));
            }
        }
        out.extend(sylls);
    }
    out
}

/// Typical length of a consonant in a sung syllable, in seconds.
fn consonant_length(p: Phoneme) -> f32 {
    match p.manner() {
        Manner::Stop => 0.045,
        Manner::Affricate => 0.08,
        Manner::Nasal => 0.065,
        Manner::Fricative => match p {
            Phoneme::S | Phoneme::Sh | Phoneme::Z | Phoneme::Zh => 0.09,
            _ => 0.07,
        },
        Manner::Approximant => 0.05,
        Manner::Aspirate => 0.055,
        _ => 0.06,
    }
}

/// A syllable's absolute timeline: onset start, vowel start (the beat),
/// coda start, end.
struct Plan {
    onset_start: f64,
    beat: f64,
    coda_start: f64,
    end: f64,
}

/// Compile a melody into an articulation track sung with `voice`.
#[must_use]
#[allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss
)]
pub fn compile(notes: &[SungNote], voice: &VoiceSettings, options: &SingOptions) -> Utterance {
    let tempo = f64::from(options.tempo.clamp(0.1, 10.0));
    let scaled: Vec<SungNote> = notes
        .iter()
        .map(|n| SungNote {
            start: n.start / tempo,
            duration: n.duration / tempo,
            key: n.key + options.transpose,
            syllable: n.syllable.clone(),
        })
        .collect();
    let sylls = syllables(&scaled);
    if sylls.is_empty() {
        return Utterance::default();
    }

    let hz = |key: f32| crate::midi_to_hz(key);
    let tense = voice.tenseness.clamp(0.0, 1.0);
    let nasality = voice.nasality.clamp(0.0, 1.0);

    let len = |ps: &[Phoneme]| -> f64 { ps.iter().map(|&p| f64::from(consonant_length(p))).sum() };
    let mut plans: Vec<Plan> = sylls
        .iter()
        .map(|s| {
            let beat = s.notes[0].0;
            Plan {
                onset_start: beat - len(&s.onset),
                beat,
                coda_start: 0.0,
                end: s.notes.last().map_or(beat, |last| last.0 + last.1),
            }
        })
        .collect();
    for i in 0..plans.len() {
        // A syllable ends where the next one's onset begins.
        if let Some(next_onset) = plans.get(i + 1).map(|p| p.onset_start) {
            let next_beat = plans[i + 1].beat;
            plans[i].end = plans[i].end.min(next_onset.max(plans[i].beat));
            // Never let the next onset reach back past this vowel's middle.
            let floor = plans[i].beat + 0.5 * (plans[i].end - plans[i].beat).max(0.0);
            if plans[i + 1].onset_start < floor {
                plans[i + 1].onset_start = floor.min(next_beat);
            }
        }
        // Coda at the end, leaving the vowel at least 60 % of the note.
        let coda = len(&sylls[i].coda);
        let room = (plans[i].end - plans[i].beat) * 0.4;
        plans[i].coda_start = plans[i].end - coda.min(room.max(0.0));
    }

    let start = plans
        .iter()
        .map(|p| p.onset_start)
        .fold(f64::INFINITY, f64::min);
    let offset = f64::from(options.lead_in) - start;

    let mut segments: Vec<Segment> = Vec::new();
    let mut cursor = 0.0f64;
    let mut last_hz = hz(sylls[0].notes[0].2);

    let push = |segments: &mut Vec<Segment>,
                cursor: &mut f64,
                until: f64,
                phoneme: Phoneme,
                articulation: crate::phoneme::Articulation,
                voiced: bool,
                pitch: (f32, f32)| {
        let duration = (until - *cursor) as f32;
        if duration <= 0.0005 {
            return;
        }
        let mut articulation = articulation;
        if is_vowel(phoneme) || phoneme.manner() == Manner::Approximant {
            articulation.velum = articulation.velum.max(nasality);
        }
        segments.push(Segment {
            phoneme,
            articulation,
            duration,
            pitch,
            tenseness: articulation.tenseness.unwrap_or(tense),
            voiced,
        });
        *cursor = until;
    };

    for (s, p) in sylls.iter().zip(&plans) {
        let note_hz = hz(s.notes[0].2);
        let tongue = s.nucleus.first().and_then(|v| v.articulation().tongue);
        let with_tongue = |ph: Phoneme| {
            let mut a = ph.articulation();
            if a.tongue.is_none() {
                a.tongue = tongue;
            }
            a
        };

        // Rest before this syllable.
        let onset_start = p.onset_start + offset;
        if onset_start > cursor + 0.001 {
            push(
                &mut segments,
                &mut cursor,
                onset_start,
                Phoneme::Sil,
                Phoneme::Sil.articulation(),
                false,
                (last_hz, last_hz),
            );
        }

        // Onset consonants, squeezed into the time before the beat.
        let beat = p.beat + offset;
        let total = len(&s.onset).max(1e-6);
        let span = (beat - cursor).max(0.0);
        for (k, &c) in s.onset.iter().enumerate() {
            let share = f64::from(consonant_length(c)) / total * span;
            let until = if k + 1 == s.onset.len() {
                beat
            } else {
                cursor + share
            };
            let voiced = c.is_voiced();
            push(
                &mut segments,
                &mut cursor,
                until,
                c,
                with_tongue(c),
                voiced,
                (note_hz, note_hz),
            );
        }

        // The vowel over every note of the syllable; a diphthong glides in
        // the last quarter of the syllable.
        let coda_start = p.coda_start + offset;
        let nuclei = s.nucleus.len().max(1);
        for (n, &(nstart, ndur, key)) in s.notes.iter().enumerate() {
            let target = hz(key);
            let note_end = if n + 1 == s.notes.len() {
                coda_start
            } else {
                (nstart + ndur + offset).min(coda_start)
            };
            // Slide into the new pitch, then hold it.
            if options.portamento > 0.0 && (target - last_hz).abs() > 0.5 {
                let slide_end = (cursor + f64::from(options.portamento)).min(note_end);
                let v = s.nucleus.first().copied().unwrap_or(Phoneme::Ax);
                push(
                    &mut segments,
                    &mut cursor,
                    slide_end,
                    v,
                    with_tongue(v),
                    true,
                    (last_hz, target),
                );
            }
            let span = (note_end - cursor).max(0.0);
            if s.nucleus.is_empty() {
                // Nothing to sing on (a word of unvoiced consonants): keep quiet.
                let sil = Phoneme::Sil;
                push(
                    &mut segments,
                    &mut cursor,
                    note_end,
                    sil,
                    sil.articulation(),
                    false,
                    (target, target),
                );
                last_hz = target;
                continue;
            }
            // The nucleus vowels that fall on this note (at least one).
            let from = (n * nuclei / s.notes.len()).min(nuclei - 1);
            let to = ((n + 1) * nuclei / s.notes.len()).clamp(from + 1, nuclei);
            let vowels = &s.nucleus[from..to];
            let each = span / vowels.len() as f64;
            for (k, &v) in vowels.iter().enumerate() {
                let until = if k + 1 == vowels.len() {
                    note_end
                } else {
                    cursor + each
                };
                let last_note = n + 1 == s.notes.len() && k + 1 == vowels.len();
                match v.glide() {
                    Some((_, to_vowel)) if last_note => {
                        let hold = cursor + (until - cursor) * 0.75;
                        push(
                            &mut segments,
                            &mut cursor,
                            hold,
                            v,
                            v.articulation(),
                            true,
                            (target, target),
                        );
                        push(
                            &mut segments,
                            &mut cursor,
                            until,
                            v,
                            to_vowel.articulation(),
                            true,
                            (target, target),
                        );
                    }
                    _ => {
                        let a = if is_vowel(v) {
                            v.articulation()
                        } else {
                            with_tongue(v)
                        };
                        push(
                            &mut segments,
                            &mut cursor,
                            until,
                            v,
                            a,
                            v.is_voiced(),
                            (target, target),
                        );
                    }
                }
            }
            last_hz = target;
        }

        // Coda consonants close the note.
        let end = p.end + offset;
        let total = len(&s.coda).max(1e-6);
        let span = (end - cursor).max(0.0);
        for (k, &c) in s.coda.iter().enumerate() {
            let share = f64::from(consonant_length(c)) / total * span;
            let until = if k + 1 == s.coda.len() {
                end
            } else {
                cursor + share
            };
            push(
                &mut segments,
                &mut cursor,
                until,
                c,
                with_tongue(c),
                c.is_voiced(),
                (last_hz, last_hz),
            );
        }
    }

    // Let the last sound ring out briefly.
    let tail = cursor + 0.15;
    push(
        &mut segments,
        &mut cursor,
        tail,
        Phoneme::Sil,
        Phoneme::Sil.articulation(),
        false,
        (last_hz, last_hz),
    );
    Utterance::from_segments(segments)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::midi::{
        self,
        tests::{lyric, smf},
    };
    use crate::speech::Speaker;
    use crate::{Preset, Trombone};
    use alloc::vec;

    fn note(start: f64, duration: f64, key: f32, syl: &str) -> SungNote {
        SungNote {
            start,
            duration,
            key,
            syllable: if syl.is_empty() {
                None
            } else {
                Some(syl.into())
            },
        }
    }

    fn voice() -> VoiceSettings {
        Preset::Android.settings()
    }

    #[test]
    fn vowels_land_on_the_beat() {
        let notes = vec![note(1.0, 0.5, 60.0, "stop "), note(1.5, 0.5, 64.0, "go ")];
        let u = compile(
            &notes,
            &voice(),
            &SingOptions {
                lead_in: 0.0,
                ..SingOptions::default()
            },
        );
        let mut t = 0.0f64;
        let mut vowel_starts = Vec::new();
        let mut prev_vowel = false;
        for s in u.segments() {
            let v = is_vowel(s.phoneme);
            if v && !prev_vowel {
                vowel_starts.push(t);
            }
            prev_vowel = v;
            t += f64::from(s.duration);
        }
        // Offsets are relative to the first onset; the beats are 0.5 s apart.
        assert_eq!(vowel_starts.len(), 2);
        assert!(
            (vowel_starts[1] - vowel_starts[0] - 0.5).abs() < 0.01,
            "{vowel_starts:?}"
        );
    }

    #[test]
    fn vowels_hold_the_note_pitch() {
        let notes = vec![note(0.0, 0.6, 69.0, "[AA] ")];
        let u = compile(&notes, &voice(), &SingOptions::default());
        let vowel = u.segments().iter().find(|s| is_vowel(s.phoneme)).unwrap();
        assert!((vowel.pitch.0 - 440.0).abs() < 0.5 && (vowel.pitch.1 - 440.0).abs() < 0.5);
    }

    #[test]
    fn hyphens_split_a_word_over_notes() {
        let notes = vec![
            note(0.0, 0.4, 60.0, "ba-"),
            note(0.4, 0.4, 62.0, "na-"),
            note(0.8, 0.4, 64.0, "na "),
        ];
        let sylls = syllables(&notes);
        assert_eq!(sylls.len(), 3);
        assert!(sylls.iter().all(|s| !s.nucleus.is_empty()));
        assert_eq!(sylls[1].onset, vec![Phoneme::N]);
    }

    #[test]
    fn notes_without_syllables_extend_the_vowel() {
        let notes = vec![
            note(0.0, 0.3, 60.0, "oh "),
            note(0.3, 0.3, 64.0, ""),
            note(0.6, 0.3, 67.0, "_"),
        ];
        let sylls = syllables(&notes);
        assert_eq!(sylls.len(), 1);
        assert_eq!(sylls[0].notes.len(), 3);
        let u = compile(&notes, &voice(), &SingOptions::default());
        let pitches: Vec<f32> = u
            .segments()
            .iter()
            .filter(|s| is_vowel(s.phoneme))
            .map(|s| s.pitch.1)
            .collect();
        assert!(
            pitches.windows(2).any(|w| w[1] > w[0] + 10.0),
            "{pitches:?}"
        );
    }

    #[test]
    fn split_syllables_marks_word_ends() {
        let s = split_syllables("la-la land");
        assert_eq!(s, vec!["la-", "la ", "land "]);
    }

    #[test]
    fn words_without_hyphens_take_one_note_per_vowel() {
        // "banana" has three vowel sounds, "cat" one; "_" holds.
        let s = split_syllables("banana cat _");
        assert_eq!(s.len(), 5, "{s:?}");
        assert!(s[0].ends_with('-') && s[1].ends_with('-') && s[2].ends_with(' '));
        assert_eq!(s[3], "cat ");
        assert_eq!(s[4], "_");
        // Joined again, the word sings as one word over three notes.
        let notes: Vec<SungNote> = s
            .iter()
            .enumerate()
            .map(|(i, syl)| note(f64::from(u8::try_from(i).unwrap()) * 0.3, 0.3, 60.0, syl))
            .collect();
        let sylls = syllables(&notes);
        assert_eq!(sylls.len(), 4);
        assert_eq!(sylls[3].notes.len(), 2, "cat holds over the _ note");
    }

    #[test]
    fn sings_a_midi_file() {
        // An original four-note phrase, one syllable per note.
        let mut events = vec![(0, vec![0xff, 0x51, 0x03, 0x07, 0xa1, 0x20])];
        for (key, syl) in [(60u8, "Sing "), (62, "a "), (64, "lit-"), (67, "tle ")] {
            events.push((0, lyric(syl)));
            events.push((0, vec![0x90, key, 100]));
            events.push((240, vec![0x80, key, 0]));
        }
        let file = midi::parse(&smf(&[events])).unwrap();
        let notes = melody(&file.tracks[0]);
        assert_eq!(notes.len(), 4);
        assert_eq!(notes[2].syllable.as_deref(), Some("lit-"));

        let u = compile(&notes, &voice(), &SingOptions::default());
        let mut speaker = Speaker::new();
        let mut v = Trombone::new(16_000.0);
        Speaker::prepare(&mut v);
        Preset::Android.settings().apply(&mut v);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let mut buf = vec![0.0; ((u.duration() + 0.2) * 16_000.0) as usize];
        speaker.speak(u);
        speaker.render(&mut v, &mut buf);
        assert!(buf.iter().all(|s| s.is_finite()));
        assert!(buf.iter().map(|s| s * s).sum::<f32>() > 1.0);
    }
}
