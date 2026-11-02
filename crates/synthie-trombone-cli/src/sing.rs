//! `synthie-trombone sing`: sing the melody of a MIDI file.
//!
//! Lyrics come from the file's lyric events (karaoke files work too) or from
//! a text file given with `--lyrics`: whitespace separates words, `-` splits
//! a word into syllables, and a lone `_` holds a vowel over one more note.

use anyhow::{Context, Result, bail};
use clap::Args;
use std::io::Write;
use std::path::PathBuf;
use synthie::audio::fx::Reverb;
use synthie_trombone::midi::{self, MidiFile};
use synthie_trombone::sing::{self, SingOptions};
use synthie_trombone::speech::{Speaker, Utterance};
use synthie_trombone::{Preset, Trombone, VoiceSettings, normalize};

/// Sing a MIDI melody.
#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)] // independent command-line switches
pub struct SingArgs {
    /// The MIDI file (.mid, .kar).
    pub midi: PathBuf,
    /// List the file's tracks and exit.
    #[arg(long)]
    pub list: bool,
    /// Track to sing (see --list); defaults to the first track with lyrics.
    #[arg(long)]
    pub track: Option<usize>,
    /// Lyrics text file to use instead of the MIDI's lyric events.
    #[arg(long)]
    pub lyrics: Option<PathBuf>,
    /// Start at this time in the MIDI file, in seconds.
    #[arg(long, default_value_t = 0.0)]
    pub from: f64,
    /// Stop at this time in the MIDI file, in seconds.
    #[arg(long)]
    pub until: Option<f64>,
    /// Semitones to shift the melody by.
    #[arg(long, default_value_t = 0.0, allow_negative_numbers = true)]
    pub transpose: f32,
    /// Tempo factor (2 sings twice as fast).
    #[arg(long, default_value_t = 1.0)]
    pub tempo: f32,
    /// Seconds of pitch slide into each note; 0 jumps.
    #[arg(long, default_value_t = 0.0)]
    pub portamento: f32,
    /// Voice preset.
    #[arg(long, default_value = "android")]
    pub voice: String,
    /// Throat length relative to the native tract (overrides the preset).
    #[arg(long)]
    pub tract: Option<f32>,
    /// Glottal tenseness, 0..1 (overrides the preset).
    #[arg(long)]
    pub tenseness: Option<f32>,
    /// Room reverb amount, 0..1.
    #[arg(long, default_value_t = 0.12)]
    pub room: f32,
    /// Write a WAV file instead of singing aloud (add --play for both).
    #[arg(short, long)]
    pub output: Option<PathBuf>,
    /// Sing aloud even when writing a WAV file.
    #[arg(long)]
    pub play: bool,
    /// Sample rate of the WAV file (singing aloud uses the device's rate).
    #[arg(long, default_value_t = 48_000)]
    pub rate: u32,
    /// Print the syllables as they map onto notes.
    #[arg(long)]
    pub verbose: bool,
    /// Print the melody notes (numbered, with gaps) in the range and exit;
    /// use it to place `@<seconds>` anchors and `*` skips in the lyrics.
    #[arg(long)]
    pub notes: bool,
    /// Ignore notes below this MIDI key when finding the melody.
    #[arg(long)]
    pub min_key: Option<u8>,
    /// Silence in seconds that ends a phrase when lining up lyrics lines.
    #[arg(long, default_value_t = 0.3)]
    pub gap: f64,
    /// Also write the sung notes, with their syllables, as a new MIDI file.
    #[arg(long)]
    pub export: Option<PathBuf>,
    /// Level of the band (the notes that aren't sung, played by synthie)
    /// against the voice, 0..1.
    #[arg(long, default_value_t = 0.35)]
    pub band: f32,
    /// Sing alone, without the band.
    #[arg(long)]
    pub no_band: bool,
    /// synthie preset for the band (e.g. "music box", "tri pad", "plucky pulse").
    #[arg(long, default_value = "music box")]
    pub band_patch: String,
}

fn list(file: &MidiFile) {
    println!("track  notes  lyrics  range     name");
    for (i, t) in file.tracks.iter().enumerate() {
        let range = match (
            t.notes.iter().map(|n| n.key).min(),
            t.notes.iter().map(|n| n.key).max(),
        ) {
            (Some(lo), Some(hi)) => format!("{lo}–{hi}"),
            _ => "–".into(),
        };
        println!(
            "{i:>5}  {:>5}  {:>6}  {range:<8}  {}",
            t.notes.len(),
            t.sung_text().len(),
            t.name.as_deref().unwrap_or("")
        );
    }
}

/// Run the `sing` subcommand.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
pub fn run(args: &SingArgs) -> Result<()> {
    let bytes =
        std::fs::read(&args.midi).with_context(|| format!("reading {}", args.midi.display()))?;
    let file = midi::parse(&bytes).with_context(|| format!("parsing {}", args.midi.display()))?;
    if args.list {
        list(&file);
        return Ok(());
    }

    let index = match args.track {
        Some(i) if i < file.tracks.len() => i,
        Some(i) => bail!(
            "track {i} does not exist; the file has {} (see --list)",
            file.tracks.len()
        ),
        None => file
            .tracks
            .iter()
            .position(|t| !t.notes.is_empty() && !t.sung_text().is_empty())
            .or_else(|| file.tracks.iter().position(|t| !t.notes.is_empty()))
            .context("the file has no notes")?,
    };

    let lyrics = match &args.lyrics {
        Some(path) => Some(
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?,
        ),
        None => None,
    };
    let in_range =
        |n: &sing::SungNote| n.start + 1e-6 >= args.from && args.until.is_none_or(|u| n.start < u);
    let track = &file.tracks[index];

    let min_key = args
        .min_key
        .or_else(|| voice_floor(track, lyrics.as_deref()?, &in_range, args.gap));
    let mut notes = sing::melody_with(track, &sing::MelodyOptions { min_key });
    notes.retain(in_range);
    if args.notes {
        print_notes(&notes);
        return Ok(());
    }
    if let Some(text) = &lyrics {
        notes = sing::align_lines(&notes, text, args.gap);
    }
    if notes.is_empty() {
        bail!(
            "no notes between {}s and {:?}s on track {index}",
            args.from,
            args.until
        );
    }
    if notes.iter().all(|n| n.syllable.is_none()) {
        bail!("track {index} has no lyrics in that range; give a lyrics file with --lyrics");
    }

    if args.verbose {
        for n in &notes {
            println!(
                "{:>8.3}s {:>6.3}s  key {:>5.1}  {}",
                n.start,
                n.duration,
                n.key + args.transpose,
                n.syllable.as_deref().unwrap_or("·")
            );
        }
    }

    let preset = Preset::from_name(&args.voice).with_context(|| {
        let names: Vec<_> = Preset::ALL.iter().map(|p| p.name()).collect();
        format!(
            "unknown voice {:?}; try one of {}",
            args.voice,
            names.join(", ")
        )
    })?;
    let mut settings = preset.settings();
    if let Some(t) = args.tract {
        settings.tract_length = t;
    }
    if let Some(t) = args.tenseness {
        settings.tenseness = t;
    }

    let options = SingOptions {
        transpose: args.transpose,
        tempo: args.tempo,
        portamento: args.portamento,
        ..SingOptions::default()
    };
    if let Some(path) = &args.export {
        export(path, &notes)?;
    }
    let (utterance, offset) = sing::compile_timed(&notes, &settings, &options);
    let band = if args.no_band {
        None
    } else {
        Some(Band::new(&file.tracks[index].notes, &notes, args, offset)?)
    };
    let song = Song {
        utterance,
        settings,
        band,
    };
    deliver(args, &song, notes.len(), index)
}

/// What gets rendered: the voice and, optionally, the band.
struct Song {
    utterance: Utterance,
    settings: VoiceSettings,
    band: Option<Band>,
}

/// The notes that aren't sung, played by synthie in step with the voice.
struct Band {
    /// (start, duration, key) in render seconds.
    notes: Vec<(f64, f64, u8)>,
    patch: synthie::prelude::SynthParams,
    level: f32,
}

impl Band {
    /// Every note of the track in the sung range that the voice doesn't
    /// sing. Without --until it stops shortly after the last sung note.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn new(
        track_notes: &[midi::Note],
        sung: &[sing::SungNote],
        args: &SingArgs,
        offset: f64,
    ) -> Result<Self> {
        use synthie::presets::sid::Preset as Patch;
        let wanted = args.band_patch.to_lowercase().replace(['-', '_'], " ");
        let patch = Patch::ALL
            .iter()
            .find(|p| p.name().to_lowercase() == wanted)
            .with_context(|| {
                let names: Vec<_> = Patch::ALL.iter().map(|p| p.name().to_lowercase()).collect();
                format!(
                    "unknown band patch {:?}; try one of {}",
                    args.band_patch,
                    names.join(", ")
                )
            })?;
        let end = args
            .until
            .or_else(|| sung.last().map(|n| n.start + n.duration + 1.5))
            .unwrap_or(f64::INFINITY);
        let tempo = f64::from(args.tempo.clamp(0.1, 10.0));
        let is_sung = |n: &midi::Note| {
            sung.iter()
                .any(|s| (s.start - n.start).abs() < 1e-3 && (s.key - f32::from(n.key)).abs() < 0.5)
        };
        let notes = track_notes
            .iter()
            .filter(|n| n.start + 1e-6 >= args.from && n.start < end && !is_sung(n))
            .map(|n| {
                let key = (f32::from(n.key) + args.transpose)
                    .round()
                    .clamp(0.0, 127.0) as u8;
                (n.start / tempo + offset, n.duration / tempo, key)
            })
            .collect();
        Ok(Self {
            notes,
            patch: patch.params(),
            level: args.band.clamp(0.0, 1.0),
        })
    }

    /// End of the last note, in render seconds.
    fn end(&self) -> f64 {
        self.notes.iter().map(|n| n.0 + n.1).fold(0.0, f64::max)
    }

    /// Render the band to `out` (mono) at `rate`.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn render(&self, rate: u32, out: &mut [f32]) {
        use synthie::prelude::{AudioEvent, ChannelNo, MidiNote, NUM_CHANNELS, SynthProcessor};
        const BLOCK: usize = 32;
        let frame = |t: f64| (t.max(0.0) * f64::from(rate)).round() as usize;

        // Spread the notes over synthie's channels, least busy first, so
        // chords don't steal each other's voices.
        let mut busy: Vec<Vec<f64>> = vec![Vec::new(); NUM_CHANNELS];
        let mut events: Vec<(usize, u8, AudioEvent)> = Vec::new();
        for c in 0..NUM_CHANNELS {
            let ch = ChannelNo(c as u8);
            events.push((
                0,
                0,
                AudioEvent::LoadPatchChannel(ch, Box::new(self.patch.clone())),
            ));
        }
        let mut sorted = self.notes.clone();
        sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (start, duration, key) in sorted {
            for ends in &mut busy {
                ends.retain(|&e| e > start);
            }
            let c = (0..NUM_CHANNELS)
                .min_by_key(|&c| busy[c].len())
                .unwrap_or(0);
            busy[c].push(start + duration);
            let ch = ChannelNo(c as u8);
            events.push((
                frame(start),
                2,
                AudioEvent::NoteOnChannel(ch, MidiNote(key)),
            ));
            events.push((
                frame(start + duration),
                1,
                AudioEvent::NoteOffChannel(ch, MidiNote(key)),
            ));
        }
        events.sort_by_key(|e| (e.0, e.1));

        let mut synth = SynthProcessor::<NUM_CHANNELS>::new(rate as f32);
        let mut next = 0;
        let mut due = Vec::new();
        for (b, block) in out.chunks_mut(BLOCK).enumerate() {
            let end = (b + 1) * BLOCK;
            due.clear();
            while next < events.len() && events[next].0 < end {
                due.push(events[next].2.clone());
                next += 1;
            }
            synth.process_block(&due, block, 1);
        }
    }
}

/// The bottom of the voice's register, from the first lyrics line: its
/// lowest note minus a fifth, so later phrases may go a little lower. Notes
/// below it are the accompaniment's top line where the singer rests, and go
/// to the band.
fn voice_floor(
    track: &midi::Track,
    lyrics: &str,
    in_range: &dyn Fn(&sing::SungNote) -> bool,
    gap: f64,
) -> Option<u8> {
    let first_line = lyrics.lines().find(|l| !l.trim().is_empty())?;
    let mut notes = sing::melody(track);
    notes.retain(|n| in_range(n));
    let lowest = sing::align_lines(&notes, first_line, gap)
        .iter()
        .map(|n| n.key)
        .fold(f32::INFINITY, f32::min);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let floor = lowest
        .is_finite()
        .then(|| (lowest - 7.0).clamp(0.0, 127.0) as u8);
    if let Some(k) = floor {
        println!("voice register from key {k} up (set --min-key to change)");
    }
    floor
}

/// Write the WAV file and/or sing aloud, as asked.
#[allow(clippy::cast_precision_loss)]
fn deliver(args: &SingArgs, song: &Song, count: usize, index: usize) -> Result<()> {
    if let Some(path) = &args.output {
        let out = render(song, args.rate, args.room)?;
        write_wav(path, args.rate, &out)?;
        println!(
            "sang {} notes from track {index} ({:.1} s) into {}",
            count,
            out.len() as f32 / args.rate as f32,
            path.display()
        );
    }
    if args.play || args.output.is_none() {
        let player = Player::open()?;
        let out = render(song, player.rate, args.room)?;
        println!(
            "singing {} notes from track {index} ({:.1} s)…",
            count,
            out.len() as f32 / player.rate as f32
        );
        player.play(out)?;
    }
    Ok(())
}

/// The default audio output, for singing aloud.
struct Player {
    device: cpal::Device,
    config: cpal::SupportedStreamConfig,
    rate: u32,
}

impl Player {
    fn open() -> Result<Self> {
        use cpal::traits::{DeviceTrait, HostTrait};
        let device = cpal::default_host()
            .default_output_device()
            .context("no audio output device; write a WAV file with -o instead")?;
        let config = device
            .default_output_config()
            .context("could not query the audio output")?;
        let rate = config.sample_rate();
        Ok(Self {
            device,
            config,
            rate,
        })
    }

    /// Play `samples` (mono) to the end, then return.
    fn play(self, samples: Vec<f32>) -> Result<()> {
        use cpal::traits::{DeviceTrait, StreamTrait};
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let channels = usize::from(self.config.channels());
        let total = samples.len();
        let position = Arc::new(AtomicUsize::new(0));
        let cursor = Arc::clone(&position);
        let stream = self
            .device
            .build_output_stream(
                cpal::StreamConfig {
                    channels: self.config.channels(),
                    sample_rate: self.config.sample_rate(),
                    buffer_size: cpal::BufferSize::Default,
                },
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let mut i = cursor.load(Ordering::Relaxed);
                    for frame in data.chunks_mut(channels) {
                        frame.fill(samples.get(i).copied().unwrap_or(0.0));
                        i += 1;
                    }
                    cursor.store(i, Ordering::Relaxed);
                },
                |err| eprintln!("audio error: {err}"),
                None,
            )
            .context("could not open the audio output")?;
        stream.play().context("could not start the audio output")?;
        while position.load(Ordering::Relaxed) < total {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        // Let the device drain its last buffer.
        std::thread::sleep(std::time::Duration::from_millis(250));
        Ok(())
    }
}

const NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn note_name(key: f32) -> String {
    let k = key.round() as i32;
    format!(
        "{}{}",
        NAMES[k.rem_euclid(12) as usize],
        k.div_euclid(12) - 1
    )
}

/// The melody as a numbered list: time, length, pitch, and the silence
/// before each note (phrases start after the longer ones).
fn print_notes(notes: &[sing::SungNote]) {
    println!("    #    start  length  key   gap");
    let mut prev_end: Option<f64> = None;
    for (i, n) in notes.iter().enumerate() {
        let gap = prev_end.map_or(String::new(), |e| {
            let g = n.start - e;
            if g > 0.005 {
                format!("{g:5.2}s")
            } else {
                String::new()
            }
        });
        println!(
            "{i:>5}  {:>6.2}s  {:>5.2}s  {:<4}  {gap}",
            n.start,
            n.duration,
            note_name(n.key)
        );
        prev_end = Some(n.start + n.duration);
    }
}

/// Write the sung notes, starting at zero, as a small MIDI file with lyrics.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn export(path: &PathBuf, notes: &[sing::SungNote]) -> Result<()> {
    let t0 = notes.first().map_or(0.0, |n| n.start);
    let rows: Vec<(f64, f64, u8, Option<&str>)> = notes
        .iter()
        .map(|n| {
            (
                n.start - t0,
                n.duration,
                n.key.round().clamp(0.0, 127.0) as u8,
                n.syllable.as_deref(),
            )
        })
        .collect();
    std::fs::write(path, midi::write(&rows))
        .with_context(|| format!("writing {}", path.display()))?;
    println!("wrote {} sung notes to {}", rows.len(), path.display());
    Ok(())
}

/// Sample rates accepted for rendering, in Hz.
const RATES: std::ops::RangeInclusive<u32> = 8_000..=192_000;

fn check_rate(rate: u32) -> Result<()> {
    if !RATES.contains(&rate) {
        bail!(
            "sample rate {rate} Hz is out of range ({} to {} Hz)",
            RATES.start(),
            RATES.end()
        );
    }
    Ok(())
}

/// Longest render accepted, in seconds.
const MAX_SECONDS: f64 = 3600.0;

/// Sing `utterance` through a short room, normalised to -1 dBFS.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn render(song: &Song, rate: u32, room: f32) -> Result<Vec<f32>> {
    // Check the length before allocating: a hostile or broken MIDI file
    // could otherwise ask for an enormous buffer.
    let band_end = song.band.as_ref().map_or(0.0, Band::end);
    let seconds = f64::from(song.utterance.duration()).max(band_end) + 0.6;
    if !seconds.is_finite() || seconds > MAX_SECONDS {
        bail!(
            "the song would be {seconds:.0} s long; the limit is {MAX_SECONDS:.0} s \
             (narrow it with --from/--until)"
        );
    }
    // The rate comes from --rate or the device; bound it too, so the
    // length cap above really bounds the buffer.
    check_rate(rate)?;
    let samples = (seconds * f64::from(rate)).ceil() as usize;
    let sr = rate as f32;
    let mut voice = Trombone::new(sr);
    Speaker::prepare(&mut voice);
    song.settings.apply(&mut voice);
    let mut out = vec![0.0f32; samples];
    let mut speaker = Speaker::new();
    speaker.speak(song.utterance.clone());
    for s in &mut out {
        *s = speaker.process(&mut voice);
    }
    // Level voice and band separately, so --band sets a predictable balance.
    normalize(&mut out, 1.0);
    if let Some(band) = &song.band {
        let mut accompaniment = vec![0.0f32; samples];
        band.render(rate, &mut accompaniment);
        normalize(&mut accompaniment, band.level);
        for (s, a) in out.iter_mut().zip(&accompaniment) {
            *s += a;
        }
    }
    let mut reverb = Reverb::new();
    reverb.set_params(0.35, 0.3);
    for s in &mut out {
        *s = reverb.process(*s, room.clamp(0.0, 1.0));
    }
    normalize(&mut out, 0.89);
    Ok(out)
}

/// 16-bit PCM mono WAV.
#[allow(clippy::cast_possible_truncation)]
fn write_wav(path: &PathBuf, sample_rate: u32, samples: &[f32]) -> Result<()> {
    let data_len = u32::try_from(samples.len() * 2).context("output too long for WAV")?;
    let mut f = std::io::BufWriter::new(
        std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?,
    );
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&sample_rate.to_le_bytes())?;
    f.write_all(&(sample_rate * 2).to_le_bytes())?;
    f.write_all(&2u16.to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    for s in samples {
        f.write_all(&((s.clamp(-1.0, 1.0) * 32_767.0) as i16).to_le_bytes())?;
    }
    f.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_rates_are_bounded() {
        assert!(check_rate(48_000).is_ok());
        assert!(check_rate(0).is_err());
        assert!(check_rate(4_000_000_000).is_err());
    }
}
