//! Screen capture: one monitor per shot, raw frames over PipeWire via the
//! ScreenCast portal ([`screencast`]) — no image encoding or disk round
//! trip, so it's fast.
//!
//! Monitors are numbered *slots*: the portal never lets an app pick a
//! monitor programmatically, so the first use of `--screen N` shows the
//! chooser once and binds the pick to that number (its own restore token);
//! every later run is instant and silent. `--pick-screen` re-binds a slot.
//!
//! The `screencast` cargo feature (on by default) carries the PipeWire
//! dependency; without it only `--from-file` works.

#[cfg(feature = "screencast")]
mod screencast;

use anyhow::Result;
use image::RgbaImage;

// Only the stub reads nothing from it in a portal-less dev build.
#[cfg_attr(not(feature = "screencast"), allow(dead_code))]
pub struct CaptureOptions {
    /// Include the mouse cursor in the frame.
    pub cursor: bool,
    /// Re-open the portal's chooser to re-bind this screen slot.
    pub pick_screen: bool,
    /// 1-based screen slot.
    pub screen: u32,
}

#[cfg(feature = "screencast")]
pub fn capture(opts: &CaptureOptions) -> Result<RgbaImage> {
    screencast::capture_screenshot(opts.cursor, opts.pick_screen, opts.screen)
}

#[cfg(not(feature = "screencast"))]
pub fn capture(_opts: &CaptureOptions) -> Result<RgbaImage> {
    anyhow::bail!(
        "this build has no capture backend (the `screencast` feature is off); \
         only --from-file works"
    )
}
