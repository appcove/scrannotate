//! The in-app hotkey settings window (`--settings`), launched from the tray's
//! Settings menu item. A tiny eframe window: record a key combination, save it
//! to [`prefs`], and close. It runs as its own short-lived process so it never
//! competes with the tray supervisor's event loop; the tray re-reads the saved
//! hotkey when this window closes.
//!
//! macOS/Windows only (paired with the tray).

use anyhow::{Result, anyhow};
use eframe::egui::{
    self, Align2, Color32, Event, FontId, Key, Pos2, Rect, RichText, Sense, Shape, Stroke, Vec2,
    ViewportBuilder, ViewportCommand,
};

use crate::hotkey::{Combo, Modifier};
use crate::prefs;

/// Show the settings window. Blocks until the user saves or closes it.
pub fn run() -> Result<()> {
    let current = prefs::load().hotkey.unwrap_or_else(|| "Ctrl+Shift+S".to_string());
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title("scrannotate — Hotkey")
            .with_inner_size([440.0, 240.0])
            .with_resizable(false),
        ..Default::default()
    };
    eframe::run_native(
        "scrannotate settings",
        options,
        Box::new(move |_| Ok(Box::new(SettingsApp::new(current)))),
    )
    .map_err(|e| anyhow!("running settings window: {e}"))
}

struct SettingsApp {
    /// The combo string being edited (e.g. `Cmd+Shift+4`).
    combo: String,
    /// True while waiting to capture the next keypress.
    recording: bool,
    error: Option<String>,
}

impl SettingsApp {
    fn new(current: String) -> Self {
        Self { combo: current, recording: false, error: None }
    }

    /// Turn the pressed key + live modifiers into a combo string, validated by
    /// the same parser the tray uses. Pure modifiers alone are ignored.
    fn capture(&mut self, ctx: &egui::Context) {
        let events = ctx.input(|i| i.events.clone());
        for ev in events {
            let Event::Key { key, pressed: true, modifiers, .. } = ev else { continue };
            let Some(token) = key_token(key) else {
                self.error = Some("That key can't be used in a hotkey — try a letter or number.".into());
                continue;
            };
            let mut parts: Vec<&str> = Vec::new();
            // On macOS the Command key is the natural chord key; elsewhere Ctrl.
            if modifiers.mac_cmd {
                parts.push("Cmd");
            }
            if modifiers.ctrl {
                parts.push("Ctrl");
            }
            if modifiers.alt {
                parts.push("Alt");
            }
            if modifiers.shift {
                parts.push("Shift");
            }
            if parts.is_empty() {
                self.error = Some("Hold at least one modifier (⌘/Ctrl/Alt/Shift) with the key.".into());
                continue;
            }
            let candidate = format!("{}+{}", parts.join("+"), token);
            match Combo::parse(&candidate) {
                Ok(_) => {
                    self.combo = candidate;
                    self.recording = false;
                    self.error = None;
                }
                Err(e) => self.error = Some(e.to_string()),
            }
            return;
        }
    }
}

impl eframe::App for SettingsApp {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        if self.recording {
            self.capture(&ctx);
            ctx.request_repaint();
        }

        root.add_space(14.0);
        root.vertical_centered(|ui| {
            ui.heading("Launch hotkey");
            ui.add_space(4.0);
            ui.label(
                RichText::new("Pressing this anywhere pops scrannotate to take a screenshot.")
                    .weak(),
            );
            ui.add_space(18.0);

            // The current combo, drawn as key-symbols (egui's fonts can't
            // render ⌃/⌥/⌘, so we paint them).
            match Combo::parse(&self.combo) {
                Ok(combo) => draw_combo(ui, &combo),
                Err(_) => {
                    ui.label(RichText::new(&self.combo).size(26.0).strong());
                }
            }
            ui.add_space(12.0);

            let label = if self.recording { "Press a combination…" } else { "Record new hotkey" };
            if ui.button(RichText::new(label).size(15.0)).clicked() {
                self.recording = !self.recording;
                self.error = None;
            }

            if let Some(err) = &self.error {
                ui.add_space(6.0);
                ui.colored_label(Color32::from_rgb(220, 90, 90), err);
            }
        });

        // Save / Close along the bottom.
        root.add_space(20.0);
        root.separator();
        root.horizontal(|ui| {
            if ui.button("Close").clicked() {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let valid = Combo::parse(&self.combo).is_ok();
                if ui
                    .add_enabled(valid, egui::Button::new("Save").min_size(Vec2::new(90.0, 28.0)))
                    .clicked()
                {
                    prefs::save_hotkey(&self.combo);
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
            });
        });

        // Esc closes without saving — unless we're recording, where Esc is a
        // candidate key press the capture loop already consumed.
        if !self.recording && ctx.input(|i| i.key_pressed(Key::Escape)) {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }
}

/// Map an egui [`Key`] to a token our [`Combo`] parser accepts. Uses the
/// key's Debug name to avoid enumerating every variant: letters (`A`), digits
/// (`Num4` → `4`), function keys (`F5`), and a few named keys.
fn key_token(key: Key) -> Option<String> {
    let name = format!("{key:?}");
    // Single letter, e.g. "S".
    if name.len() == 1 && name.chars().next().unwrap().is_ascii_alphabetic() {
        return Some(name);
    }
    // "Num4" -> "4".
    if let Some(digit) = name.strip_prefix("Num")
        && digit.len() == 1
        && digit.chars().next().unwrap().is_ascii_digit()
    {
        return Some(digit.to_string());
    }
    // "F1".."F12".
    if let Some(n) = name.strip_prefix('F')
        && !n.is_empty()
        && n.chars().all(|c| c.is_ascii_digit())
    {
        return Some(name);
    }
    match name.as_str() {
        "Space" => Some("Space".to_string()),
        _ => None,
    }
}

/// Draw the combo as a centered row of modifier symbols + the key, painting
/// the modifier glyphs ourselves so they render identically (egui's bundled
/// fonts have none of ⌃/⌥/⌘, only a stray ⇧).
fn draw_combo(ui: &mut egui::Ui, combo: &Combo) {
    let color = ui.visuals().strong_text_color();
    let stroke = Stroke::new(2.4, color);
    const BOX: f32 = 40.0;

    ui.horizontal(|ui| {
        for m in combo.modifiers() {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(34.0, BOX), Sense::hover());
            draw_modifier(ui.painter(), rect, m, stroke, color);
        }
        let (rect, _) = ui.allocate_exact_size(Vec2::new(30.0, BOX), Sense::hover());
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            combo.key_display(),
            FontId::proportional(27.0),
            color,
        );
    });
}

/// Paint one modifier symbol centered in `rect`.
fn draw_modifier(painter: &egui::Painter, rect: Rect, m: Modifier, stroke: Stroke, fill: Color32) {
    let c = rect.center();
    match m {
        // ⌘ — four corner loops joined into a square.
        Modifier::Meta => {
            let o = 6.5;
            let corners = [
                c + Vec2::new(-o, -o),
                c + Vec2::new(o, -o),
                c + Vec2::new(o, o),
                c + Vec2::new(-o, o),
            ];
            for p in corners {
                painter.circle_stroke(p, 3.6, stroke);
            }
            for i in 0..4 {
                painter.line_segment([corners[i], corners[(i + 1) % 4]], stroke);
            }
        }
        // ⇧ — filled up-arrow.
        Modifier::Shift => {
            let top = rect.top() + 9.0;
            let mid = c.y + 1.0;
            let bot = rect.bottom() - 9.0;
            let (tw, sw) = (9.0, 4.0);
            let tri = vec![
                Pos2::new(c.x, top),
                Pos2::new(c.x - tw, mid),
                Pos2::new(c.x + tw, mid),
            ];
            painter.add(Shape::convex_polygon(tri, fill, Stroke::NONE));
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(c.x - sw, mid), Pos2::new(c.x + sw, bot)),
                0.0,
                fill,
            );
        }
        // ⌃ — upward chevron.
        Modifier::Ctrl => {
            let apex = Pos2::new(c.x, rect.top() + 12.0);
            let y = c.y + 3.0;
            painter.line_segment([apex, Pos2::new(c.x - 8.5, y)], stroke);
            painter.line_segment([apex, Pos2::new(c.x + 8.5, y)], stroke);
        }
        // ⌥ — option: a diagonal into the top-right bar, plus a top-left dash.
        Modifier::Alt => {
            let ytop = rect.top() + 12.0;
            let ybot = rect.bottom() - 12.0;
            let xl = rect.left() + 7.0;
            let xr = rect.right() - 7.0;
            painter.line_segment([Pos2::new(xl, ybot), Pos2::new(c.x, ytop)], stroke);
            painter.line_segment([Pos2::new(c.x, ytop), Pos2::new(xr, ytop)], stroke);
            painter.line_segment([Pos2::new(xl, ytop), Pos2::new(xl + 6.5, ytop)], stroke);
        }
    }
}
