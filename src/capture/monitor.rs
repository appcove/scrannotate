//! Direct display capture for macOS, Windows, and Linux/X11: unlike the
//! Wayland portal, these platforms let an app enumerate monitors and pick
//! one, so `--screen N` is simply the Nth display in [`screen_list`] order —
//! no chooser dialog, no persisted grant. On macOS the first capture
//! triggers the system's Screen Recording permission flow (pinray requests
//! it; the grant is attributed to whatever app launched the process). On
//! X11 pinray reads the screen directly (RandR monitors, polled GetImage).

use std::time::Duration;

use anyhow::{Context, Result, bail};
use pinray::{CaptureSource, DisplaySource, VideoCaptureTarget};

use super::{Capture, DisplayInfo, session};

#[cfg(target_os = "macos")]
const ENUM_CONTEXT: &str = "enumerating displays (macOS requires the Screen Recording \
     permission — System Settings → Privacy & Security → Screen & System Audio Recording)";
#[cfg(not(target_os = "macos"))]
const ENUM_CONTEXT: &str = "enumerating displays";

/// All displays, primary first (where the platform reports one), then OS
/// enumeration order. This ordering defines what `--screen N` means.
fn displays() -> Result<Vec<DisplaySource>> {
    let sources = pinray::enumerate_sources().context(ENUM_CONTEXT)?;
    let mut displays: Vec<DisplaySource> = sources
        .into_iter()
        .filter_map(|source| match source {
            CaptureSource::Display(display) => Some(display),
            _ => None,
        })
        .collect();
    if displays.is_empty() {
        bail!("no displays found");
    }
    // sort_by_key is stable: primary first, everything else keeps OS order.
    displays.sort_by_key(|display| !display.is_primary);
    Ok(displays)
}

/// The numbered display list `--screen N` indexes into (`--pick-screen`).
pub fn screen_list() -> Result<String> {
    use std::fmt::Write;

    let mut out = String::new();
    for (i, display) in displays()?.iter().enumerate() {
        let _ = write!(out, "{}: {} ({}x{})", i + 1, display.name, display.width, display.height);
        if display.is_primary {
            out.push_str("  [primary]");
        }
        out.push('\n');
    }
    Ok(out)
}

/// Grab one frame from the display bound to screen `slot` (1-based).
pub fn capture_screenshot(embed_cursor: bool, slot: u32) -> Result<Capture> {
    let mut displays = displays()?;
    let count = displays.len();
    let index = usize::try_from(slot.saturating_sub(1)).unwrap_or(usize::MAX);
    if index >= count {
        bail!(
            "screen {slot} does not exist — {count} display{} available (--pick-screen lists them)",
            if count == 1 { "" } else { "s" }
        );
    }
    let display = displays.swap_remove(index);

    let mut session = session::builder(embed_cursor)
        .video_target(VideoCaptureTarget::Display(display.id.clone()))
        .build()
        .context("building capture session")?;
    session.start().context("starting capture")?;
    let image = session::take_frame(&mut session, Duration::from_millis(150));
    session.stop().ok();

    Ok(Capture {
        image: image?,
        display: Some(DisplayInfo {
            #[cfg(target_os = "macos")]
            id: display.id.0,
            #[cfg(any(windows, target_os = "linux"))]
            name: display.name,
            #[cfg(any(windows, target_os = "linux"))]
            width: display.width,
            #[cfg(any(windows, target_os = "linux"))]
            height: display.height,
        }),
    })
}
