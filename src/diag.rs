//! Lightweight startup diagnostics. Appends timestamped lines to
//! `~/Desktop/scrannotate-debug.log` (and mirrors them to stderr) so a crash on
//! someone else's machine shows exactly how far launch got. scrannotate runs as
//! several short-lived processes (tray → capture / flash / settings); they all
//! write to the same file, each line tagged with the process role and pid, so
//! the log reads as one timeline across them.
//!
//! This is deliberately dependency-free and best-effort: logging never fails a
//! launch. Remove or gate it once the field crash is understood.

use std::io::Write;
use std::sync::OnceLock;

static TAG: OnceLock<String> = OnceLock::new();

/// Set this process's role (e.g. "tray", "capture"). First call wins.
pub fn set_tag(tag: &str) {
    let _ = TAG.set(tag.to_owned());
}

/// Where the log is written: the Desktop if it exists, else home, else temp.
pub fn path() -> std::path::PathBuf {
    if let Some(home) = std::env::home_dir() {
        let desktop = home.join("Desktop");
        if desktop.is_dir() {
            return desktop.join("scrannotate-debug.log");
        }
        return home.join("scrannotate-debug.log");
    }
    std::env::temp_dir().join("scrannotate-debug.log")
}

/// Empty the log — called once by the primary launch so each session is fresh.
pub fn truncate() {
    let _ = std::fs::write(path(), b"");
}

/// Append a line to the log and mirror it to stderr.
pub fn log(msg: &str) {
    let tag = TAG.get().map(String::as_str).unwrap_or("?");
    let line = format!(
        "{} [{tag}:{}] {msg}\n",
        chrono::Local::now().format("%H:%M:%S%.3f"),
        std::process::id()
    );
    eprint!("{line}");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path()) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Install a panic hook that records the panic to the log before the process
/// dies. winit aborts (rather than unwinds) on a panic from its callbacks, but
/// the hook still runs first, so the last thing in the log is the panic.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log(&format!("PANIC: {info}"));
        previous(info);
    }));
}

/// `diag!("...", args)` — format and append one diagnostic line.
#[macro_export]
macro_rules! diag {
    ($($arg:tt)*) => { $crate::diag::log(&format!($($arg)*)) };
}
