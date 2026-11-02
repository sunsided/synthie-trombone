//! Rendering: the tract as a Braille side view, glottis meters, scope,
//! and the speech line.

use crate::app::{App, Mode, VOWEL_KEYS, X_BOUNDS, Y_BOUNDS};
use crate::audio::SCOPE_LEN;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::widgets::canvas::{Canvas, Context, Line as CLine};
use synthie_trombone::{N, NOSE_LEN, NOSE_START, TONGUE_DIAMETER};
use synthie_ui::theme::{self, BG, CYAN, DIM, FAINT, FG, MAGENTA, PURPLE, YELLOW, bold, fg};
use synthie_ui::widgets::{key_hints, keycap, logo, meter, paint_background, panel, vu};

/// Vertical centre of the nasal tract in canvas units.
const NOSE_Y: f64 = 3.6;
const NOTE_NAMES: [&str; 12] = [
    "C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B",
];

pub fn draw(frame: &mut Frame, app: &mut App) {
    paint_background(frame);
    let [header, tract, mid, speech, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(12),
        Constraint::Length(10),
        Constraint::Length(4),
        Constraint::Length(3),
    ])
    .areas(frame.area());

    draw_header(frame, header, app);
    draw_tract(frame, tract, app);
    let [glottis, scope] =
        Layout::horizontal([Constraint::Length(46), Constraint::Min(20)]).areas(mid);
    draw_glottis(frame, glottis, app);
    draw_scope(frame, scope, app);
    draw_speech(frame, speech, app);
    draw_footer(frame, footer, app);
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let mut spans = logo().spans;
    spans.push(Span::styled("  P I N K   T R O M B O N E  ", bold(MAGENTA)));
    let (label, color) = match app.mode {
        Mode::Play => (" PLAY ", CYAN),
        Mode::Type => (" TYPE ", YELLOW),
    };
    spans.push(Span::styled(
        label,
        Style::default()
            .fg(BG)
            .bg(color)
            .add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(format!("  {}", app.status), fg(DIM)));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn amp_color(a: f32) -> Color {
    theme::heat((a * 1.6).clamp(0.0, 1.0))
}

fn lerp_cell(cells: &[f32], x: f64) -> f64 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let lo = (x.floor().max(0.0) as usize).min(cells.len() - 1);
    let hi = (lo + 1).min(cells.len() - 1);
    #[allow(clippy::cast_precision_loss)]
    let frac = (x - lo as f64).clamp(0.0, 1.0);
    f64::from(cells[lo]) * (1.0 - frac) + f64::from(cells[hi]) * frac
}

#[allow(
    clippy::cast_precision_loss,
    clippy::too_many_lines,
    clippy::many_single_char_names
)]
fn draw_tract(frame: &mut Frame, area: Rect, app: &mut App) {
    let block = panel(
        "vocal tract  ·  left-drag: constrict  ·  right-drag: tongue",
        true,
    );
    app.tract_area = block.inner(area);
    let s = app.snap;

    let canvas = Canvas::default()
        .block(block)
        .background_color(BG)
        .marker(Marker::Braille)
        .x_bounds(X_BOUNDS)
        .y_bounds(Y_BOUNDS)
        .paint(move |ctx: &mut Context| {
            // Air column, coloured by recent pressure.
            let mut x = 0.0;
            while x <= (N - 1) as f64 {
                let d = lerp_cell(&s.diameters, x) / 2.0;
                let a = lerp_cell(&s.amplitudes, x);
                #[allow(clippy::cast_possible_truncation)]
                let color = amp_color(a as f32);
                ctx.draw(&CLine::new(x, -d, x, d, color));
                x += 0.2;
            }
            let mut x = NOSE_START as f64;
            while x <= (N - 1) as f64 {
                let k = x - NOSE_START as f64;
                let d = lerp_cell(&s.nose, k) / 2.0;
                let a = lerp_cell(&s.nose_amplitudes, k);
                #[allow(clippy::cast_possible_truncation)]
                let color = amp_color(a as f32);
                ctx.draw(&CLine::new(x, NOSE_Y - d, x, NOSE_Y + d, color));
                x += 0.2;
            }

            // Target shape, faint.
            for i in 0..N - 1 {
                let (a, b) = (
                    f64::from(s.targets[i]) / 2.0,
                    f64::from(s.targets[i + 1]) / 2.0,
                );
                let (x0, x1) = (i as f64, (i + 1) as f64);
                ctx.draw(&CLine::new(x0, a, x1, b, FAINT));
                ctx.draw(&CLine::new(x0, -a, x1, -b, FAINT));
            }
            // Walls.
            for i in 0..N - 1 {
                let (a, b) = (
                    f64::from(s.diameters[i]) / 2.0,
                    f64::from(s.diameters[i + 1]) / 2.0,
                );
                let (x0, x1) = (i as f64, (i + 1) as f64);
                ctx.draw(&CLine::new(x0, a, x1, b, CYAN));
                ctx.draw(&CLine::new(x0, -a, x1, -b, PURPLE));
            }
            for k in 0..NOSE_LEN - 1 {
                let (a, b) = (f64::from(s.nose[k]) / 2.0, f64::from(s.nose[k + 1]) / 2.0);
                let (x0, x1) = ((NOSE_START + k) as f64, (NOSE_START + k + 1) as f64);
                ctx.draw(&CLine::new(x0, NOSE_Y + a, x1, NOSE_Y + b, CYAN));
                ctx.draw(&CLine::new(x0, NOSE_Y - a, x1, NOSE_Y - b, CYAN));
            }
            // Velum: the door between mouth and nose.
            let top = f64::from(s.diameters[NOSE_START]) / 2.0;
            let gap = f64::from(s.velum);
            let vx = NOSE_START as f64;
            let vcol = if gap > 0.2 { YELLOW } else { DIM };
            ctx.draw(&CLine::new(
                vx,
                top,
                vx,
                NOSE_Y - f64::from(s.nose[0]) / 2.0,
                vcol,
            ));
            ctx.draw(&CLine::new(
                vx + 0.6,
                top,
                vx + 0.6 + gap * 0.8,
                top + 0.8,
                vcol,
            ));
            ctx.layer();

            // Markers.
            let (ti, td) = s.tongue;
            let depth =
                (td - TONGUE_DIAMETER.start()) / (TONGUE_DIAMETER.end() - TONGUE_DIAMETER.start());
            ctx.print(
                f64::from(ti),
                -0.6 - 1.5 * f64::from(depth),
                Span::styled("▲tongue", bold(YELLOW)),
            );
            for (k, c) in s.constrictions.iter().enumerate() {
                if let Some(c) = c {
                    let label = match k {
                        0 | 1 => "✕",
                        2 => "●",
                        _ => "○",
                    };
                    ctx.print(f64::from(c.index), 1.9, Span::styled(label, bold(MAGENTA)));
                }
            }
            ctx.print(-0.8, -2.4, Span::styled("glottis", fg(DIM)));
            ctx.print(40.5, -2.4, Span::styled("lips", fg(DIM)));
            ctx.print(41.0, NOSE_Y + 1.1, Span::styled("nose", fg(DIM)));
            ctx.print(NOSE_START as f64 - 3.5, 2.4, Span::styled("velum", fg(DIM)));
        });
    frame.render_widget(canvas, area);
}

fn note_name(note: i32) -> String {
    #[allow(clippy::cast_sign_loss)]
    let name = NOTE_NAMES[note.rem_euclid(12) as usize];
    format!("{name}{}", note.div_euclid(12) - 1)
}

fn row<'a>(label: &'a str, bar: Vec<Span<'a>>, value: impl std::fmt::Display) -> Line<'a> {
    let mut spans = vec![Span::styled(format!(" {label:<8}"), fg(DIM))];
    spans.extend(bar);
    spans.push(Span::styled(format!(" {value}"), fg(FG)));
    Line::from(spans)
}

fn draw_glottis(frame: &mut Frame, area: Rect, app: &App) {
    let s = &app.snap;
    let w = 22;
    let on_off = |b: bool| {
        if b {
            "on".to_string()
        } else {
            "off".to_string()
        }
    };
    let vowel = app.last_vowel.map_or("—", |v| v.symbol());
    let lines = vec![
        Line::from(vec![
            Span::styled(" note    ", fg(DIM)),
            Span::styled(format!("{:<4}", note_name(app.note)), bold(YELLOW)),
            Span::styled(format!(" {:6.1} Hz", s.frequency), fg(FG)),
            Span::styled(format!("  oct {}  {vowel}  ", app.octave), fg(DIM)),
            Span::styled(app.preset.name(), bold(MAGENTA)),
        ]),
        row(
            "voice",
            meter(s.intensity, w),
            format!("{:.2}", s.intensity),
        ),
        row(
            "tense",
            meter(s.tenseness, w),
            format!("{:.2}", s.tenseness),
        ),
        row(
            "tongue",
            meter((s.tongue.0 - 12.0) / 17.0, w),
            format!("{:.1}/{:.2}", s.tongue.0, s.tongue.1),
        ),
        row(
            "velum",
            meter(s.velum, w),
            format!("{:.2}{}", s.velum, if app.nasal { " (n)" } else { "" }),
        ),
        row(
            "vibrato",
            meter(app.vibrato / 0.05, w),
            format!("{:.3}", app.vibrato),
        ),
        row(
            "reverb",
            meter(app.reverb / 0.4, w),
            format!("{:.2}", app.reverb),
        ),
        Line::from(vec![
            Span::styled(" wobble ", fg(DIM)),
            Span::styled(format!("{:<4}", on_off(app.wobble)), fg(FG)),
            Span::styled(" drone ", fg(DIM)),
            Span::styled(format!("{:<4}", on_off(app.drone)), fg(FG)),
            Span::styled(" rate ", fg(DIM)),
            Span::styled(format!("×{:.1}", app.rate), fg(FG)),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines).block(panel("glottis", false)), area);
}

#[allow(clippy::cast_precision_loss)]
fn draw_scope(frame: &mut Frame, area: Rect, app: &App) {
    let s = &app.snap.scope;
    // Trigger on a rising zero crossing so the waveform stands still.
    let show = SCOPE_LEN / 2;
    let start = (1..SCOPE_LEN - show)
        .find(|&i| s[i - 1] < 0.0 && s[i] >= 0.0)
        .unwrap_or(0);
    let gain = 0.9 / app.snap.level.max(0.05);
    let samples: Vec<f64> = s[start..start + show]
        .iter()
        .map(|v| f64::from(v * gain))
        .collect();
    let title = format!("scope  ·  {:.0} kHz", app.sample_rate() / 1000.0);
    let mut lvl = vec![Span::raw(" ")];
    lvl.extend(vu(app.snap.level, 12));
    let block = panel(&title, false).title_bottom(Line::from(lvl));
    let canvas = Canvas::default()
        .block(block)
        .background_color(BG)
        .marker(Marker::Braille)
        .x_bounds([0.0, show as f64])
        .y_bounds([-1.0, 1.0])
        .paint(move |ctx| {
            ctx.draw(&CLine::new(0.0, 0.0, show as f64, 0.0, FAINT));
            for (i, w) in samples.windows(2).enumerate() {
                let x = i as f64;
                ctx.draw(&CLine::new(
                    x,
                    w[0],
                    x + 1.0,
                    w[1],
                    theme::gradient(i as f32 / show as f32),
                ));
            }
        });
    frame.render_widget(canvas, area);
}

fn draw_speech(frame: &mut Frame, area: Rect, app: &App) {
    let focused = app.mode == Mode::Type;
    let cursor = if focused && (theme::clock() * 2.0).fract() < 0.5 {
        "▏"
    } else {
        " "
    };
    let prompt = Line::from(vec![
        Span::styled(" ❯ ", bold(if focused { YELLOW } else { DIM })),
        Span::styled(app.text.clone(), fg(FG)),
        Span::styled(cursor, fg(YELLOW)),
        if app.text.is_empty() && !focused {
            Span::styled("press Tab and type something to say", fg(FAINT))
        } else {
            Span::raw("")
        },
    ]);
    let current = app.snap.speaking;
    let mut strip = vec![Span::raw("   ")];
    for (sym, range) in &app.spoken {
        let style = if current.is_some_and(|c| range.contains(&c)) {
            Style::default()
                .fg(BG)
                .bg(MAGENTA)
                .add_modifier(Modifier::BOLD)
        } else if *sym == "SIL" {
            fg(FAINT)
        } else {
            fg(PURPLE)
        };
        let sym = if *sym == "SIL" { "·" } else { sym };
        strip.push(Span::styled(format!("{sym} "), style));
    }
    frame.render_widget(
        Paragraph::new(vec![prompt, Line::from(strip)]).block(panel("speak", focused)),
        area,
    );
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let line = match app.mode {
        Mode::Play => {
            let l = key_hints(&[
                ("a‥;", "sing"),
                ("Space", "stop"),
                ("z x", "octave"),
                ("←→↑↓", "tongue"),
                ("[ ]", "tense"),
                ("n", "nasal"),
                ("b", "pop"),
                ("i", "drone"),
                ("Tab", "type"),
                ("q", "quit"),
            ]);
            let mut v = vec![Span::styled(" vowels ", fg(DIM))];
            for (k, p) in VOWEL_KEYS {
                v.push(keycap_owned(k));
                v.push(Span::styled(format!(" {}  ", p.symbol()), fg(DIM)));
            }
            let settings = key_hints(&[
                ("v V", "voice"),
                ("- =", "vibrato"),
                ("m", "wobble"),
                ("r", "reverb"),
                (", .", "speech rate"),
                ("c", "clear"),
            ]);
            vec![l, Line::from(v), settings]
        }
        Mode::Type => vec![key_hints(&[
            ("Enter", "speak"),
            ("↑↓", "history"),
            ("[HH AH0 L OW1]", "ARPAbet"),
            ("Esc", "back"),
        ])],
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn keycap_owned(c: char) -> Span<'static> {
    let style = keycap("").style;
    Span::styled(format!(" {c} "), style)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{Cmd, Link};
    use crossbeam_channel::bounded;
    use ratatui::{Terminal, backend::TestBackend};
    use synthie_trombone::speech::{Prosody, Speaker, Utterance};
    use synthie_trombone::{Constriction, Phoneme, Trombone};

    /// Render one frame of a live-looking voice to text, so layout changes
    /// can be eyeballed with `cargo test -- --nocapture`.
    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn renders_a_frame() {
        let (cmd, cmd_rx) = bounded::<Cmd>(1024);
        let (_snap_tx, snapshots) = bounded(1);
        let (_trash_tx, trash) = bounded(1);
        let link = Link {
            cmd,
            snapshots,
            trash,
            sample_rate: 48_000.0,
        };
        let mut app = App::new(link, true);
        app.preset = synthie_trombone::Preset::Critter;

        // Drive a real voice into an interesting state.
        let mut voice = Trombone::new(48_000.0);
        voice.articulate_phoneme(Phoneme::Aa);
        voice.set_velum(0.6);
        voice.set_constriction(2, Some(Constriction::new(36.0, 0.5)));
        voice.note_on_midi(45.0);
        let mut buf = vec![0.0; 24_000];
        voice.render(&mut buf);
        let mut snap = crate::audio::Snapshot::default();
        snap.diameters = *voice.diameters();
        snap.targets = *voice.target_diameters();
        snap.amplitudes = *voice.amplitudes();
        snap.nose = *voice.nose_diameters();
        snap.nose_amplitudes = *voice.nose_amplitudes();
        snap.velum = voice.velum();
        snap.tongue = voice.tongue();
        snap.intensity = voice.intensity();
        snap.frequency = voice.frequency();
        snap.constrictions[2] = voice.constriction(2);
        snap.scope.copy_from_slice(&buf[buf.len() - SCOPE_LEN..]);
        snap.level = 0.4;
        snap.speaking = Some(3);
        app.snap = snap;
        app.text = "hello there".into();
        let u = Utterance::from_text("Hello, pink trombone!", &Prosody::default());
        for (k, s) in u.segments().iter().enumerate() {
            app.spoken.push((s.phoneme.symbol(), k..k + 1));
        }
        let _ = Speaker::new();
        drop(cmd_rx);

        let mut term = Terminal::new(TestBackend::new(120, 34)).unwrap();
        term.draw(|f| draw(f, &mut app)).unwrap();
        let buf = term.backend().buffer();
        let mut text = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                text.push_str(buf[(x, y)].symbol());
            }
            text.push('\n');
        }
        println!("{text}");
        assert!(text.contains("vocal tract"));
        assert!(app.tract_area.width > 0);
    }
}
