//! One frame out of a pinray capture session — the part every platform
//! backend shares: session settings, pulling events briefly past stream
//! start-up, keeping the freshest frame, and converting it to an image.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use image::RgbaImage;
use pinray::{CaptureEvent, CaptureSession, CursorMode, FrameData, PixelFormat, SessionBuilder};

/// Session settings shared by every backend. Single-shot capture: pinray
/// copies every delivered frame into a fresh buffer (~44 MB at 5K), so its
/// default 60 fps would churn gigabytes through the allocator during the
/// settle window — enough to get the process OOM-killed. 10 fps is plenty.
pub fn builder(embed_cursor: bool) -> SessionBuilder {
    let builder = CaptureSession::builder()
        .pixel_format(PixelFormat::Rgba8888)
        .frame_rate(Some(10))
        .cursor_mode(if embed_cursor {
            CursorMode::Embedded
        } else {
            CursorMode::Hidden
        });
    // pinray's Auto backend prefers DXGI, which does not rotate portrait
    // frames or composite a separate hardware cursor. WGC handles those
    // semantics. Do not silently fall back to incorrect output on failure.
    #[cfg(windows)]
    let builder = builder.backend_preference(pinray::BackendPreference::WindowsWgc);
    builder
}

/// Take one frame from a started session: wait for the stream, keep
/// collecting for `settle` after the first frame, and return the freshest
/// as an image. A stream's first frames can be stale (on Wayland they can
/// still show the portal's share dialog fading out), so freshest wins.
pub fn take_frame(session: &mut CaptureSession, settle: Duration) -> Result<RgbaImage> {
    let frame = wait_for_frame(session, settle)?;
    let tight = frame
        .to_tight_bytes()
        .context("frame was not delivered as host memory")?;
    if frame.pixel_format != PixelFormat::Rgba8888 {
        bail!("unexpected pixel format {:?}", frame.pixel_format);
    }
    RgbaImage::from_raw(frame.width, frame.height, tight)
        .context("frame buffer smaller than advertised dimensions")
}

/// Wait for the stream, then keep collecting frames for `settle` after the
/// first one and return the freshest.
fn wait_for_frame(session: &mut CaptureSession, settle: Duration) -> Result<pinray::VideoFrame> {
    // Generous timeout: the first event can wait on portal negotiation or a
    // permission dialog.
    let deadline = std::time::Instant::now() + Duration::from_secs(120);
    let mut latest: Option<pinray::VideoFrame> = None;
    let mut first_seen: Option<std::time::Instant> = None;
    let mut frames = 0u32;
    while std::time::Instant::now() < deadline {
        if let Some(first) = first_seen
            && first.elapsed() >= settle
        {
            break;
        }
        match session.next_event(Some(Duration::from_millis(100))) {
            Ok(CaptureEvent::Video(frame)) => {
                if matches!(frame.data, FrameData::Host(_)) {
                    if first_seen.is_none() {
                        first_seen = Some(std::time::Instant::now());
                    }
                    latest = Some(frame);
                    frames += 1;
                    // Belt and braces: each frame is a full-screen copy, so
                    // never process more than a handful regardless of rate.
                    if frames >= 12 {
                        break;
                    }
                }
            }
            Ok(CaptureEvent::End) => break,
            Ok(_) => {}
            // Timeouts while a permission dialog is open are expected.
            Err(pinray::PinrayError::Timeout(_)) => {}
            Err(err) => return Err(err).context("waiting for a frame"),
        }
    }
    latest.context("timed out waiting for a captured frame")
}
