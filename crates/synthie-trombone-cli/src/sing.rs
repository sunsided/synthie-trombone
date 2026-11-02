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

    let mut notes = sing::melody(&file.tracks[index]);
    if let Some(path) = &args.lyrics {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let first = notes
            .iter()
            .position(|n| n.start + 1e-6 >= args.from)
            .unwrap_or(notes.len());
        let syllables = sing::split_syllables(&text);
        let mut tail = sing::with_syllables(notes.split_off(first), &syllables);
        for n in &mut notes {
            n.syllable = None;
        }
        notes.append(&mut tail);
    }
    notes.retain(|n| n.start + 1e-6 >= args.from && args.until.is_none_or(|u| n.start < u));
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
    let utterance = sing::compile(&notes, &settings, &options);
    let out = render(utterance, &settings, args.rate, args.room);
    write_wav(&args.output, args.rate, &out)?;
    println!(
        "sang {} notes from track {index} ({:.1} s) into {}",
        notes.len(),
        out.len() as f32 / args.rate as f32,
        args.output.display()
    );
    Ok(())
}

/// Sing `utterance` through a short room, normalised to -1 dBFS.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn render(utterance: Utterance, settings: &VoiceSettings, rate: u32, room: f32) -> Vec<f32> {
    let sr = rate as f32;
    let mut voice = Trombone::new(sr);
    Speaker::prepare(&mut voice);
    settings.apply(&mut voice);
    let mut out = vec![0.0f32; ((utterance.duration() + 0.6) * sr) as usize];
    let mut speaker = Speaker::new();
    speaker.speak(utterance);
    let mut reverb = Reverb::new();
    reverb.set_params(0.35, 0.3);
    for s in &mut out {
        *s = reverb.process(speaker.process(&mut voice), room.clamp(0.0, 1.0));
    }
    normalize(&mut out, 0.89);
    out
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
