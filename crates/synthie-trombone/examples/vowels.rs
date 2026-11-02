//! Sing the vowel triangle and write `vowels.wav`.
//!
//! ```sh
//! cargo run -p synthie-trombone --example vowels --release
//! ```

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

#[path = "support/wav.rs"]
mod wav;

use synthie_trombone::{Phoneme, Trombone};

fn main() -> std::io::Result<()> {
    const SR: u32 = 48_000;
    let mut voice = Trombone::new(SR as f32);
    voice.set_vibrato(0.008, 5.5);
    voice.note_on_midi(52.0);

    let melody = [
        (Phoneme::Iy, 52.0),
        (Phoneme::Eh, 55.0),
        (Phoneme::Ae, 57.0),
        (Phoneme::Aa, 59.0),
        (Phoneme::Ao, 57.0),
        (Phoneme::Uh, 55.0),
        (Phoneme::Uw, 52.0),
    ];
    let mut out = Vec::new();
    let mut buf = vec![0.0f32; SR as usize * 7 / 10];
    for (vowel, note) in melody {
        println!("{vowel}");
        voice.articulate_phoneme(vowel);
        voice.set_note(note);
        voice.render(&mut buf);
        out.extend_from_slice(&buf);
    }
    voice.note_off();
    voice.render(&mut buf);
    out.extend_from_slice(&buf);

    wav::write("vowels.wav", SR, &out)?;
    println!("wrote vowels.wav ({:.1} s)", out.len() as f32 / SR as f32);
    Ok(())
}
