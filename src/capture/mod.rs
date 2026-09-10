//! Screen capture: one monitor per shot, raw frames via pinray — no image
//! encoding or disk round trip, so it's fast. Every platform runs the same
//! one-shot session ([`session`]); what differs is how `--screen N` finds
//! its monitor:
//!
//! - Linux under Wayland ([`screencast`]): the ScreenCast portal never lets
//!   an app pick a monitor programmatically, so slots are *grants* — the
//!   first use of `--screen N` shows the desktop's chooser once and binds
//!   the pick to that number (its own restore token); every later run is
//!   instant and silent. `--pick-screen` re-binds a slot.
//! - X11/macOS/Windows ([`monitor`]): displays are enumerable, so slot N is
//!   simply the Nth display, primary first. `--pick-screen` lists them.
//!
//! Which flow a Linux session gets is decided by [`is_wayland_session`],
//! the same test pinray's `Auto` backend policy applies — so the flow here
//! always matches the backend pinray actually resolves.
//!
//! The `capture` cargo feature (on by default) carries the pinray
//! dependency — on Linux that means libpipewire-0.3-dev at build time;
//! macOS and Windows need no system packages. Without it only `--from-file`
//! works.

#[cfg(all(feature = "capture", any(target_os = "linux", target_os = "macos", windows)))]
mod monitor;
#[cfg(all(feature = "capture", target_os = "linux"))]
mod screencast;
#[cfg(all(feature = "capture", any(target_os = "linux", target_os = "macos", windows)))]
mod session;

use anyhow::Result;
use image::RgbaImage;

// Only the stub reads nothing from it in a capture-less dev build.
#[cfg_attr(not(feature = "capture"), allow(dead_code))]
pub struct CaptureOptions {
    /// Include the mouse cursor in the frame.
    pub cursor: bool,
    /// Linux/Wayland only: re-open the portal's chooser to re-bind this
    /// screen slot. (Where displays are enumerable — X11, macOS, Windows —
    /// `--pick-screen` is answered in `main` via [`screen_list`] instead of
    /// reaching `capture`.)
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub pick_screen: bool,
    /// 1-based screen slot.
    pub screen: u32,
}

/// A captured frame plus — where the platform can know it — the identity of
/// the monitor it came from, so the editor window can open on that monitor.
pub struct Capture {
    pub image: RgbaImage,
    /// `None` on Wayland: the portal never says which monitor the user
    /// picked, and the compositor owns window placement there anyway.
    pub display: Option<DisplayInfo>,
}

/// Identity of the captured monitor — exactly what each platform's
/// window-placement matcher needs (`app::ScreencapApp::place_window`).
pub struct DisplayInfo {
    /// macOS: the CGDirectDisplayID as a decimal string, matched against
    /// winit's `MonitorHandle::native_id`.
    #[cfg(target_os = "macos")]
    pub id: String,
    /// Windows: `\\.\DISPLAYn`; X11: the RandR monitor name (e.g. `DP-1`).
    /// Both matched against winit's `MonitorHandle::name`.
    #[cfg(any(windows, target_os = "linux"))]
    pub name: String,
    /// Physical size — the fallback match if names disagree.
    #[cfg(any(windows, target_os = "linux"))]
    pub width: u32,
    #[cfg(any(windows, target_os = "linux"))]
    pub height: u32,
}

/// Whether this is a Wayland session — the same test pinray's `Auto`
/// backend policy uses (`XDG_SESSION_TYPE=wayland` or `WAYLAND_DISPLAY`
/// set), so the capture flow chosen here can never disagree with the
/// backend pinray resolves.
#[cfg(target_os = "linux")]
pub fn is_wayland_session() -> bool {
    std::env::var_os("XDG_SESSION_TYPE")
        .and_then(|value| value.into_string().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("wayland"))
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

#[cfg(all(feature = "capture", target_os = "linux"))]
pub fn capture(opts: &CaptureOptions) -> Result<Capture> {
    // Only Wayland needs the portal's chooser-and-grant flow; an X11
    // session enumerates displays like macOS/Windows (pinray resolves its
    // native X11 backend there, where "auto" would always mean the primary
    // monitor and the portal chooser never appears).
    if is_wayland_session() {
        screencast::capture_screenshot(opts.cursor, opts.pick_screen, opts.screen)
    } else {
        monitor::capture_screenshot(opts.cursor, opts.screen)
    }
}

#[cfg(all(feature = "capture", any(target_os = "macos", windows)))]
pub fn capture(opts: &CaptureOptions) -> Result<Capture> {
    monitor::capture_screenshot(opts.cursor, opts.screen)
}

#[cfg(not(all(feature = "capture", any(target_os = "linux", target_os = "macos", windows))))]
pub fn capture(_opts: &CaptureOptions) -> Result<Capture> {
    anyhow::bail!(
        "this build has no capture backend (the `capture` feature is off); \
         only --from-file works"
    )
}

/// `--pick-screen` on X11/macOS/Windows: the numbered display list.
#[cfg(all(feature = "capture", any(target_os = "linux", target_os = "macos", windows)))]
pub fn screen_list() -> Result<String> {
    monitor::screen_list()
}

#[cfg(all(not(feature = "capture"), any(target_os = "linux", target_os = "macos", windows)))]
pub fn screen_list() -> Result<String> {
    anyhow::bail!("this build has no capture backend (the `capture` feature is off)")
}
