//! Persisted user preferences: a tiny `key=value` file in the same state
//! directory as the portal restore token ([`state_dir`]). Stores the
//! recently-used color palette, and — only once the user has deliberately
//! adjusted them — the stroke width and font size. Untouched sizes stay
//! resolution-scaled defaults and are never written, so a small capture's
//! defaults can't leak into a 4K session.

use std::path::PathBuf;

use eframe::egui::Color32;

use crate::annotate::Style;

/// `$XDG_STATE_HOME/scrannotate` (default `~/.local/state/scrannotate`).
#[cfg(all(unix, not(target_os = "macos")))]
pub fn state_dir() -> Option<PathBuf> {
    let root = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|h| h.join(".local/state")))?;
    Some(root.join("scrannotate"))
}

/// `~/Library/Application Support/scrannotate` on macOS,
/// `%LOCALAPPDATA%\scrannotate` on Windows.
#[cfg(any(target_os = "macos", windows))]
pub fn state_dir() -> Option<PathBuf> {
    Some(dirs::data_local_dir()?.join("scrannotate"))
}

fn prefs_path() -> Option<PathBuf> {
    Some(state_dir()?.join("prefs"))
}

#[derive(Default)]
pub struct Prefs {
    /// Recently-used palette, newest first (the head is the active color).
    pub palette: Option<Vec<Color32>>,
    pub width: Option<f32>,
    pub font_size: Option<f32>,
    /// The tray's launch hotkey, e.g. `Cmd+Shift+4` (set in Settings).
    pub hotkey: Option<String>,
}

fn parse_color(hex: &str) -> Option<Color32> {
    let hex = hex.trim();
    if hex.len() != 8 {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    Some(Color32::from_rgba_unmultiplied(byte(0)?, byte(2)?, byte(4)?, byte(6)?))
}

fn format_color(c: Color32) -> String {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    format!("{r:02x}{g:02x}{b:02x}{a:02x}")
}

pub fn load() -> Prefs {
    let mut prefs = Prefs::default();
    let Some(path) = prefs_path() else { return prefs };
    let Ok(contents) = std::fs::read_to_string(path) else { return prefs };
    for line in contents.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        match key.trim() {
            "palette" => {
                let colors: Vec<Color32> = value.split(',').filter_map(parse_color).collect();
                if !colors.is_empty() {
                    prefs.palette = Some(colors);
                }
            }
            "width" => {
                prefs.width = value.trim().parse().ok().filter(|w| (1.0..=64.0).contains(w));
            }
            "font_size" => {
                prefs.font_size =
                    value.trim().parse().ok().filter(|s| (6.0..=400.0).contains(s));
            }
            "hotkey" => {
                let v = value.trim();
                if !v.is_empty() {
                    prefs.hotkey = Some(v.to_string());
                }
            }
            _ => {}
        }
    }
    prefs
}

/// Serialize the whole `Prefs` back to the file (only the `Some` fields
/// produce lines). One writer so every caller preserves the fields it does
/// not itself own — e.g. saving the palette must not drop the hotkey.
fn write_all(prefs: &Prefs) {
    let Some(path) = prefs_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut contents = String::new();
    if let Some(palette) = &prefs.palette {
        let line = palette.iter().map(|c| format_color(*c)).collect::<Vec<_>>().join(",");
        contents.push_str(&format!("palette={line}\n"));
    }
    if let Some(width) = prefs.width {
        contents.push_str(&format!("width={width}\n"));
    }
    if let Some(font_size) = prefs.font_size {
        contents.push_str(&format!("font_size={font_size}\n"));
    }
    if let Some(hotkey) = &prefs.hotkey {
        contents.push_str(&format!("hotkey={hotkey}\n"));
    }
    if let Err(err) = std::fs::write(&path, contents) {
        eprintln!("warning: could not persist preferences: {err}");
    }
}

/// `style: None` keeps sizes out of the file (they stay session defaults).
/// The hotkey is carried over from disk so the editor's saves don't drop it.
pub fn save(palette: &[Color32], style: Option<&Style>) {
    let mut prefs = load();
    prefs.palette = Some(palette.to_vec());
    match style {
        Some(style) => {
            prefs.width = Some(style.width);
            prefs.font_size = Some(style.font_size);
        }
        // Untouched sizes are never written (a small capture's defaults must
        // not leak into a 4K session); existing on-disk sizes are dropped too,
        // matching the original single-writer behavior.
        None => {
            prefs.width = None;
            prefs.font_size = None;
        }
    }
    write_all(&prefs);
}

/// Persist the tray hotkey, preserving palette/sizes already on disk.
#[cfg(any(target_os = "macos", windows))]
pub fn save_hotkey(combo: &str) {
    let mut prefs = load();
    prefs.hotkey = Some(combo.to_string());
    write_all(&prefs);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_roundtrip() {
        let c = Color32::from_rgba_unmultiplied(0xe0, 0x2d, 0x2d, 0xff);
        assert_eq!(parse_color(&format_color(c)), Some(c));
        assert_eq!(parse_color("nonsense"), None);
        assert_eq!(parse_color("e02d2d"), None); // rgb without alpha
    }
}
