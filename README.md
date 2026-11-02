# synthie-trombone

[Pink Trombone](https://dood.al/pinktrombone/) is Neil Thapen's 2017 web toy
that lets you talk by dragging a finger across a diagram of a mouth: a
glottal source pushes air through a 44-cell waveguide model of the vocal
tract, you squeeze the tract somewhere, and out comes something between a
vowel and a very confused trombone. I had been building
[synthie](https://github.com/sunsided/synth), a chiptune synthesizer in
Rust, and at some point the question came up whether the trombone could live
there too.

It turns out somebody already did the port: the
[`pink-trombone`](https://crates.io/crates/pink-trombone) crate translates
[chdh's TypeScript version](https://github.com/chdh/pink-trombone-mod) to
Rust, more or less line by line. I did my own anyway, partly because I wanted
it inside an audio callback (the port is a faithful translation, but it
allocates in the audio path, and a real-time callback is the one place that
can't have that), and partly because once the tract was running I
wanted it to sing notes, speak typed text, and sound like more than one
person. So that's what this is: the original model, a fixed 48 kHz core
that sounds the same at any output rate, an English text-to-speech front
end, a handful of voices from a grown man to a squeaky critter, and a
terminal app to play with all of it.

The vowels were the part I enjoyed most. My first formant probe was a plain
DFT peak search, which happily reported 210 Hz for every vowel I threw at it
(the glottal source tilts the spectrum so hard that the low harmonics win
every time). Switching to LPC gave sensible numbers, and from there a grid
search over tongue position, tongue height and lip rounding fitted each vowel
to Peterson & Barney's 1952 averages for adult male speakers; all nine land
within about 2 % on F1 and F2. The search runs for about four minutes
(`task tune:vowels`).

![Singing an "ah" with the critter voice, a finger on the tongue tip](docs/singing.png)

![The woman voice mid-sentence, about to close the lips for the "m" in "am"](docs/speaking.png)

## What's in the box

| Crate | What |
|---|---|
| [`synthie-trombone`](crates/synthie-trombone/) | The voice. Real-time safe (no allocation in the audio path), sample-rate independent, `no_std` capable, and builds for `wasm32-unknown-unknown`. Phonemes, voice presets, text-to-speech, and `Talker` for "tune once, then send sentences". |
| [`synthie-trombone-cli`](crates/synthie-trombone-cli/) | `synthie-trombone`, the terminal app: play the tract with keyboard and mouse, or type text and hear it spoken, through synthie's reverb and in synthie's neon look. |

## Getting started

The project uses [go-task](https://taskfile.dev/); `task` without arguments
lists everything.

```sh
task run                                   # the terminal app
task install                               # → ~/.cargo/bin/synthie-trombone
task say -- --voice woman "Hello there!"   # → say.wav
task voices -- "Welcome, traveller!"       # one line in every voice → voices/*.wav
task vowels                                # the vowel triangle, sung → vowels.wav
task formants                              # measured F1–F3 for every vowel
```

Without `task`, the equivalents are `cargo run --release -p
synthie-trombone-cli` and `cargo run --release -p synthie-trombone --example
say -- …`. Build in release mode: the waveguide runs 96,000 tract steps per
second, and a debug build doesn't keep up.

## Voices

A voice is more than its pitch. The vocal tract length sets every formant at
once (an adult woman's tract is about 14 % shorter than a man's, a child's
about 30 %), so a "woman" made by raising the pitch alone sounds like a man
singing falsetto. Each preset combines pitch, tract length, tenseness,
jitter, vibrato, nasality, speaking rate and how much the intonation moves:

| Preset | Character |
|---|---|
| `man` | 105 Hz, the model's native tract |
| `woman` | 210 Hz, tract 0.86, livelier intonation |
| `child` | 290 Hz, tract 0.72 |
| `elder` | breathy and slow, with a tremor and a shaky pitch |
| `giant` | 62 Hz, tract 1.4, slow |
| `critter` | 360 Hz, tract 0.6, fast, nasal and very excitable |
| `robot` | monotone, no jitter, pressed |
| `whisper` | breath only, no voicing |

Every field of `VoiceSettings` is public, so tuning your own means starting
from a preset and changing what you like; with the `serde` feature the
settings can live in a data file.

## Using it in a game

Most games want a voice tuned once and then sentences turned into sound
buffers. `Talker` does exactly that, and the result is plain mono `f32`
samples for whatever plays your audio:

```rust
use synthie_trombone::{Preset, Talker, VoiceSettings, normalize};

let mut shopkeeper = Talker::new(44_100.0, Preset::Elder);
let mut line = shopkeeper.bake("Welcome, traveller!");
normalize(&mut line, 0.9);
// e.g. with game-toolkit: audio.play_samples(&line, 44_100)?;

let goblin = VoiceSettings { nasality: 0.4, rate: 1.5, ..Preset::Critter.settings() };
let squeak = Talker::new(44_100.0, goblin).bake("Shiny! Mine!");
```

For live dialogue, `Talker::say` queues text and `Talker::render` fills
audio buffers as they come. The crate never opens an audio device itself, so
it doesn't fight kira (or anything else) for the output.

## The terminal app

| Key | Action |
|---|---|
| `a w s e d f t g y h u j k o l p ;` | sing, piano layout; `z` / `x` change the octave |
| `Space` or `Esc` | stop singing (and speaking) |
| `1` … `0` | vowels IY IH EH AE AA AH AO UH UW ER |
| arrow keys | move the tongue body |
| left-drag on the tract | constrict it, like a finger on the original's diagram |
| right-drag on the tract | move the tongue |
| `v` / `V` | next / previous voice preset |
| `n`, `b`, `i` | nasal (velum), lip pop, drone |
| `[` `]`, `-` `=` | tenseness, vibrato |
| `m`, `r`, `c` | pitch wobble, reverb amount, clear constrictions |
| `Tab` | type text; `Enter` speaks it at the current note's pitch |
| `,` `.` | speech rate |
| `q` | quit |

The screenshots above are rendered from the real UI and a real voice by
`task screenshots` (headless Chrome and ImageMagick turn the frames into
PNGs).

Notes stop when you let go of the key in terminals that report key releases
(kitty, WezTerm, foot, Ghostty). Elsewhere a note holds until `Space`.

## Text to speech

Text goes through letter-to-sound rules for English (digraphs, the silent
*e*, soft *c* and *g*, unstressed vowels) and a dictionary of about a
hundred common and irregular words; numbers are spelled out. Consonants take
the tongue position of the next vowel, stops get a closure, a burst and a
breath, and the pitch drifts down over a phrase, falls at `.` and rises at
`?`. Being rule-based, it will get some English spellings wrong; when it
does, write the phonemes yourself in ARPAbet:

```text
[HH AX L OW1], pink trombone!
```

## License

EUPL-1.2 (`LICENSE`), like synthie and game-toolkit. Derived from Pink
Trombone, © 2017 Neil Thapen, used under the MIT licence; its notice is in
`NOTICE-pink-trombone`.
