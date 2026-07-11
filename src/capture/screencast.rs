//! Single-monitor frame capture via pinray (ScreenCast portal + PipeWire):
//! raw RGBA frames over shared memory, no encoding, no disk.
//!
//! Each screen *slot* persists its own portal grant (restore token), so
//! `--screen N` shows the chooser once per slot and is silent after that.
//!
//! pinray 0.2.4 requests a persistent portal grant and receives a restore
//! token back, but only logs it (`tracing::info!(restore_token = ...)`)
//! instead of exposing it in its API. `TokenCatcher` is a tracing layer that
//! intercepts that event so we can persist the token and skip the GNOME
//! permission dialog on subsequent runs. Remove once pinray exposes the token.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use image::RgbaImage;
use pinray::{
    CaptureEvent, CaptureSession, CursorMode, FrameData, PixelFormat, SourceId, VideoCaptureTarget,
};
use tracing::field::{Field, Visit};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;

fn token_path(slot: u32) -> Option<PathBuf> {
    // Slot 1 keeps the historical name so existing grants stay bound.
    let name = if slot <= 1 {
        "restore_token".to_owned()
    } else {
        format!("restore_token_{slot}")
    };
    Some(crate::prefs::state_dir()?.join(name))
}

fn load_token(slot: u32) -> Option<String> {
    let path = token_path(slot)?;
    let token = std::fs::read_to_string(&path)
        .ok()
        .or_else(|| {
            if slot > 1 {
                return None;
            }
            // Migrate from the pre-rename state dir (screencap).
            let legacy = path.parent()?.parent()?.join("screencap/restore_token");
            std::fs::read_to_string(legacy).ok()
        })?;
    let token = token.trim().to_owned();
    (!token.is_empty()).then_some(token)
}

fn store_token(slot: u32, token: &str) {
    let Some(path) = token_path(slot) else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(err) = std::fs::write(&path, token) {
        eprintln!("warning: could not persist portal restore token: {err}");
    }
}

#[derive(Default)]
struct TokenVisitor {
    token: Option<String>,
}

impl Visit for TokenVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "restore_token" {
            self.token = Some(value.to_owned());
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "restore_token" {
            self.token = Some(format!("{value:?}").trim_matches('"').to_owned());
        }
    }
}

struct TokenCatcher {
    slot: Arc<Mutex<Option<String>>>,
}

impl<S: tracing::Subscriber> Layer<S> for TokenCatcher {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut visitor = TokenVisitor::default();
        event.record(&mut visitor);
        if let Some(token) = visitor.token {
            *self.slot.lock().expect("token slot poisoned") = Some(token);
        }
    }
}

/// Grab one frame from the display bound to screen `slot`. The first use of
/// a slot shows the portal's monitor chooser and persists the grant under
/// that slot's restore token; pass `pick_screen` to re-open the chooser and
/// re-bind the slot. Blocks until the user answers the dialog.
pub fn capture_screenshot(embed_cursor: bool, pick_screen: bool, slot: u32) -> Result<RgbaImage> {
    let caught = Arc::new(Mutex::new(None));
    let subscriber = tracing_subscriber::registry().with(TokenCatcher { slot: caught.clone() });
    let _guard = tracing::subscriber::set_default(subscriber);

    let mut builder = CaptureSession::builder()
        .video_target(VideoCaptureTarget::Display(SourceId::new("auto")))
        .pixel_format(PixelFormat::Rgba8888)
        // Single-shot capture: pinray copies every delivered frame into a
        // fresh buffer (~44 MB at 5K), so its default 60 fps would churn
        // gigabytes through the allocator during the settle window — enough
        // to get the process OOM-killed. 10 fps is plenty to outwait the
        // share dialog's fade-out.
        .frame_rate(Some(10))
        .cursor_mode(if embed_cursor { CursorMode::Embedded } else { CursorMode::Hidden });
    let mut used_token = false;
    if !pick_screen && let Some(token) = load_token(slot) {
        builder = builder.restore_token(token);
        used_token = true;
    }

    let mut session = builder.build().context("building capture session")?;
    session.start().context(
        "starting capture (is the app running with access to the session D-Bus and PipeWire?)",
    )?;

    // The stream's first frames can still show the portal's share dialog
    // fading out, so keep pulling briefly and take the freshest frame —
    // longer when the dialog was actually shown.
    let settle = if used_token {
        Duration::from_millis(150)
    } else {
        Duration::from_millis(600)
    };
    let frame = wait_for_frame(&mut session, settle);
    session.stop().ok();

    if let Some(token) = caught.lock().expect("token slot poisoned").take() {
        store_token(slot, &token);
    }

    let frame = frame?;
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
    // Generous timeout: the first event waits on portal negotiation.
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
            // Timeouts while the permission dialog is open are expected.
            Err(pinray::PinrayError::Timeout(_)) => {}
            Err(err) => return Err(err).context("waiting for a frame"),
        }
    }
    latest.context("timed out waiting for a captured frame")
}
