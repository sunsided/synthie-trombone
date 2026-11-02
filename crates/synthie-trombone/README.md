# synthie-trombone

A real-time [Pink Trombone](https://dood.al/pinktrombone/) vocal tract
synthesizer for Rust, made for [synthie](https://crates.io/crates/synthie):
play it like an instrument, shape vowels and consonants, or type text and
hear it spoken.

Pink Trombone (Neil Thapen, 2017) drives a 44-cell Kelly–Lochbaum waveguide
of the vocal tract, with a nasal branch, from a Liljencrants–Fant glottal
source, and adds turbulence at constrictions and bursts when closures
release. This crate keeps that model and adds:

- **Real-time safe:** fixed-size state; no allocation, locks or trait
  objects in the audio path; recovers from non-finite state on its own.
- **Sample-rate independent:** runs at a fixed 48 kHz internally and
  resamples, so formants don't shift at 44.1 or 96 kHz. Parameter blocks
  don't depend on the host buffer size either.
- **Built as an instrument:** note on/off, MIDI pitch, glide, voicing
  attack/release, up to four simultaneous constrictions, explicit velum.
- **Measured vowels:** ARPAbet phonemes. The vowels were grid-searched
  against Peterson & Barney formant data, and every vowel lands within
  about 2 % on F1/F2 (`examples/tune_vowels.rs`, `examples/formants.rs`).
- **Text to speech** (`alloc`): English spelling rules plus an exceptions
  dictionary, ARPAbet in `[brackets]`, numbers, stress, coarticulation,
  declining intonation, falls at `.` and rises at `?`.
- **`no_std`:** `default-features = false, features = ["libm"]` (add
  `alloc` for speech).

## Voices for games

Pick a voice once, then turn lines of dialogue into sample buffers for
whatever plays your audio:

```rust
use synthie_trombone::{Preset, Talker, VoiceSettings, normalize};

// Built-in voices: Deep, Bright, Child, Elder, Giant, Critter, Robot, Whisper.
let mut shopkeeper = Talker::new(44_100.0, Preset::Elder);
let mut line = shopkeeper.bake("Welcome, traveller!");
normalize(&mut line, 0.9); // mono f32 at 44.1 kHz, ready to play

// Or tune your own: a nasal, fast-talking goblin.
let goblin = VoiceSettings {
    nasality: 0.4,
    rate: 1.5,
    ..Preset::Critter.settings()
};
let mut goblin = Talker::new(44_100.0, goblin);
let squeak = goblin.bake("Shiny!");
# assert!(!squeak.is_empty());
```

Pitch and vocal tract length vary independently from person to person:
raising only the pitch sounds like the same speaker reaching higher, while a
shorter tract shifts every resonance and sounds like a smaller speaker.
Tenseness, jitter, vibrato, nasality, speaking rate and intonation range
complete the character. With the `serde` feature, `VoiceSettings` can live
in your game's data files.

## Instrument and streaming

```rust
use synthie_trombone::{Phoneme, Trombone};
use synthie_trombone::speech::{Prosody, Speaker, Utterance};

let mut voice = Trombone::new(48_000.0);

// Sing an "ah" at A3.
voice.articulate_phoneme(Phoneme::Aa);
voice.note_on_midi(57.0);
let mut buf = [0.0f32; 256];
voice.render(&mut buf);

// Speak. Compile off the audio thread; Speaker::process never allocates.
let utterance = Utterance::from_text("Hello, pink trombone!", &Prosody::default());
let mut speaker = Speaker::new();
Speaker::prepare(&mut voice);
speaker.speak(utterance);
for s in &mut buf {
    *s = speaker.process(&mut voice);
}
```

## Examples

```sh
cargo run -p synthie-trombone --example say --release -- --voice bright "Can you hear me?"  # → say.wav
cargo run -p synthie-trombone --example voices --release                       # → voices/*.wav
cargo run -p synthie-trombone --example vowels --release                       # → vowels.wav
cargo run -p synthie-trombone --example formants --release                     # F1–F3 table
```

## License

EUPL-1.2 (`LICENSE`). Derived from Pink Trombone, © 2017 Neil Thapen,
used under the MIT licence; its notice is in `NOTICE-pink-trombone`.
