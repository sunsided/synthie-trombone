//! README screenshots: render real UI states to HTML, which `task
//! screenshots` turns into PNGs with headless Chrome.
//!
//! ```sh
//! cargo test -p synthie-trombone-cli --release -- --ignored screenshots
//! ```

use crate::app::{App, Mode};
use crate::audio::{Cmd, Link, Scope, Snapshot};
use crossbeam_channel::{Receiver, bounded};
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use ratatui::{Terminal, backend::TestBackend};
use std::fmt::Write as _;
use synthie_trombone::speech::{Speaker, Utterance};
use synthie_trombone::{Constriction, Phoneme, Preset, Trombone};
use synthie_ui::{Key, KeyCode, KeyKind};

const SR: f32 = 48_000.0;
const COLS: u16 = 120;
const ROWS: u16 = 36;

fn app() -> (App, Receiver<Cmd>) {
    let (cmd, rx) = bounded(4096);
    let (_s, snapshots) = bounded(1);
    let (_t, trash) = bounded(1);
    let link = Link {
        cmd,
        snapshots,
        trash,
        sample_rate: SR,
    };
    (App::new(link, true), rx)
}

fn press(app: &mut App, c: char) {
    app.key(Key {
        code: KeyCode::Char(c),
        kind: KeyKind::Press,
        ctrl: false,
        shift: false,
    });
}

/// Run `seconds` of audio through `voice`/`speaker`, keeping the scope fed.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn run(voice: &mut Trombone, speaker: &mut Speaker, scope: &mut Scope, seconds: f32) {
    for _ in 0..(seconds * SR) as usize {
        scope.push(speaker.process(voice) * 0.9);
    }
}

fn color(c: Color, fallback: &str) -> String {
    match c {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Reset => fallback.to_string(),
        Color::Black => "#000".into(),
        Color::White | Color::Gray => "#ddd".into(),
        other => format!("{other}").to_lowercase(),
    }
}

/// The buffer as a standalone HTML page. Every cell is a fixed-size box,
/// so glyphs from fallback fonts (braille, arrows) cannot shift the grid.
fn to_html(buf: &Buffer) -> String {
    let (fg0, bg0) = ("#ded8ff", "#0c0918");
    let mut body = String::new();
    for y in 0..buf.area.height {
        body.push_str("<div class=r>");
        let mut x = 0;
        while x < buf.area.width {
            let cell = &buf[(x, y)];
            // Wide glyphs leave an empty symbol in the following cell.
            let wide = x + 1 < buf.area.width && buf[(x + 1, y)].symbol().is_empty();
            let weight = if cell.modifier.contains(Modifier::BOLD) {
                "font-weight:bold;"
            } else {
                ""
            };
            let width = if wide { " w" } else { "" };
            let _ = write!(
                body,
                "<span class=\"c{width}\" style=\"color:{};background:{};{weight}\">",
                color(cell.fg, fg0),
                color(cell.bg, bg0)
            );
            for ch in cell.symbol().chars() {
                match ch {
                    '<' => body.push_str("&lt;"),
                    '>' => body.push_str("&gt;"),
                    '&' => body.push_str("&amp;"),
                    ' ' => body.push_str("&nbsp;"),
                    c => body.push(c),
                }
            }
            body.push_str("</span>");
            x += if wide { 2 } else { 1 };
        }
        body.push_str("</div>");
    }
    format!(
        "<!doctype html><meta charset=utf-8><style>\
         html,body{{margin:0;background:#000}}\
         .t{{margin:24px;display:inline-block;background:{bg0};\
         font:15px 'DejaVu Sans Mono',monospace}}\
         .r{{display:flex;height:18px}}\
         .c{{display:inline-block;width:9px;height:18px;line-height:18px;\
         overflow:hidden;text-align:center;white-space:pre}}\
         .c.w{{width:18px}}</style><div class=t>{body}</div>"
    )
}

fn shoot(name: &str, app: &mut App) {
    let mut term = Terminal::new(TestBackend::new(COLS, ROWS)).unwrap();
    term.draw(|f| crate::ui::draw(f, app)).unwrap();
    let dir = std::env::var("SCREENSHOT_DIR").unwrap_or_else(|_| "target/screenshots".into());
    std::fs::create_dir_all(&dir).unwrap();
    let path = format!("{dir}/{name}.html");
    std::fs::write(&path, to_html(term.backend().buffer())).unwrap();
    println!("wrote {path}");
}

/// Singing an "ah" with the critter voice, finger on the tongue tip.
fn singing() {
    let (mut app, _rx) = app();
    for _ in 0..5 {
        press(&mut app, 'v'); // man → critter
    }
    press(&mut app, '5'); // AA
    press(&mut app, 'g');

    let mut voice = Trombone::new(SR);
    let mut speaker = Speaker::new();
    let mut scope = Scope::default();
    app.settings.apply(&mut voice);
    voice.set_note(f32::from(i16::try_from(app.note).unwrap()));
    voice.articulate_phoneme(Phoneme::Aa);
    voice.set_constriction(2, Some(Constriction::new(35.5, 0.75)));
    voice.set_velum(0.0);
    voice.note_on();
    run(&mut voice, &mut speaker, &mut scope, 0.8);
    app.snap = Snapshot::capture(&voice, &speaker, &scope);
    app.status = "voice: critter".into();
    shoot("singing", &mut app);
}

/// Mid-sentence: the woman voice saying a line, caught on the "m".
fn speaking() {
    let (mut app, _rx) = app();
    press(&mut app, 'v'); // woman
    app.mode = Mode::Type;
    app.text = "Can you hear me?".into();

    let text = "Hello, I am the pink trombone!";
    let mut voice = Trombone::new(SR);
    Speaker::prepare(&mut voice);
    app.settings.apply(&mut voice);
    let u = Utterance::from_text(text, &app.settings.prosody());
    for (k, s) in u.segments().iter().enumerate() {
        let sym = s.phoneme.symbol();
        match app.spoken.last_mut() {
            Some((last, range)) if *last == sym => range.end = k + 1,
            _ => app.spoken.push((sym, k..k + 1)),
        }
    }
    // Stop in the middle of the "AE" of "am", lips about to close.
    let target = u
        .segments()
        .iter()
        .position(|s| s.phoneme == Phoneme::M)
        .unwrap();
    let mut speaker = Speaker::new();
    let mut scope = Scope::default();
    speaker.speak(u);
    while speaker.current().is_some_and(|(i, _)| i < target) {
        run(&mut voice, &mut speaker, &mut scope, 0.001);
    }
    run(&mut voice, &mut speaker, &mut scope, 0.03);
    app.snap = Snapshot::capture(&voice, &speaker, &scope);
    app.status = format!("“{text}” · 2.1 s");
    shoot("speaking", &mut app);
}

#[test]
#[ignore = "writes README screenshots; run via `task screenshots`"]
fn screenshots() {
    let _ = Preset::ALL;
    singing();
    speaking();
}
