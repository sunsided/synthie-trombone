//! Grid-search tongue position, height and lip rounding for each vowel
//! against Peterson & Barney (1952) adult male formant averages, plus /o/
//! from Hillenbrand et al. (1995), which Peterson & Barney did not record.
//!
//! ```sh
//! cargo run -p synthie-trombone --example tune_vowels --release            # all
//! cargo run -p synthie-trombone --example tune_vowels --release -- OH AA   # some
//! ```

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

#[path = "support/lpc.rs"]
mod lpc;

use synthie_trombone::{Constriction, SLOT_LIPS, Trombone};

const SR: f32 = 16_000.0;

/// (name, F1, F2) targets.
const TARGETS: &[(&str, f32, f32)] = &[
    ("IY", 270.0, 2290.0),
    ("IH", 390.0, 1990.0),
    ("EH", 530.0, 1840.0),
    ("AE", 660.0, 1720.0),
    ("AH", 640.0, 1190.0),
    ("AA", 730.0, 1090.0),
    ("AO", 570.0, 840.0),
    ("UH", 440.0, 1020.0),
    ("UW", 300.0, 870.0),
    // Hillenbrand et al. (1995), men: the /o/ that starts "go".
    ("OH", 497.0, 910.0),
];

fn measure(index: f32, diameter: f32, lips: Option<f32>) -> Option<(f32, f32)> {
    let mut t = Trombone::new(SR);
    t.set_frequency(100.0);
    t.set_vibrato(0.0, 0.0);
    t.set_tongue(index, diameter);
    t.set_constriction(SLOT_LIPS, lips.map(|d| Constriction::new(41.0, d)));
    t.note_on();
    let mut buf = vec![0.0; (SR * 0.5) as usize];
    t.render(&mut buf);
    let mut x = vec![0.0; (SR * 0.3) as usize];
    t.render(&mut x);
    let f = lpc::formants(&x, SR, 4000.0);
    Some((*f.first()?, *f.get(1)?))
}

fn main() {
    let lips = [None, Some(1.3), Some(1.1), Some(0.95), Some(0.8)];
    let only: Vec<String> = std::env::args().skip(1).map(|a| a.to_uppercase()).collect();
    for &(name, t1, t2) in TARGETS {
        if !only.is_empty() && !only.iter().any(|o| o == name) {
            continue;
        }
        let mut best = (f32::INFINITY, 0.0, 0.0, None, 0.0, 0.0);
        for i in 0..=34 {
            let index = 12.0 + i as f32 * 0.5;
            for d in 0..=29 {
                let diameter = 2.05 + d as f32 * 0.05;
                for &l in &lips {
                    let Some((f1, f2)) = measure(index, diameter, l) else {
                        continue;
                    };
                    let e = (f1 / t1).ln().powi(2) + (f2 / t2).ln().powi(2);
                    if e < best.0 {
                        best = (e, index, diameter, l, f1, f2);
                    }
                }
            }
        }
        let (e, index, diameter, l, f1, f2) = best;
        println!(
            "{name}: tongue ({index:.1}, {diameter:.2}) lips {l:?} -> {f1:.0}/{f2:.0} (target {t1:.0}/{t2:.0}, err {e:.4})"
        );
    }
}
