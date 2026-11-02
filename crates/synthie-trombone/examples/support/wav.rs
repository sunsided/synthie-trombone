#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
//! Minimal 16-bit mono WAV writer shared by the examples.

use std::io::{self, Write};

/// Write `samples` as a 16-bit PCM mono WAV file.
#[allow(clippy::cast_possible_truncation)]
pub fn write(path: &str, sample_rate: u32, samples: &[f32]) -> io::Result<()> {
    let data_len = u32::try_from(samples.len() * 2).expect("file too long");
    let mut f = io::BufWriter::new(std::fs::File::create(path)?);
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?; // PCM
    f.write_all(&1u16.to_le_bytes())?; // mono
    f.write_all(&sample_rate.to_le_bytes())?;
    f.write_all(&(sample_rate * 2).to_le_bytes())?;
    f.write_all(&2u16.to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    for s in samples {
        f.write_all(&((s.clamp(-1.0, 1.0) * 32_767.0) as i16).to_le_bytes())?;
    }
    f.flush()
}
