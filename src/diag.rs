//! Verbose startup/capture diagnostics. Appends timestamped lines to
//! `~/Desktop/scrannotate-debug.log` (and mirrors them to stderr) so a crash on
//! another machine shows exactly how far launch got. scrannotate runs as
//! several short-lived processes (tray → capture / settings); they all write to
//! the same file, each line tagged with the process role and pid, so the log
//! reads as one timeline across them.
//!
//! Best-effort: logging never fails a launch. The primary process truncates the
//! log so each session starts fresh; spawned children carry `SCRANNOTATE_CHILD`
//! and append instead.

use std::io::Write;
use std::sync::OnceLock;

/// Env var the tray sets on the processes it spawns, so they append to (rather
/// than truncate) the primary's log.
pub const CHILD_ENV: &str = "SCRANNOTATE_CHILD";

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

/// Empty the log unless this is a spawned child. Call once, early in the
/// primary process, so each session's log starts fresh.
pub fn init(tag: &str) {
    set_tag(tag);
    install_panic_hook();
    if std::env::var_os(CHILD_ENV).is_none() {
        let _ = std::fs::write(path(), b"");
    }
    log(&format!(
        "process start — pid={} exe={:?} args={:?}",
        std::process::id(),
        std::env::current_exe().ok(),
        std::env::args().skip(1).collect::<Vec<_>>(),
    ));
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

/// Record panics to the log before the process dies (winit aborts rather than
/// unwinds on a panic from its callbacks, but the hook still runs first).
fn install_panic_hook() {
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
