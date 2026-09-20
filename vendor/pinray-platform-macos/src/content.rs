use std::sync::{Arc, Condvar, Mutex};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_foundation::NSError;
use objc2_screen_capture_kit::{SCDisplay, SCShareableContent};
use objc2_core_graphics::{CGDisplayCopyDisplayMode, CGDisplayMode, CGMainDisplayID};

use pinray_core::{CaptureSource, DisplaySource, PinrayError, Result, SourceId, WindowSource};

/// Synchronously retrieve `SCShareableContent` by blocking on the async completion handler.
// The completion handler runs on an SCKit-owned queue, so the slot does cross
// threads; `Retained<SCShareableContent>` lacks Send/Sync markers but the
// Mutex serializes all access and the value is only consumed after the
// handler finishes.
#[allow(clippy::arc_with_non_send_sync)]
pub fn get_shareable_content() -> Result<Retained<SCShareableContent>> {
    let slot: Arc<Mutex<Option<Result<Retained<SCShareableContent>>>>> = Arc::new(Mutex::new(None));
    let cv = Arc::new(Condvar::new());

    let slot2 = Arc::clone(&slot);
    let cv2 = Arc::clone(&cv);

    // RcBlock::new itself is safe; the ObjC call below is unsafe.
    let block = RcBlock::new(
        move |content_ptr: *mut SCShareableContent, error_ptr: *mut NSError| {
            let outcome = if !error_ptr.is_null() {
                let msg = unsafe { &*error_ptr }.localizedDescription().to_string();
                Err(PinrayError::Platform(format!(
                    "SCShareableContent failed: {msg}"
                )))
            } else if !content_ptr.is_null() {
                unsafe {
                    Retained::retain(content_ptr).ok_or_else(|| {
                        PinrayError::Platform("SCShareableContent retain returned nil".into())
                    })
                }
            } else {
                Err(PinrayError::Platform("SCShareableContent is nil".into()))
            };

            *slot2.lock().unwrap() = Some(outcome);
            cv2.notify_one();
        },
    );

    unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&block) };

    let mut guard = cv
        .wait_while(slot.lock().unwrap(), |v| v.is_none())
        .unwrap();
    guard.take().unwrap()
}

/// Enumerate capture sources via `SCShareableContent`.
pub fn enumerate_sources() -> Result<Vec<CaptureSource>> {
    let content = get_shareable_content()?;
    let mut sources = Vec::new();

    let displays = unsafe { content.displays() };
    for display in displays.iter() {
        let id = unsafe { display.displayID() };
        let (width, height, scale_factor_milli) = display_dimensions(&display);

        sources.push(CaptureSource::Display(DisplaySource {
            id: SourceId::new(id.to_string()),
            name: format!("Display {id}"),
            width,
            height,
            scale_factor_milli,
            is_primary: id == CGMainDisplayID(),
        }));
    }

    let windows = unsafe { content.windows() };
    for window in windows.iter() {
        let win_id = unsafe { window.windowID() };
        let title = unsafe { window.title() }
            .map(|s| s.to_string())
            .unwrap_or_default();
        let app_name = unsafe { window.owningApplication() }
            .map(|app| unsafe { app.applicationName() }.to_string());

        if title.is_empty() && app_name.is_none() {
            continue;
        }

        sources.push(CaptureSource::Window(WindowSource {
            id: SourceId::new(win_id.to_string()),
            title,
            app_name,
        }));
    }

    Ok(sources)
}

/// ScreenCaptureKit reports logical dimensions. Derive the actual backing
/// scale from the selected display mode rather than assuming a Retina screen.
/// Multiplying the SCDisplay dimensions also preserves its orientation.
pub(crate) fn display_dimensions(display: &SCDisplay) -> (u32, u32, u32) {
    let scale = CGDisplayCopyDisplayMode(unsafe { display.displayID() })
        .map(|mode| {
            let logical = CGDisplayMode::width(Some(&mode));
            let pixels = CGDisplayMode::pixel_width(Some(&mode));
            if logical == 0 || pixels == 0 { 1.0 } else { pixels as f64 / logical as f64 }
        })
        .unwrap_or(1.0);
    let width = (unsafe { display.width() } as f64 * scale).round().max(1.0) as u32;
    let height = (unsafe { display.height() } as f64 * scale).round().max(1.0) as u32;
    (width, height, (scale * 1000.0).round() as u32)
}
