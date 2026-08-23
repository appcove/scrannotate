//! System hotkey setup. scrannotate is a one-shot tool — a keypress should
//! *launch* it (there is no resident process to receive a hotkey), so the
//! binding lives in the OS, not in the app. This module turns a
//! user-chosen combination like `Ctrl+Shift+4` into the right binding for
//! the current platform:
//!
//! - GNOME (X11 or Wayland): installed automatically via `gsettings` — the
//!   one desktop where a global shortcut is cleanly scriptable, and the way
//!   Wayland allows it at all (the compositor owns the grab, not the app).
//! - Everywhere else: printed as exact, copy-paste setup for the chosen
//!   combo, using each platform's own reliable mechanism.
//!
//! Parsing and per-platform rendering are pure (unit-tested below); only the
//! GNOME install shells out.

// `Command` and the `anyhow!` macro are only used by the GNOME install path.
#[cfg(all(unix, not(target_os = "macos")))]
use std::process::Command;

use anyhow::{Result, bail};

/// A parsed key combination: modifier flags plus one non-modifier key.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Combo {
    ctrl: bool,
    shift: bool,
    alt: bool,
    /// The Command/Super/Windows/Meta key (one flag; the name differs per OS).
    meta: bool,
    /// The non-modifier key, normalized: single letters lowercased, named
    /// keys title-cased (`Print`, `Space`, `F5`).
    key: String,
}

impl Combo {
    /// Parse `Ctrl+Shift+S`, `Cmd+Shift+4`, `Super+Print`, etc. Modifier
    /// aliases are generous; the last non-modifier token is the key.
    pub fn parse(s: &str) -> Result<Combo> {
        let mut c = Combo { ctrl: false, shift: false, alt: false, meta: false, key: String::new() };
        for raw in s.split('+') {
            let tok = raw.trim();
            if tok.is_empty() {
                continue;
            }
            match tok.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "ctl" | "^" => c.ctrl = true,
                "shift" | "⇧" => c.shift = true,
                "alt" | "opt" | "option" | "⌥" => c.alt = true,
                "cmd" | "command" | "⌘" | "super" | "win" | "windows" | "meta" | "⊞" => c.meta = true,
                _ => {
                    if !c.key.is_empty() {
                        bail!("more than one non-modifier key in '{s}'");
                    }
                    c.key = normalize_key(tok);
                }
            }
        }
        if c.key.is_empty() {
            bail!("no key in '{s}' (e.g. Ctrl+Shift+S)");
        }
        if !(c.ctrl || c.shift || c.alt || c.meta) {
            bail!("'{s}' has no modifier — a bare key makes a poor global hotkey");
        }
        Ok(c)
    }

    // The three renderers below are each only called on their own platform's
    // build, but all are exercised by the tests — so each looks "dead" to the
    // bin build of the other platforms.
    /// GNOME accelerator syntax, e.g. `<Control><Shift>s` or `<Super>Print`.
    #[allow(dead_code)]
    fn gnome_accel(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str("<Control>");
        }
        if self.alt {
            s.push_str("<Alt>");
        }
        if self.shift {
            s.push_str("<Shift>");
        }
        if self.meta {
            s.push_str("<Super>");
        }
        // GNOME keysyms: single letters are lowercase; named keys keep case.
        if self.key.chars().count() == 1 {
            s.push_str(&self.key.to_ascii_lowercase());
        } else {
            s.push_str(&self.key);
        }
        s
    }

    /// macOS glyphs, e.g. `⌘⇧4`.
    #[allow(dead_code)]
    fn mac_glyphs(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push('⌃');
        }
        if self.alt {
            s.push('⌥');
        }
        if self.shift {
            s.push('⇧');
        }
        if self.meta {
            s.push('⌘');
        }
        // macOS renders shortcut letters uppercase (⇧S), unlike the keysym
        // strings the other platforms want.
        if self.key.chars().count() == 1 {
            s.push_str(&self.key.to_ascii_uppercase());
        } else {
            s.push_str(&self.key);
        }
        s
    }

    /// AutoHotkey v2 hotkey syntax, e.g. `^+s` (Ctrl+Shift+S).
    #[allow(dead_code)]
    fn ahk(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push('^');
        }
        if self.alt {
            s.push('!');
        }
        if self.shift {
            s.push('+');
        }
        if self.meta {
            s.push('#');
        }
        if self.key.chars().count() == 1 {
            s.push_str(&self.key.to_ascii_lowercase());
        } else {
            s.push_str(&self.key);
        }
        s
    }
}

// Helpers for the resident tray (tray.rs), which needs the combo in
// global-hotkey's parser format and a human label for the menu.
#[cfg(any(target_os = "macos", windows))]
impl Combo {
    /// global-hotkey's `FromStr` format, e.g. `CONTROL+SHIFT+S`.
    pub fn accelerator(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.ctrl {
            parts.push("CONTROL".into());
        }
        if self.alt {
            parts.push("ALT".into());
        }
        if self.shift {
            parts.push("SHIFT".into());
        }
        if self.meta {
            parts.push("SUPER".into());
        }
        // Our "Print" normalizes to global-hotkey's "PRINTSCREEN"; letters and
        // digits pass through uppercased (KeyS accepts "S", Digit4 accepts "4").
        parts.push(if self.key == "Print" { "PRINTSCREEN".into() } else { self.key.to_uppercase() });
        parts.join("+")
    }

    /// A compact label for the tray menu (glyphs on macOS, text on Windows).
    pub fn label(&self) -> String {
        #[cfg(target_os = "macos")]
        {
            self.mac_glyphs()
        }
        #[cfg(not(target_os = "macos"))]
        {
            let mut s = String::new();
            for (on, name) in
                [(self.ctrl, "Ctrl"), (self.alt, "Alt"), (self.shift, "Shift"), (self.meta, "Win")]
            {
                if on {
                    s.push_str(name);
                    s.push('+');
                }
            }
            s.push_str(&self.key.to_uppercase());
            s
        }
    }
}

/// Single letters lowercased; multi-char names title-cased so `print`,
/// `PRINT`, and `Print` all normalize alike.
fn normalize_key(tok: &str) -> String {
    if tok.chars().count() == 1 {
        return tok.to_ascii_lowercase();
    }
    let mut chars = tok.chars();
    let first = chars.next().unwrap().to_ascii_uppercase();
    let rest: String = chars.as_str().to_ascii_lowercase();
    // Function keys stay uppercase (F5, not F5->F5 is fine either way).
    format!("{first}{rest}")
}

/// The command a hotkey should launch, for this platform. `current_exe`
/// keeps it working from a dev build or an unusual install path.
fn launch_command(screen: u32) -> String {
    let screen_arg = if screen == 1 { String::new() } else { format!(" --screen {screen}") };
    if cfg!(target_os = "macos") {
        // GUI apps on macOS are launched by bundle, not path, so the
        // Screen-Recording grant sticks to scrannotate's bundle identity.
        format!("open -a scrannotate --args{}", if screen_arg.is_empty() { " --screen 1".into() } else { screen_arg })
    } else {
        let exe = std::env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(str::to_owned))
            .unwrap_or_else(|| "scrannotate".into());
        format!("{exe}{screen_arg}")
    }
}

/// Entry point for `--setup-hotkey`. Installs (GNOME) or prints exact setup.
pub fn setup(combo: &str, screen: u32) -> Result<()> {
    let combo = Combo::parse(combo)?;
    let command = launch_command(screen);

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if is_gnome() {
            return install_gnome(&combo, &command, screen);
        }
        print_linux_other(&combo, &command);
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        print_macos(&combo, &command);
        return Ok(());
    }
    #[cfg(windows)]
    {
        print_windows(&combo, &command);
        return Ok(());
    }
    #[allow(unreachable_code)]
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn is_gnome() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .map(|d| d.to_ascii_uppercase().contains("GNOME"))
        .unwrap_or(false)
}

/// Register a GNOME custom media-key binding via `gsettings`. Best-effort and
/// idempotent: it reuses the `scrannotate<screen>` slot so re-running just
/// updates the combo.
#[cfg(all(unix, not(target_os = "macos")))]
fn install_gnome(combo: &Combo, command: &str, screen: u32) -> Result<()> {
    const BASE: &str = "org.gnome.settings-daemon.plugins.media-keys";
    let path =
        format!("/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/scrannotate{screen}/");
    let schema = format!("{BASE}.custom-keybinding:{path}");

    // 1. Ensure our path is in the list of custom-keybinding paths.
    let current = gsettings_get(BASE, "custom-keybindings")?;
    let mut paths = parse_gvariant_list(&current);
    if !paths.iter().any(|p| p == &path) {
        paths.push(path.clone());
    }
    let list = format!(
        "[{}]",
        paths.iter().map(|p| format!("'{p}'")).collect::<Vec<_>>().join(", ")
    );
    gsettings_set(BASE, "custom-keybindings", &list)?;

    // 2. Fill in this binding's name, command, and accelerator.
    gsettings_set(&schema, "name", "'scrannotate'")?;
    gsettings_set(&schema, "command", &format!("'{command}'"))?;
    gsettings_set(&schema, "binding", &format!("'{}'", combo.gnome_accel()))?;

    println!("Installed GNOME hotkey {} → {command}", combo.gnome_accel());
    println!("(Change or remove it in Settings → Keyboard → Keyboard Shortcuts → Custom Shortcuts.)");
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn gsettings_get(schema: &str, key: &str) -> Result<String> {
    let out = Command::new("gsettings")
        .args(["get", schema, key])
        .output()
        .map_err(|e| anyhow::anyhow!("running gsettings (is GNOME installed?): {e}"))?;
    if !out.status.success() {
        bail!("gsettings get {schema} {key} failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn gsettings_set(schema: &str, key: &str, value: &str) -> Result<()> {
    let status = Command::new("gsettings")
        .args(["set", schema, key, value])
        .status()
        .map_err(|e| anyhow::anyhow!("running gsettings: {e}"))?;
    if !status.success() {
        bail!("gsettings set {schema} {key} failed");
    }
    Ok(())
}

/// Pull quoted paths out of a gsettings list literal like
/// `['/a/', '/b/']` or `@as []`.
#[cfg(all(unix, not(target_os = "macos")))]
fn parse_gvariant_list(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(start) = rest.find('\'') {
        let after = &rest[start + 1..];
        if let Some(end) = after.find('\'') {
            out.push(after[..end].to_owned());
            rest = &after[end + 1..];
        } else {
            break;
        }
    }
    out
}

#[cfg(all(unix, not(target_os = "macos")))]
fn print_linux_other(combo: &Combo, command: &str) {
    println!("scrannotate hotkey: {}", combo.gnome_accel());
    println!();
    println!("Bind this command to that combination in your desktop's keyboard settings:");
    println!("    {command}");
    println!();
    println!("KDE:  System Settings → Shortcuts → add a Custom Shortcut running the command.");
    println!("wlroots (Sway/Hyprland): add a `bindsym`/`bind` line to your config for the command.");
    println!("On Wayland, only the compositor can own a global shortcut — the app cannot.");
}

#[cfg(target_os = "macos")]
fn print_macos(combo: &Combo, command: &str) {
    println!("scrannotate hotkey: {}", combo.mac_glyphs());
    println!();
    println!("macOS has no supported way to register a global launch shortcut from the");
    println!("command line, so set it up once via Shortcuts.app:");
    println!("  1. Shortcuts.app → + (new) → add a 'Run Shell Script' action.");
    println!("  2. Set it to /bin/zsh and paste:  {command}");
    println!("  3. Rename it (e.g. 'Scrannotate'), open its ⓘ Details,");
    println!("     enable 'Use as Quick Action' → Services.");
    println!("  4. System Settings → Keyboard → Keyboard Shortcuts → Services,");
    println!("     find it under General and assign {}.", combo.mac_glyphs());
}

#[cfg(windows)]
fn print_windows(combo: &Combo, command: &str) {
    println!("scrannotate hotkey: {}", combo.mac_glyphs());
    println!();
    println!("Windows has no built-in global-hotkey-to-launch. Easiest reliable option is");
    println!("AutoHotkey v2 — save this as scrannotate.ahk and run it (or drop it in shell:startup):");
    println!();
    println!("    {}::Run(\"{}\")", combo.ahk(), command.replace('\\', "\\\\"));
    println!();
    println!("Or use PowerToys' Keyboard Manager / a Start-Menu shortcut's 'Shortcut key'");
    println!("property (limited to Ctrl+Alt+<key>).");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_combos() {
        let c = Combo::parse("Ctrl+Shift+S").unwrap();
        assert!(c.ctrl && c.shift && !c.alt && !c.meta);
        assert_eq!(c.key, "s");
        assert_eq!(c.gnome_accel(), "<Control><Shift>s");
        assert_eq!(c.ahk(), "^+s");
    }

    #[test]
    fn modifier_aliases_and_glyphs() {
        let c = Combo::parse("cmd + option + 4").unwrap();
        assert!(c.meta && c.alt);
        assert_eq!(c.mac_glyphs(), "⌥⌘4");
        // Super and Cmd/Win are the same physical modifier.
        assert_eq!(Combo::parse("Super+Print").unwrap(), Combo::parse("Win+print").unwrap());
    }

    #[test]
    fn named_keys_normalize_case() {
        assert_eq!(Combo::parse("Ctrl+PRINT").unwrap().gnome_accel(), "<Control>Print");
        assert_eq!(Combo::parse("Super+space").unwrap().gnome_accel(), "<Super>Space");
    }

    #[test]
    fn rejects_bad_input() {
        assert!(Combo::parse("Ctrl").is_err()); // no key
        assert!(Combo::parse("S").is_err()); // no modifier
        assert!(Combo::parse("Ctrl+A+B").is_err()); // two keys
        assert!(Combo::parse("").is_err());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn parses_gvariant_lists() {
        assert_eq!(parse_gvariant_list("@as []"), Vec::<String>::new());
        assert_eq!(
            parse_gvariant_list("['/org/a/', '/org/b/']"),
            vec!["/org/a/".to_string(), "/org/b/".to_string()]
        );
    }
}
