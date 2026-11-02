//! A small Standard MIDI File reader: notes, lyrics and track names, with
//! times in seconds.
//!
//! Reads format 0 and 1 files with tick-based timing, follows the tempo map
//! (tempo changes in any track), handles running status, and collects lyric
//! meta events (`FF 05`) as well as text events (`FF 01`), which karaoke
//! (`.kar`) files use for their lyrics.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

/// A note with absolute timing.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    /// Start in seconds.
    pub start: f64,
    /// Length in seconds.
    pub duration: f64,
    /// MIDI key, 60 is middle C.
    pub key: u8,
    /// Note-on velocity.
    pub velocity: u8,
    /// MIDI channel, 0-based.
    pub channel: u8,
}

/// A lyric or text event.
#[derive(Clone, Debug, PartialEq)]
pub struct Lyric {
    /// Time in seconds.
    pub time: f64,
    /// The text, as stored in the file.
    pub text: String,
}

/// One track of a MIDI file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    /// The track name meta event, if any.
    pub name: Option<String>,
    /// Notes, sorted by start time.
    pub notes: Vec<Note>,
    /// Lyric meta events (`FF 05`), in order.
    pub lyrics: Vec<Lyric>,
    /// Text events (`FF 01`), in order; karaoke files keep lyrics here.
    pub texts: Vec<Lyric>,
}

/// A parsed MIDI file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MidiFile {
    /// The tracks, in file order.
    pub tracks: Vec<Track>,
}

/// Why a file could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MidiError {
    /// The data does not start with an `MThd` header.
    NotMidi,
    /// The file ends in the middle of a chunk or event.
    Truncated,
    /// SMPTE time division is not supported.
    SmpteTiming,
    /// Format 2 (independent sequences) is not supported.
    Format2,
    /// A variable-length number runs past its four-byte limit.
    Malformed,
}

impl fmt::Display for MidiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            MidiError::NotMidi => "not a Standard MIDI File (no MThd header)",
            MidiError::Truncated => "the MIDI file is truncated",
            MidiError::SmpteTiming => "SMPTE time division is not supported",
            MidiError::Format2 => "MIDI format 2 is not supported",
            MidiError::Malformed => {
                "the MIDI file is malformed (a variable-length number is too long)"
            }
        })
    }
}

#[cfg(feature = "std")]
impl std::error::Error for MidiError {}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn done(&self) -> bool {
        self.pos >= self.data.len()
    }

    fn u8(&mut self) -> Result<u8, MidiError> {
        let b = *self.data.get(self.pos).ok_or(MidiError::Truncated)?;
        self.pos += 1;
        Ok(b)
    }

    fn peek(&self) -> Result<u8, MidiError> {
        self.data.get(self.pos).copied().ok_or(MidiError::Truncated)
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8], MidiError> {
        let end = self.pos.checked_add(n).ok_or(MidiError::Truncated)?;
        let b = self.data.get(self.pos..end).ok_or(MidiError::Truncated)?;
        self.pos = end;
        Ok(b)
    }

    fn u32(&mut self) -> Result<u32, MidiError> {
        let b = self.bytes(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Variable-length quantity: at most four bytes, the last one without
    /// the continuation bit. Anything longer is malformed, and failing here
    /// keeps the parser from reading the rest of the event as a new one.
    fn vlq(&mut self) -> Result<u32, MidiError> {
        let mut v = 0u32;
        for _ in 0..4 {
            let b = self.u8()?;
            v = (v << 7) | u32::from(b & 0x7f);
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err(MidiError::Malformed)
    }

    fn rest(&self) -> &'a [u8] {
        &self.data[self.pos.min(self.data.len())..]
    }
}

/// An event before tick-to-seconds conversion.
enum Raw {
    NoteOn {
        tick: u64,
        key: u8,
        velocity: u8,
        channel: u8,
    },
    NoteOff {
        tick: u64,
        key: u8,
        channel: u8,
    },
    Lyric {
        tick: u64,
        text: String,
    },
    Text {
        tick: u64,
        text: String,
    },
}

/// Decode MIDI text. It is nominally ASCII, with Latin-1 the common practice
/// beyond it. Control characters (C0, DEL, C1) are dropped, so text from an
/// untrusted file cannot carry terminal escape sequences.
fn text(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| char::from(b))
        .filter(|c| !c.is_control())
        .collect()
}

/// Parse a Standard MIDI File.
///
/// # Errors
///
/// See [`MidiError`].
#[allow(clippy::cast_precision_loss, clippy::too_many_lines)] // one pass over the format
pub fn parse(data: &[u8]) -> Result<MidiFile, MidiError> {
    let mut r = Reader::new(data);
    if r.bytes(4).map_err(|_| MidiError::NotMidi)? != b"MThd" {
        return Err(MidiError::NotMidi);
    }
    let header_len = r.u32()? as usize;
    let header = r.bytes(header_len)?;
    if header.len() < 6 {
        return Err(MidiError::Truncated);
    }
    let format = u16::from_be_bytes([header[0], header[1]]);
    let division = u16::from_be_bytes([header[4], header[5]]);
    if format == 2 {
        return Err(MidiError::Format2);
    }
    if division & 0x8000 != 0 {
        return Err(MidiError::SmpteTiming);
    }
    let ppq = f64::from(division.max(1));

    let mut tempo: Vec<(u64, u32)> = Vec::new(); // (tick, microseconds per quarter)
    let mut raw_tracks: Vec<(Option<String>, Vec<Raw>)> = Vec::new();

    while !r.done() {
        // Some writers pad files with zero bytes; any other leftover that is
        // too short for a chunk header means the file was cut off.
        if r.rest().iter().all(|&b| b == 0) {
            break;
        }
        let id = r.bytes(4)?;
        let len = r.u32()? as usize;
        let chunk = r.bytes(len)?;
        if id != b"MTrk" {
            continue;
        }
        let mut t = Reader::new(chunk);
        let mut tick = 0u64;
        let mut status = 0u8;
        let mut name = None;
        let mut events = Vec::new();
        while !t.done() {
            tick += u64::from(t.vlq()?);
            let mut b = t.peek()?;
            if b & 0x80 != 0 {
                t.pos += 1;
                if b < 0xf0 {
                    status = b;
                }
            } else {
                // Running status: reuse the last channel status.
                b = status;
            }
            match b {
                0xff => {
                    let kind = t.u8()?;
                    let len = t.vlq()? as usize;
                    let data = t.bytes(len)?;
                    match kind {
                        0x51 if data.len() == 3 => {
                            let us = u32::from_be_bytes([0, data[0], data[1], data[2]]);
                            tempo.push((tick, us));
                        }
                        0x03 if name.is_none() => name = Some(text(data)),
                        0x05 => events.push(Raw::Lyric {
                            tick,
                            text: text(data),
                        }),
                        0x01 => events.push(Raw::Text {
                            tick,
                            text: text(data),
                        }),
                        0x2f => break,
                        _ => {}
                    }
                }
                0xf0 | 0xf7 => {
                    let len = t.vlq()? as usize;
                    t.bytes(len)?;
                }
                0x80..=0xef => {
                    let channel = b & 0x0f;
                    match b & 0xf0 {
                        0x80 => {
                            let key = t.u8()?;
                            t.u8()?;
                            events.push(Raw::NoteOff { tick, key, channel });
                        }
                        0x90 => {
                            let key = t.u8()?;
                            let velocity = t.u8()?;
                            events.push(if velocity == 0 {
                                Raw::NoteOff { tick, key, channel }
                            } else {
                                Raw::NoteOn {
                                    tick,
                                    key,
                                    velocity,
                                    channel,
                                }
                            });
                        }
                        0xc0 | 0xd0 => {
                            t.u8()?;
                        }
                        _ => {
                            t.bytes(2)?;
                        }
                    }
                }
                // A data byte with no running status to apply: skip it.
                _ => {
                    t.pos += 1;
                }
            }
        }
        raw_tracks.push((name, events));
    }

    // Tempo map: tick -> seconds.
    tempo.sort_by_key(|&(tick, _)| tick);
    if tempo.first().is_none_or(|&(tick, _)| tick > 0) {
        tempo.insert(0, (0, 500_000));
    }
    let mut map: Vec<(u64, f64, f64)> = Vec::with_capacity(tempo.len()); // (tick, seconds, s/tick)
    let mut seconds = 0.0;
    for (k, &(tick, us)) in tempo.iter().enumerate() {
        if let Some(&(prev_tick, prev_s, rate)) = map.last() {
            seconds = prev_s + (tick - prev_tick) as f64 * rate;
        }
        let rate = f64::from(us) / 1_000_000.0 / ppq;
        if k > 0 && map.last().is_some_and(|m| m.0 == tick) {
            map.pop();
        }
        map.push((tick, seconds, rate));
    }
    let to_seconds = |tick: u64| -> f64 {
        let i = map.partition_point(|m| m.0 <= tick).saturating_sub(1);
        let (t0, s0, rate) = map[i];
        s0 + (tick - t0) as f64 * rate
    };

    let mut file = MidiFile::default();
    for (name, events) in raw_tracks {
        let mut track = Track {
            name,
            ..Track::default()
        };
        // Open notes per (channel, key): start tick and velocity.
        let mut open: Vec<(u8, u8, u64, u8)> = Vec::new();
        for e in events {
            match e {
                Raw::NoteOn {
                    tick,
                    key,
                    velocity,
                    channel,
                } => {
                    open.push((channel, key, tick, velocity));
                }
                Raw::NoteOff { tick, key, channel } => {
                    if let Some(i) = open.iter().position(|o| o.0 == channel && o.1 == key) {
                        let (channel, key, start, velocity) = open.remove(i);
                        let s = to_seconds(start);
                        track.notes.push(Note {
                            start: s,
                            duration: (to_seconds(tick) - s).max(0.0),
                            key,
                            velocity,
                            channel,
                        });
                    }
                }
                Raw::Lyric { tick, text } => track.lyrics.push(Lyric {
                    time: to_seconds(tick),
                    text,
                }),
                Raw::Text { tick, text } => track.texts.push(Lyric {
                    time: to_seconds(tick),
                    text,
                }),
            }
        }
        track
            .notes
            .sort_by(|a, b| a.start.total_cmp(&b.start).then(b.key.cmp(&a.key)));
        file.tracks.push(track);
    }
    Ok(file)
}

/// Write a single-track MIDI file (format 0, 120 bpm) with `notes` given
/// as `(start seconds, duration seconds, key, lyric)`. Lyrics become lyric
/// meta events on their notes.
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn write(notes: &[(f64, f64, u8, Option<&str>)]) -> Vec<u8> {
    // 480 ticks per quarter at 120 bpm: 960 ticks per second.
    const TICKS_PER_SECOND: f64 = 960.0;
    // Round half up by hand: `f64::round` is not in `core`.
    let tick = |s: f64| (s.max(0.0) * TICKS_PER_SECOND + 0.5) as u32;
    // (tick, order, bytes): note-offs before note-ons at the same tick.
    let mut events: Vec<(u32, u8, Vec<u8>)> = Vec::new();
    for &(start, duration, key, lyric) in notes {
        let on = tick(start);
        let off = tick(start + duration).max(on + 1);
        if let Some(text) = lyric {
            let mut e = alloc::vec![0xff, 0x05];
            let bytes: Vec<u8> = text
                .chars()
                .map(|c| u8::try_from(c).unwrap_or(b'?'))
                .collect();
            push_vlq(bytes.len() as u32, &mut e);
            e.extend(bytes);
            events.push((on, 1, e));
        }
        events.push((on, 2, alloc::vec![0x90, key & 0x7f, 96]));
        events.push((off, 0, alloc::vec![0x80, key & 0x7f, 0]));
    }
    events.sort_by_key(|e| (e.0, e.1));

    let mut body = Vec::new();
    // Tempo: 500000 us per quarter (120 bpm).
    body.extend([0x00, 0xff, 0x51, 0x03, 0x07, 0xa1, 0x20]);
    let mut now = 0u32;
    for (t, _, bytes) in events {
        push_vlq(t - now, &mut body);
        body.extend(bytes);
        now = t;
    }
    body.extend([0x00, 0xff, 0x2f, 0x00]);

    let mut out = b"MThd".to_vec();
    out.extend(6u32.to_be_bytes());
    out.extend(0u16.to_be_bytes());
    out.extend(1u16.to_be_bytes());
    out.extend(480u16.to_be_bytes());
    out.extend(b"MTrk");
    out.extend((body.len() as u32).to_be_bytes());
    out.extend(body);
    out
}

fn push_vlq(mut v: u32, out: &mut Vec<u8>) {
    let mut bytes = [0u8; 5];
    let mut n = 0;
    loop {
        bytes[n] = (v & 0x7f) as u8;
        n += 1;
        v >>= 7;
        if v == 0 {
            break;
        }
    }
    for k in (0..n).rev() {
        out.push(bytes[k] | if k > 0 { 0x80 } else { 0 });
    }
}

impl Track {
    /// The track's sung text: lyric events if there are any, otherwise text
    /// events that look like karaoke lyrics (not `@` header fields).
    #[must_use]
    pub fn sung_text(&self) -> Vec<Lyric> {
        if !self.lyrics.is_empty() {
            return self.lyrics.clone();
        }
        self.texts
            .iter()
            .filter(|t| !t.text.starts_with('@'))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use alloc::vec;

    fn vlq(mut v: u32, out: &mut Vec<u8>) {
        let mut stack = vec![(v & 0x7f) as u8];
        v >>= 7;
        while v > 0 {
            stack.push(((v & 0x7f) as u8) | 0x80);
            v >>= 7;
        }
        out.extend(stack.into_iter().rev());
    }

    /// A one-track file: (delta ticks, raw event bytes) pairs, 480 ppq.
    pub(crate) fn smf(tracks: &[Vec<(u32, Vec<u8>)>]) -> Vec<u8> {
        let mut out = b"MThd".to_vec();
        out.extend(6u32.to_be_bytes());
        out.extend(1u16.to_be_bytes());
        out.extend(u16::try_from(tracks.len()).unwrap().to_be_bytes());
        out.extend(480u16.to_be_bytes());
        for events in tracks {
            let mut body = Vec::new();
            for (delta, bytes) in events {
                vlq(*delta, &mut body);
                body.extend(bytes);
            }
            body.extend([0x00, 0xff, 0x2f, 0x00]);
            out.extend(b"MTrk");
            out.extend(u32::try_from(body.len()).unwrap().to_be_bytes());
            out.extend(body);
        }
        out
    }

    pub(crate) fn lyric(s: &str) -> Vec<u8> {
        let mut v = vec![0xff, 0x05, u8::try_from(s.len()).unwrap()];
        v.extend(s.bytes());
        v
    }

    #[test]
    fn reads_notes_lyrics_and_tempo() {
        // 120 bpm (500000 us/quarter) then 60 bpm after one quarter.
        let conductor = vec![
            (0, vec![0xff, 0x51, 0x03, 0x07, 0xa1, 0x20]),
            (480, vec![0xff, 0x51, 0x03, 0x0f, 0x42, 0x40]),
        ];
        let melody = vec![
            (0, vec![0xff, 0x03, 0x03, b'V', b'o', b'x']),
            (0, lyric("La ")),
            (0, vec![0x90, 60, 100]),
            (480, vec![0x80, 60, 0]),
            (0, lyric("lu")),
            // Running status, and note-on with velocity 0 as note-off.
            (0, vec![0x90, 64, 90]),
            (0, vec![67, 80]),
            (480, vec![64, 0]),
            (0, vec![67, 0]),
        ];
        let f = parse(&smf(&[conductor, melody])).unwrap();
        let t = &f.tracks[1];
        assert_eq!(t.name.as_deref(), Some("Vox"));
        assert_eq!(t.notes.len(), 3);
        assert!((t.notes[0].start - 0.0).abs() < 1e-9);
        assert!((t.notes[0].duration - 0.5).abs() < 1e-9, "120 bpm quarter");
        assert!((t.notes[1].start - 0.5).abs() < 1e-9);
        assert!((t.notes[1].duration - 1.0).abs() < 1e-9, "60 bpm quarter");
        // Simultaneous notes: higher key first.
        assert_eq!((t.notes[1].key, t.notes[2].key), (67, 64));
        assert_eq!(t.lyrics.len(), 2);
        assert_eq!(t.lyrics[1].text, "lu");
        assert!((t.lyrics[1].time - 0.5).abs() < 1e-9);
    }

    #[test]
    fn strips_control_characters_from_text() {
        let mut name = vec![0xff, 0x03, 0x07];
        name.extend(b"\x1b[2JVox");
        let mut syl = vec![0xff, 0x05, 0x03];
        syl.extend([b'L', 0x9b, b'a']);
        let f = parse(&smf(&[vec![(0, name), (0, syl)]])).unwrap();
        assert_eq!(f.tracks[0].name.as_deref(), Some("[2JVox"));
        assert_eq!(f.tracks[0].lyrics[0].text, "La");
    }

    #[test]
    fn written_files_read_back() {
        let bytes = write(&[
            (0.0, 0.5, 60, Some("Sing ")),
            (0.5, 1.0, 64, None),
            (2.0, 0.25, 67, Some("la ")),
        ]);
        let f = parse(&bytes).unwrap();
        let t = &f.tracks[0];
        assert_eq!(t.notes.len(), 3);
        assert!(
            (t.notes[1].start - 0.5).abs() < 0.002 && (t.notes[1].duration - 1.0).abs() < 0.002
        );
        assert!((t.notes[2].start - 2.0).abs() < 0.002);
        assert_eq!(t.lyrics.len(), 2);
        assert_eq!(t.lyrics[1].text, "la ");
    }

    #[test]
    fn rejects_overlong_variable_length_numbers() {
        // A delta time whose fourth byte still carries the continuation bit.
        let body = [0x81, 0x81, 0x81, 0x81, 0x01, 0x90, 60, 100];
        let mut f = b"MThd".to_vec();
        f.extend(6u32.to_be_bytes());
        f.extend([0, 0, 0, 1, 0x01, 0xe0]);
        f.extend(b"MTrk");
        f.extend(u32::try_from(body.len()).unwrap().to_be_bytes());
        f.extend(body);
        assert_eq!(parse(&f), Err(MidiError::Malformed));
    }

    #[test]
    fn trailing_bytes_are_padding_or_truncation() {
        let f = smf(&[vec![(0, vec![0x90, 60, 100]), (480, vec![0x80, 60, 0])]]);
        let mut padded = f.clone();
        padded.extend([0, 0, 0]);
        assert!(parse(&padded).is_ok(), "zero padding is fine");
        let mut cut = f;
        cut.extend(b"MT");
        assert_eq!(parse(&cut), Err(MidiError::Truncated));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse(b"nope"), Err(MidiError::NotMidi));
        assert_eq!(parse(b"MThd\0\0\0\x06\0"), Err(MidiError::Truncated));
        let mut f = smf(&[vec![(0, vec![0x90, 60, 100])]]);
        f.truncate(f.len() - 3);
        assert!(parse(&f).is_err());
    }

    #[test]
    fn karaoke_text_events_count_as_lyrics() {
        let mut header = vec![0xff, 0x01, 0x05];
        header.extend(b"@KMID");
        let mut syl = vec![0xff, 0x01, 0x03];
        syl.extend(b"Hey");
        let f = parse(&smf(&[vec![
            (0, header),
            (0, syl),
            (0, vec![0x90, 60, 100]),
            (480, vec![0x80, 60, 0]),
        ]]))
        .unwrap();
        let sung = f.tracks[0].sung_text();
        assert_eq!(sung.len(), 1);
        assert_eq!(sung[0].text, "Hey");
    }
}
