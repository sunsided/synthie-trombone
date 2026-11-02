//! Speak one line in every preset voice into `voices/<preset>.wav`.
//!
//! ```sh
//! cargo run -p synthie-trombone --example voices --release -- "Welcome, traveller!"
//! ```

#![allow(clippy::cast_precision_loss)]

#[path = "support/wav.rs"]
mod wav;

use synthie_trombone::{Preset, Talker, normalize};

fn main() -> std::io::Result<()> {
    const SR: u32 = 44_100;
    let text = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    let text = if text.is_empty() {
        "Welcome, traveller! Have you come to buy, or to sell?".to_string()
    } else {
        text
    };
    std::fs::create_dir_all("voices")?;
    for preset in Preset::ALL {
        let mut talker = Talker::new(SR as f32, preset);
        let mut line = talker.bake(&text);
        normalize(&mut line, 0.9);
        let path = format!("voices/{preset}.wav");
        wav::write(&path, SR, &line)?;
        println!("{path:<20} {:.1} s", line.len() as f32 / SR as f32);
    }
    Ok(())
}
