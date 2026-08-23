//! The in-app hotkey settings window (`--settings`), launched from the tray's
//! Settings menu item. A tiny eframe window: record a key combination, save it
//! to [`prefs`], and close. It runs as its own short-lived process so it never
//! competes with the tray supervisor's event loop; the tray re-reads the saved
//! hotkey when this window closes.
//!
//! macOS/Windows only (paired with the tray).

use anyhow::{Result, anyhow};
use eframe::egui::{
    self, Button, Color32, CornerRadius, Event, Key, Margin, RichText, Stroke, TextStyle, Vec2,
    ViewportBuilder, ViewportCommand,
};

use crate::hotkey::Combo;
use crate::prefs;
use crate::ui::ACCENT;

/// Red used for the destructive Close button.
const DESTRUCTIVE: Color32 = Color32::from_rgb(0xb8, 0x36, 0x36);

/// Show the settings window. Blocks until the user saves or closes it.
pub fn run() -> Result<()> {
    let current = prefs::load().hotkey.unwrap_or_else(|| "Ctrl+Shift+S".to_string());
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title("scrannotate — Hotkey")
            .with_inner_size([460.0, 300.0])
            .with_resizable(false),
        ..Default::default()
    };
    eframe::run_native(
        "scrannotate settings",
        options,
        Box::new(move |cc| {
            // Fonts/style must be set before the first frame renders — egui
            // only applies new fonts on the next frame, so setting them here
            // (not inside ui()) avoids a first-frame "family not bound" panic.
            install_system_font(&cc.egui_ctx);
            install_style(&cc.egui_ctx);
            Ok(Box::new(SettingsApp::new(current)))
        }),
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

        // Header, hotkey display, and the record button.
        egui::Frame::default()
            .inner_margin(Margin::symmetric(26, 22))
            .show(root, |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("Launch hotkey");
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("Press this anywhere to pop scrannotate and take a screenshot.")
                            .weak(),
                    );
                    ui.add_space(26.0);

                    // The combo in the real system glyphs (⌘/⇧/⌃/⌥).
                    let shown = Combo::parse(&self.combo)
                        .map(|c| c.label())
                        .unwrap_or_else(|_| self.combo.clone());
                    ui.label(combo_text(shown));
                    ui.add_space(24.0);

                    // Record button: accent-outlined when idle, accent-filled
                    // while listening.
                    let text = if self.recording { "Press a combination…" } else { "Record new hotkey" };
                    let mut btn = Button::new(RichText::new(text).size(15.0).color(Color32::WHITE))
                        .min_size(Vec2::new(220.0, 36.0));
                    btn = if self.recording {
                        btn.fill(ACCENT)
                    } else {
                        btn.fill(Color32::from_gray(58)).stroke(Stroke::new(1.5, ACCENT))
                    };
                    if ui.add(btn).clicked() {
                        self.recording = !self.recording;
                        self.error = None;
                    }

                    if let Some(err) = &self.error {
                        ui.add_space(10.0);
                        ui.colored_label(Color32::from_rgb(0xe0, 0x82, 0x82), err);
                    }
                });
            });

        // Footer pinned to the bottom: Close (destructive) left, Save right,
        // matched size and padding.
        let footer_h = 74.0;
        root.add_space((root.available_height() - footer_h).max(0.0));
        root.separator();
        egui::Frame::default()
            .inner_margin(Margin { left: 18, right: 18, top: 0, bottom: 22 })
            .show(root, |ui| {
                let size = Vec2::new(108.0, 36.0);
                ui.horizontal(|ui| {
                    let close = Button::new(RichText::new("Close").color(Color32::WHITE))
                        .fill(DESTRUCTIVE)
                        .min_size(size);
                    if ui.add(close).clicked() {
                        ctx.send_viewport_cmd(ViewportCommand::Close);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let valid = Combo::parse(&self.combo).is_ok();
                        let save = Button::new(RichText::new("Save").color(Color32::WHITE))
                            .fill(ACCENT)
                            .min_size(size);
                        if ui.add_enabled(valid, save).clicked() {
                            prefs::save_hotkey(&self.combo);
                            ctx.send_viewport_cmd(ViewportCommand::Close);
                        }
                    });
                });
            });

        // Esc closes without saving — unless we're recording, where Esc is a
        // candidate key press the capture loop already consumed.
        if !self.recording && ctx.input(|i| i.key_pressed(Key::Escape)) {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }
}

/// App-wide look for this window: rounded widgets and comfortable button
/// padding, matching the toolbar's style.
fn install_style(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        let v = &mut style.visuals;
        for w in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
            &mut v.widgets.noninteractive,
        ] {
            w.corner_radius = CornerRadius::same(7);
        }
        style.spacing.button_padding = Vec2::new(14.0, 8.0);
        style.spacing.item_spacing = Vec2::new(8.0, 8.0);
        if let Some(f) = style.text_styles.get_mut(&TextStyle::Button) {
            f.size = 15.0;
        }
    });
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

/// The combo text, large and centered. Uses the default proportional family —
/// on macOS the system font is appended to it as a fallback (see
/// [`install_system_font`]), so ⌘/⇧/⌃/⌥ resolve to the real system glyphs;
/// elsewhere the label is plain text anyway.
fn combo_text(text: String) -> RichText {
    RichText::new(text).size(30.0).strong()
}

/// On macOS, append the system font (San Francisco) as a last-resort fallback
/// to the default families, so the modifier glyphs the bundled fonts lack
/// (⌘/⌃/⌥) render natively. Appending (not a named family) means the family is
/// always bound — text never panics even before the fonts take effect.
fn install_system_font(ctx: &egui::Context) {
    #[cfg(target_os = "macos")]
    {
        use eframe::egui::{FontData, FontDefinitions, FontFamily};
        let Ok(bytes) = std::fs::read("/System/Library/Fonts/SFNS.ttf") else { return };
        let mut fonts = FontDefinitions::default();
        fonts
            .font_data
            .insert("system".to_owned(), std::sync::Arc::new(FontData::from_owned(bytes)));
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts.families.entry(family).or_default().push("system".to_owned());
        }
        ctx.set_fonts(fonts);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = ctx;
}
