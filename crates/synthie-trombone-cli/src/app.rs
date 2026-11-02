//! UI state and input handling.

use crate::audio::{Cmd, Link, Snapshot};
use crossbeam_channel::Sender;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use std::time::{Duration, Instant};
use synthie_trombone::speech::{Prosody, Utterance};
use synthie_trombone::{
    Constriction, N, Phoneme, Preset, TONGUE_DIAMETER, TONGUE_INDEX, VoiceSettings,
};
use synthie_ui::{Key, KeyCode, KeyKind};

/// Constriction slot for mouse dragging (0 and 1 belong to the speaker).
const SLOT_MOUSE: usize = 2;
/// Constriction slot for the lip-pop key.
const SLOT_POP: usize = 3;
const NO_RELEASE_HINT: &str =
    "terminal sends no key releases: notes hold, Space stops · Tab to type text";
/// Piano keys, one semitone apart, starting at C.
const PIANO: &str = "awsedftgyhujkolp;";
/// Canvas y range of the tract view (shared with `ui`).
pub const Y_BOUNDS: [f64; 2] = [-2.6, 5.2];
/// Canvas x range of the tract view.
pub const X_BOUNDS: [f64; 2] = [-1.0, 45.0];

/// Number keys → vowels.
pub const VOWEL_KEYS: [(char, Phoneme); 10] = [
    ('1', Phoneme::Iy),
    ('2', Phoneme::Ih),
    ('3', Phoneme::Eh),
    ('4', Phoneme::Ae),
    ('5', Phoneme::Aa),
    ('6', Phoneme::Ah),
    ('7', Phoneme::Ao),
    ('8', Phoneme::Uh),
    ('9', Phoneme::Uw),
    ('0', Phoneme::Er),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Play,
    Type,
}

#[allow(clippy::struct_excessive_bools)] // independent toggles
pub struct App {
    cmd: Sender<Cmd>,
    link: Link,
    pub snap: Snapshot,
    pub mode: Mode,
    pub keyboard_release: bool,

    pub octave: i32,
    pub note: i32,
    held: Option<char>,
    pub voiced: bool,

    pub tongue: (f32, f32),
    pub tenseness: f32,
    pub vibrato: f32,
    pub wobble: bool,
    pub nasal: bool,
    pub drone: bool,
    pub reverb: f32,
    pub rate: f32,
    pub preset: Preset,
    pub settings: VoiceSettings,

    pub text: String,
    history: Vec<String>,
    history_pos: usize,
    /// Phoneme symbols of the last utterance with their segment ranges.
    pub spoken: Vec<(&'static str, std::ops::Range<usize>)>,
    pub last_vowel: Option<Phoneme>,

    /// Inner area of the tract canvas, for mouse mapping.
    pub tract_area: Rect,
    mouse: Option<MouseButton>,
    pop_until: Option<Instant>,
    pub status: String,
}

impl App {
    pub fn new(link: Link, keyboard_release: bool) -> Self {
        let mut app = Self {
            cmd: link.cmd.clone(),
            link,
            snap: Snapshot::default(),
            mode: Mode::Play,
            keyboard_release,
            octave: 3,
            note: 45,
            held: None,
            voiced: false,
            tongue: (12.9, 2.43),
            tenseness: 0.6,
            vibrato: 0.005,
            wobble: false,
            nasal: false,
            drone: false,
            reverb: 0.18,
            rate: 1.0,
            preset: Preset::Deep,
            settings: Preset::Deep.settings(),
            text: String::new(),
            history: Vec::new(),
            history_pos: 0,
            spoken: Vec::new(),
            last_vowel: None,
            tract_area: Rect::default(),
            mouse: None,
            pop_until: None,
            status: String::new(),
        };
        app.status = if keyboard_release {
            "hold a piano key to sing · Space stops · Tab to type text".into()
        } else {
            NO_RELEASE_HINT.into()
        };
        app.send(Cmd::Note(app.note_f32()));
        app
    }

    pub fn sample_rate(&self) -> f32 {
        self.link.sample_rate
    }

    fn send(&self, cmd: Cmd) {
        let _ = self.cmd.try_send(cmd);
    }

    #[allow(clippy::cast_precision_loss)]
    fn note_f32(&self) -> f32 {
        self.note as f32
    }

    /// Per-frame housekeeping: newest snapshot, timers, freeing old
    /// utterances.
    pub fn tick(&mut self) {
        while let Ok(s) = self.link.snapshots.try_recv() {
            self.snap = s;
        }
        while self.link.trash.try_recv().is_ok() {}
        if self.pop_until.is_some_and(|t| Instant::now() >= t) {
            self.pop_until = None;
            self.send(Cmd::Constriction(SLOT_POP, None));
        }
    }

    /// Handle a key; returns `true` to quit.
    pub fn key(&mut self, key: Key) -> bool {
        if key.ctrl && key.code == KeyCode::Char('c') {
            return true;
        }
        match self.mode {
            Mode::Play => self.play_key(key),
            Mode::Type => {
                if key.kind != KeyKind::Release {
                    self.type_key(key);
                }
                false
            }
        }
    }

    fn play_key(&mut self, key: Key) -> bool {
        let KeyCode::Char(c) = key.code else {
            if key.kind != KeyKind::Release {
                self.play_special(key.code);
            }
            return false;
        };
        let upper = c.is_ascii_uppercase();
        let c = c.to_ascii_lowercase();

        if let Some(semi) = PIANO.find(c) {
            match key.kind {
                KeyKind::Press if self.keyboard_release && self.held == Some(c) => {
                    // A second press of a held key without a release in
                    // between: this terminal does not report releases after
                    // all. Fall back to Space-to-stop.
                    self.keyboard_release = false;
                    self.status = NO_RELEASE_HINT.into();
                }
                KeyKind::Press => {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                    let semi = semi as i32;
                    self.note = 12 * (self.octave + 1) + semi;
                    self.held = Some(c);
                    self.voiced = true;
                    self.send(Cmd::NoteOn(self.note_f32()));
                }
                KeyKind::Release if self.held == Some(c) => {
                    self.held = None;
                    self.voiced = false;
                    self.send(Cmd::NoteOff);
                }
                _ => {}
            }
            return false;
        }

        if key.kind == KeyKind::Release {
            if c == 'b' && self.pop_until.is_none() {
                self.send(Cmd::Constriction(SLOT_POP, None));
            }
            return false;
        }
        if key.kind == KeyKind::Repeat && c == 'b' {
            return false;
        }

        if let Some(&(_, v)) = VOWEL_KEYS.iter().find(|(k, _)| *k == c) {
            self.vowel(v);
            return false;
        }

        match c {
            'q' => return true,
            ' ' => self.silence(),
            'z' => self.octave = (self.octave - 1).max(1),
            'x' => self.octave = (self.octave + 1).min(6),
            'b' => {
                // Lips shut; open on release (or after 150 ms without
                // release events) for a "b"/"p" burst.
                self.send(Cmd::Constriction(
                    SLOT_POP,
                    Some(Constriction::new(41.0, 0.0)),
                ));
                if !self.keyboard_release {
                    self.pop_until = Some(Instant::now() + Duration::from_millis(150));
                }
            }
            'n' => {
                self.nasal = !self.nasal;
                self.send(Cmd::Velum(if self.nasal { 1.0 } else { 0.0 }));
            }
            'v' => self.cycle_preset(if upper || key.shift { -1 } else { 1 }),
            'm' => {
                self.wobble = !self.wobble;
                self.send(Cmd::Wobble(self.wobble));
            }
            'i' => {
                self.drone = !self.drone;
                self.send(Cmd::Drone(self.drone));
            }
            'c' => {
                for slot in 0..synthie_trombone::MAX_CONSTRICTIONS {
                    self.send(Cmd::Constriction(slot, None));
                }
            }
            'r' => {
                self.reverb = match self.reverb {
                    r if r < 0.05 => 0.18,
                    r if r < 0.3 => 0.4,
                    _ => 0.0,
                };
                self.send(Cmd::Reverb(self.reverb));
            }
            '[' => self.set_tenseness(self.tenseness - 0.05),
            ']' => self.set_tenseness(self.tenseness + 0.05),
            '-' => self.set_vibrato(self.vibrato - 0.005),
            '=' => self.set_vibrato(self.vibrato + 0.005),
            ',' => self.rate = (self.rate - 0.1).max(0.4),
            '.' => self.rate = (self.rate + 0.1).min(2.5),
            _ => {}
        }
        false
    }

    fn play_special(&mut self, code: KeyCode) {
        let (mut i, mut d) = self.tongue;
        match code {
            KeyCode::Tab => {
                self.mode = Mode::Type;
                self.status = "type, Enter to speak · ↑↓ history · Esc/Tab back".into();
                return;
            }
            KeyCode::Enter => {
                if let Some(t) = self.history.last().cloned() {
                    self.speak(&t);
                }
                return;
            }
            KeyCode::Esc => {
                self.silence();
                return;
            }
            KeyCode::Left => i -= 0.5,
            KeyCode::Right => i += 0.5,
            KeyCode::Up => d -= 0.08,
            KeyCode::Down => d += 0.08,
            _ => return,
        }
        self.set_tongue(i, d);
    }

    fn type_key(&mut self, key: Key) {
        match key.code {
            KeyCode::Esc | KeyCode::Tab => {
                self.mode = Mode::Play;
                self.status.clear();
            }
            KeyCode::Enter => {
                let t = self.text.trim().to_string();
                if !t.is_empty() {
                    self.speak(&t);
                    if self.history.last() != Some(&t) {
                        self.history.push(t);
                    }
                    self.history_pos = self.history.len();
                    self.text.clear();
                }
            }
            KeyCode::Backspace => {
                self.text.pop();
            }
            KeyCode::Up if self.history_pos > 0 => {
                self.history_pos -= 1;
                self.text.clone_from(&self.history[self.history_pos]);
            }
            KeyCode::Down => {
                self.history_pos = (self.history_pos + 1).min(self.history.len());
                self.text = self
                    .history
                    .get(self.history_pos)
                    .cloned()
                    .unwrap_or_default();
            }
            KeyCode::Char(c) if self.text.len() < 400 => self.text.push(c),
            _ => {}
        }
    }

    /// Stop singing and speaking; the voice fades out. Also ends a drone.
    fn silence(&mut self) {
        self.held = None;
        self.voiced = false;
        if self.drone {
            self.drone = false;
            self.send(Cmd::Drone(false));
        }
        self.send(Cmd::NoteOff);
        self.send(Cmd::StopSpeaking);
    }

    /// Switch to the next (`+1`) or previous (`-1`) voice preset.
    fn cycle_preset(&mut self, dir: isize) {
        let all = Preset::ALL;
        let i = all.iter().position(|&p| p == self.preset).unwrap_or(0);
        #[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
        let next = (i as isize + dir).rem_euclid(all.len() as isize) as usize;
        self.preset = all[next];
        self.settings = self.preset.settings();
        self.tenseness = self.settings.tenseness;
        self.vibrato = self.settings.vibrato;
        // Play at the note nearest the voice's speaking pitch.
        #[allow(clippy::cast_possible_truncation)]
        let note = (69.0 + 12.0 * (self.settings.pitch / 440.0).log2()).round() as i32;
        self.note = note;
        self.octave = (note / 12 - 1).clamp(1, 6);
        self.send(Cmd::Voice(self.settings));
        self.send(Cmd::Note(self.note_f32()));
        self.status = format!("voice: {}", self.preset);
    }

    fn speak(&mut self, text: &str) {
        let prosody = Prosody {
            pitch: synthie_trombone::midi_to_hz(self.note_f32()),
            rate: self.settings.rate * self.rate,
            tenseness: self.tenseness,
            ..self.settings.prosody()
        };
        let u = Utterance::from_text(text, &prosody);
        self.spoken.clear();
        for (k, s) in u.segments().iter().enumerate() {
            let sym = s.phoneme.symbol();
            match self.spoken.last_mut() {
                Some((last, range)) if *last == sym => range.end = k + 1,
                _ => self.spoken.push((sym, k..k + 1)),
            }
        }
        self.status = format!("“{text}” · {:.1} s", u.duration());
        self.send(Cmd::Speak(u));
    }

    fn vowel(&mut self, v: Phoneme) {
        let a = v.articulation();
        if let Some((i, d)) = a.tongue {
            self.set_tongue(i, d);
        }
        self.send(Cmd::Constriction(0, a.constriction));
        self.send(Cmd::Constriction(1, a.lips));
        self.last_vowel = Some(v);
    }

    fn set_tongue(&mut self, i: f32, d: f32) {
        self.tongue = (
            i.clamp(*TONGUE_INDEX.start(), *TONGUE_INDEX.end()),
            d.clamp(*TONGUE_DIAMETER.start(), *TONGUE_DIAMETER.end()),
        );
        self.send(Cmd::Tongue(self.tongue.0, self.tongue.1));
    }

    fn set_tenseness(&mut self, t: f32) {
        self.tenseness = t.clamp(0.0, 1.0);
        self.send(Cmd::Tenseness(self.tenseness));
    }

    fn set_vibrato(&mut self, v: f32) {
        self.vibrato = v.clamp(0.0, 0.05);
        self.send(Cmd::Vibrato(self.vibrato));
    }

    /// Map a terminal cell to tract coordinates `(index, y)`.
    fn to_tract(&self, col: u16, row: u16) -> Option<(f32, f32)> {
        let a = self.tract_area;
        if a.width == 0 || !a.contains((col, row).into()) {
            return None;
        }
        let fx = (f64::from(col - a.x) + 0.5) / f64::from(a.width);
        let fy = (f64::from(row - a.y) + 0.5) / f64::from(a.height);
        let x = X_BOUNDS[0] + fx * (X_BOUNDS[1] - X_BOUNDS[0]);
        let y = Y_BOUNDS[1] - fy * (Y_BOUNDS[1] - Y_BOUNDS[0]);
        #[allow(clippy::cast_possible_truncation)]
        Some((x as f32, y as f32))
    }

    /// Left drag: press the tract closed like a finger on the original.
    /// Right drag: move the tongue body.
    pub fn mouse(&mut self, ev: MouseEvent) {
        match ev.kind {
            MouseEventKind::Down(b) => {
                if self.to_tract(ev.column, ev.row).is_some() {
                    self.mouse = Some(b);
                    self.drag(b, ev.column, ev.row);
                }
            }
            MouseEventKind::Drag(b) if self.mouse == Some(b) => self.drag(b, ev.column, ev.row),
            MouseEventKind::Up(b) if self.mouse == Some(b) => {
                self.mouse = None;
                if b == MouseButton::Left {
                    self.send(Cmd::Constriction(SLOT_MOUSE, None));
                }
            }
            _ => {}
        }
    }

    #[allow(clippy::cast_precision_loss)]
    fn drag(&mut self, b: MouseButton, col: u16, row: u16) {
        let Some((x, y)) = self.to_tract(col, row) else {
            return;
        };
        let index = x.clamp(2.0, N as f32 - 0.5);
        match b {
            MouseButton::Left => {
                // Distance from the midline is the opening left.
                let diameter = (y.abs() * 2.0 - 0.1).max(0.0);
                self.send(Cmd::Constriction(
                    SLOT_MOUSE,
                    Some(Constriction::new(index, diameter)),
                ));
            }
            MouseButton::Right => {
                let span = TONGUE_DIAMETER.end() - TONGUE_DIAMETER.start();
                let d = TONGUE_DIAMETER.start() + (y.abs() / 1.75) * span;
                self.set_tongue(index, d);
            }
            MouseButton::Middle => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::{Receiver, bounded};

    fn app(release: bool) -> (App, Receiver<Cmd>) {
        let (cmd, rx) = bounded(1024);
        let (_s, snapshots) = bounded(1);
        let (_t, trash) = bounded(1);
        let link = Link {
            cmd,
            snapshots,
            trash,
            sample_rate: 48_000.0,
        };
        let app = App::new(link, release);
        while rx.try_recv().is_ok() {}
        (app, rx)
    }

    fn key(c: char, kind: KeyKind) -> Key {
        Key {
            code: KeyCode::Char(c),
            kind,
            ctrl: false,
            shift: false,
        }
    }

    fn sent(rx: &Receiver<Cmd>) -> Vec<Cmd> {
        rx.try_iter().collect()
    }

    #[test]
    fn space_stops_the_voice() {
        let (mut app, rx) = app(false);
        app.key(key('a', KeyKind::Press));
        assert!(matches!(sent(&rx)[..], [Cmd::NoteOn(_)]));
        app.key(key(' ', KeyKind::Press));
        let cmds = sent(&rx);
        assert!(cmds.iter().any(|c| matches!(c, Cmd::NoteOff)), "{cmds:?}");
        assert!(!app.voiced);
    }

    #[test]
    fn escape_stops_the_voice_in_play_mode() {
        let (mut app, rx) = app(true);
        app.key(key('a', KeyKind::Press));
        sent(&rx);
        app.key(Key::press(KeyCode::Esc));
        assert!(sent(&rx).iter().any(|c| matches!(c, Cmd::NoteOff)));
    }

    #[test]
    fn space_also_ends_a_drone() {
        let (mut app, rx) = app(true);
        app.key(key('i', KeyKind::Press));
        sent(&rx);
        app.key(key(' ', KeyKind::Press));
        let cmds = sent(&rx);
        assert!(
            cmds.iter().any(|c| matches!(c, Cmd::Drone(false))),
            "{cmds:?}"
        );
        assert!(cmds.iter().any(|c| matches!(c, Cmd::NoteOff)));
    }

    #[test]
    fn releasing_the_held_key_stops_the_note() {
        let (mut app, rx) = app(true);
        app.key(key('a', KeyKind::Press));
        app.key(key('a', KeyKind::Repeat));
        sent(&rx);
        app.key(key('a', KeyKind::Release));
        assert!(matches!(sent(&rx)[..], [Cmd::NoteOff]));
    }

    #[test]
    fn repeated_presses_mean_no_release_events() {
        let (mut app, rx) = app(true);
        app.key(key('a', KeyKind::Press));
        app.key(key('a', KeyKind::Press));
        assert!(!app.keyboard_release);
        assert!(app.status.contains("Space stops"));
        // The note keeps sounding; Space ends it.
        sent(&rx);
        app.key(key(' ', KeyKind::Press));
        assert!(sent(&rx).iter().any(|c| matches!(c, Cmd::NoteOff)));
    }
}
