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
    /// Output WAV file.
    #[arg(short, long, default_value = "sing.wav")]
    pub output: PathBuf,
    /// Sample rate of the WAV file.
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

    let melody_options = sing::MelodyOptions {
        min_key: args.min_key,
    };
    let mut notes = sing::melody_with(&file.tracks[index], &melody_options);
    notes.retain(|n| n.start + 1e-6 >= args.from && args.until.is_none_or(|u| n.start < u));
    if args.notes {
        print_notes(&notes);
        return Ok(());
    }
    if let Some(path) = &args.lyrics {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        notes = sing::align_lines(&notes, &text, args.gap);
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
    let utterance = sing::compile(&notes, &settings, &options);
    let out = render(utterance, &settings, args.rate, args.room)?;
    write_wav(&args.output, args.rate, &out)?;
    println!(
        "sang {} notes from track {index} ({:.1} s) into {}",
        notes.len(),
        out.len() as f32 / args.rate as f32,
        args.output.display()
    );
    Ok(())
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

/// Longest render accepted, in seconds.
const MAX_SECONDS: f64 = 3600.0;

/// Sing `utterance` through a short room, normalised to -1 dBFS.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn render(
    utterance: Utterance,
    settings: &VoiceSettings,
    rate: u32,
    room: f32,
) -> Result<Vec<f32>> {
    // Check the length before allocating: a hostile or broken MIDI file
    // could otherwise ask for an enormous buffer.
    let seconds = f64::from(utterance.duration()) + 0.6;
    if !seconds.is_finite() || seconds > MAX_SECONDS {
        bail!(
            "the song would be {seconds:.0} s long; the limit is {MAX_SECONDS:.0} s \
             (narrow it with --from/--until)"
        );
    }
    let samples = (seconds * f64::from(rate)).ceil() as usize;
    let sr = rate as f32;
    let mut voice = Trombone::new(sr);
    Speaker::prepare(&mut voice);
    settings.apply(&mut voice);
    let mut out = vec![0.0f32; samples];
    let mut speaker = Speaker::new();
    speaker.speak(utterance);
    let mut reverb = Reverb::new();
    reverb.set_params(0.35, 0.3);
    for s in &mut out {
        *s = reverb.process(speaker.process(&mut voice), room.clamp(0.0, 1.0));
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
