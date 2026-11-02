//! Speak text into `say.wav`.
//!
//! ```sh
//! cargo run -p synthie-trombone --example say --release -- "Hello, pink trombone!"
//! cargo run -p synthie-trombone --example say --release -- --voice critter "Are you there?"
//! cargo run -p synthie-trombone --example say --release -- --pitch 180 --rate 0.8 "Hello?"
//! ```

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

#[path = "support/wav.rs"]
mod wav;

use synthie_trombone::{Preset, Talker, normalize};

fn main() -> std::io::Result<()> {
    const SR: u32 = 48_000;
    let mut settings = Preset::Man.settings();
    let mut text = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = |name: &str| {
            args.next()
                .unwrap_or_else(|| panic!("{name} needs a value"))
        };
        match a.as_str() {
            "--voice" => {
                let v = value("--voice");
                let preset = Preset::from_name(&v).unwrap_or_else(|| {
                    let names: Vec<_> = Preset::ALL.iter().map(|p| p.name()).collect();
                    panic!("unknown voice {v:?}; try one of {}", names.join(", "))
                });
                settings = preset.settings();
            }
            "--pitch" => settings.pitch = value("--pitch").parse().expect("--pitch needs Hz"),
            "--rate" => settings.rate = value("--rate").parse().expect("--rate needs a number"),
            "--expr" => {
                settings.expressiveness = value("--expr").parse().expect("--expr needs a number");
            }
            _ => text.push(a),
        }
    }
    let text = if text.is_empty() {
        "Hello! I am the pink trombone. Can you understand me?".to_string()
    } else {
        text.join(" ")
    };

    let mut talker = Talker::new(SR as f32, settings);
    let utterance = talker.compile(&text);
    for s in utterance.segments() {
        print!("{} ", s.phoneme);
    }
    println!();
    let mut out = talker.bake_utterance(utterance);
    normalize(&mut out, 0.89);

    wav::write("say.wav", SR, &out)?;
    println!("wrote say.wav ({:.1} s)", out.len() as f32 / SR as f32);
    Ok(())
}
