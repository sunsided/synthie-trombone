//! Measure F1/F2/F3 of every vowel, for tuning the phoneme table.
//!
//! ```sh
//! cargo run -p synthie-trombone --example formants --release
//! ```

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

#[path = "support/lpc.rs"]
mod lpc;

use synthie_trombone::{Manner, Phoneme, Trombone};

const SR: f32 = 16_000.0;

fn main() {
    println!("       F1    F2    F3");
    for &p in Phoneme::ALL {
        if !matches!(p.manner(), Manner::Vowel) && p != Phoneme::W && p != Phoneme::Y {
            continue;
        }
        let mut t = Trombone::new(SR);
        t.set_frequency(100.0);
        t.set_vibrato(0.0, 0.0);
        t.articulate_phoneme(p);
        t.note_on();
        let mut buf = vec![0.0; (SR * 0.6) as usize];
        t.render(&mut buf);
        let mut x = vec![0.0; (SR * 0.5) as usize];
        t.render(&mut x);
        let f = lpc::formants(&x, SR, 4000.0);
        print!("{:<5}", p.symbol());
        for f in f.iter().take(3) {
            print!(" {f:5.0}");
        }
        println!();
    }
}
