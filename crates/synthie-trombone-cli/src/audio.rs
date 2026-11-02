//! Audio thread: one [`Trombone`], a [`Speaker`] for typed text, and
//! synthie's reverb. Talks to the UI over bounded channels; the callback
//! never allocates or blocks.

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, Sender, bounded};
use synthie::audio::fx::Reverb;
use synthie_trombone::speech::{Speaker, Utterance};
use synthie_trombone::{Constriction, MAX_CONSTRICTIONS, N, NOSE_LEN, Trombone, VoiceSettings};

/// Samples kept for the oscilloscope.
pub const SCOPE_LEN: usize = 1024;

/// UI → audio commands.
#[derive(Debug)]
pub enum Cmd {
    NoteOn(f32),
    NoteOff,
    Note(f32),
    Tongue(f32, f32),
    Constriction(usize, Option<Constriction>),
    Velum(f32),
    Tenseness(f32),
    Vibrato(f32),
    Wobble(bool),
    Drone(bool),
    Reverb(f32),
    Voice(VoiceSettings),
    Speak(Utterance),
    StopSpeaking,
}

/// Audio → UI state, sent about 60 times a second. `Copy`, so sending it
/// through a bounded channel never allocates.
#[derive(Clone, Copy, Debug)]
pub struct Snapshot {
    pub diameters: [f32; N],
    pub targets: [f32; N],
    pub amplitudes: [f32; N],
    pub nose: [f32; NOSE_LEN],
    pub nose_amplitudes: [f32; NOSE_LEN],
    pub constrictions: [Option<Constriction>; MAX_CONSTRICTIONS],
    pub tongue: (f32, f32),
    pub velum: f32,
    pub intensity: f32,
    pub frequency: f32,
    pub tenseness: f32,
    /// Segment index while speaking.
    pub speaking: Option<usize>,
    pub scope: [f32; SCOPE_LEN],
    pub level: f32,
}

impl Snapshot {
    pub(crate) fn capture(voice: &Trombone, speaker: &Speaker, scope: &Scope) -> Self {
        let mut constrictions = [None; MAX_CONSTRICTIONS];
        for (k, c) in constrictions.iter_mut().enumerate() {
            *c = voice.constriction(k);
        }
        Self {
            diameters: *voice.diameters(),
            targets: *voice.target_diameters(),
            amplitudes: *voice.amplitudes(),
            nose: *voice.nose_diameters(),
            nose_amplitudes: *voice.nose_amplitudes(),
            constrictions,
            tongue: voice.tongue(),
            velum: voice.velum(),
            intensity: voice.intensity(),
            frequency: voice.frequency(),
            tenseness: voice.tenseness(),
            speaking: speaker.current().map(|(i, _)| i),
            scope: scope.ordered(),
            level: scope.level,
        }
    }
}

impl Default for Snapshot {
    fn default() -> Self {
        Self::capture(&Trombone::new(48_000.0), &Speaker::new(), &Scope::default())
    }
}

pub(crate) struct Scope {
    buf: [f32; SCOPE_LEN],
    pos: usize,
    level: f32,
}

impl Default for Scope {
    fn default() -> Self {
        Self {
            buf: [0.0; SCOPE_LEN],
            pos: 0,
            level: 0.0,
        }
    }
}

impl Scope {
    pub(crate) fn push(&mut self, s: f32) {
        self.buf[self.pos] = s;
        self.pos = (self.pos + 1) % SCOPE_LEN;
        self.level = (self.level * 0.9995).max(s.abs());
    }

    fn ordered(&self) -> [f32; SCOPE_LEN] {
        let mut out = [0.0; SCOPE_LEN];
        let (a, b) = self.buf.split_at(self.pos);
        out[..b.len()].copy_from_slice(b);
        out[b.len()..].copy_from_slice(a);
        out
    }
}

/// Instrument settings to restore after speaking.
struct Instrument {
    tenseness: f32,
    velum: f32,
    wobble: bool,
}

struct Engine {
    voice: Trombone,
    speaker: Speaker,
    reverb: Reverb,
    reverb_mix: f32,
    instrument: Instrument,
    was_speaking: bool,
    scope: Scope,
    snapshot_every: usize,
    since_snapshot: usize,
    cmd_rx: Receiver<Cmd>,
    snap_tx: Sender<Snapshot>,
    trash_tx: Sender<Utterance>,
}

impl Engine {
    fn apply(&mut self, cmd: Cmd) {
        let v = &mut self.voice;
        match cmd {
            Cmd::NoteOn(note) => {
                self.stop_speaking();
                let v = &mut self.voice;
                v.set_note(note);
                v.note_on();
            }
            Cmd::NoteOff => v.note_off(),
            Cmd::Note(note) => v.set_note(note),
            Cmd::Tongue(i, d) => v.set_tongue(i, d),
            Cmd::Constriction(slot, c) => v.set_constriction(slot, c),
            Cmd::Velum(x) => {
                self.instrument.velum = x;
                v.set_velum(x);
            }
            Cmd::Tenseness(t) => {
                self.instrument.tenseness = t;
                if !self.speaker.is_speaking() {
                    v.set_tenseness(t);
                }
            }
            Cmd::Vibrato(a) => v.set_vibrato(a, 5.5),
            Cmd::Wobble(w) => {
                self.instrument.wobble = w;
                v.set_wobble(w);
            }
            Cmd::Drone(on) => v.set_always_voice(on),
            Cmd::Reverb(mix) => self.reverb_mix = mix,
            Cmd::Voice(settings) => {
                settings.apply(v);
                self.instrument.tenseness = settings.tenseness;
            }
            Cmd::Speak(u) => {
                Speaker::prepare(&mut self.voice);
                let old = self.speaker.speak(u);
                self.was_speaking = true;
                // Hand the old track back so it is freed off this thread.
                let _ = self.trash_tx.try_send(old);
            }
            Cmd::StopSpeaking => self.stop_speaking(),
        }
    }

    fn stop_speaking(&mut self) {
        if self.speaker.is_speaking() {
            let old = self.speaker.stop(&mut self.voice);
            let _ = self.trash_tx.try_send(old);
        }
    }

    fn restore_instrument(&mut self) {
        let v = &mut self.voice;
        v.reset_voicing_rates();
        v.set_articulation_speed(1.0);
        v.set_glide(0.1);
        v.set_wobble(self.instrument.wobble);
        v.set_tenseness(self.instrument.tenseness);
        v.set_velum(self.instrument.velum);
    }

    fn render(&mut self, data: &mut [f32], channels: usize) {
        while let Ok(cmd) = self.cmd_rx.try_recv() {
            self.apply(cmd);
        }
        for frame in data.chunks_mut(channels) {
            let dry = self.speaker.process(&mut self.voice);
            if self.was_speaking && !self.speaker.is_speaking() {
                self.was_speaking = false;
                self.restore_instrument();
            }
            let s = (self.reverb.process(dry, self.reverb_mix) * 0.9).clamp(-1.0, 1.0);
            frame.fill(s);
            self.scope.push(s);

            self.since_snapshot += 1;
            if self.since_snapshot >= self.snapshot_every {
                self.since_snapshot = 0;
                let snap = Snapshot::capture(&self.voice, &self.speaker, &self.scope);
                let _ = self.snap_tx.try_send(snap);
            }
        }
    }
}

/// Channels the UI keeps.
pub struct Link {
    pub cmd: Sender<Cmd>,
    pub snapshots: Receiver<Snapshot>,
    pub trash: Receiver<Utterance>,
    pub sample_rate: f32,
}

/// Open the default output device and start the engine.
pub fn start() -> Result<(cpal::Stream, Link)> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .context("no default audio output device")?;
    let config = device
        .default_output_config()
        .context("failed to query default output config")?;
    #[allow(clippy::cast_precision_loss)]
    let sample_rate = config.sample_rate() as f32;
    let channels = usize::from(config.channels());
    let stream_config = cpal::StreamConfig {
        channels: config.channels(),
        sample_rate: config.sample_rate(),
        buffer_size: cpal::BufferSize::Default,
    };

    let (cmd_tx, cmd_rx) = bounded(256);
    let (snap_tx, snap_rx) = bounded(4);
    let (trash_tx, trash_rx) = bounded(16);

    let mut reverb = Reverb::new();
    reverb.set_params(0.55, 0.45);
    let mut voice = Trombone::new(sample_rate);
    voice.set_frequency(110.0);
    voice.set_tenseness(0.6);

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let snapshot_every = (sample_rate / 60.0) as usize;
    let mut engine = Engine {
        voice,
        speaker: Speaker::new(),
        reverb,
        reverb_mix: 0.18,
        instrument: Instrument {
            tenseness: 0.6,
            velum: 0.0,
            wobble: false,
        },
        was_speaking: false,
        scope: Scope::default(),
        snapshot_every,
        since_snapshot: 0,
        cmd_rx,
        snap_tx,
        trash_tx,
    };

    let stream = device
        .build_output_stream(
            stream_config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| engine.render(data, channels),
            |_err| {},
            None,
        )
        .context("failed to build output stream")?;
    stream.play().context("failed to start audio stream")?;

    Ok((
        stream,
        Link {
            cmd: cmd_tx,
            snapshots: snap_rx,
            trash: trash_rx,
            sample_rate,
        },
    ))
}
