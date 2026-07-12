// A GUI app must not flash a console window on every hotkey launch; main()
// reattaches to a parent terminal so CLI output still works. (Not under
// cfg(test): the test harness needs a console.)
#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

mod annotate;
mod app;
mod capture;
mod clipboard;
mod document;
mod editor;
mod export;
mod prefs;
mod ui;
mod view;

use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use clap::Parser;

/// Screenshot + annotation tool. Captures one screen per shot as a raw
/// frame (no encoding, no disk round-trip — fast) and edits it in place.
/// Screens are numbered: on Linux/Wayland the first use of a number shows
/// the portal's chooser once and remembers your pick; on macOS and Windows
/// numbers simply follow the display list (--pick-screen shows it).
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Which screen to capture. Linux: the first use of a number asks you
    /// to pick the monitor it means (the grant persists). macOS/Windows:
    /// the Nth display, primary first.
    #[arg(long, value_name = "N", default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    screen: u32,

    /// Linux: re-open the monitor chooser to re-bind this screen slot.
    /// macOS/Windows: list the numbered screens and exit.
    #[arg(long)]
    pick_screen: bool,

    /// Include the mouse cursor in the capture.
    #[arg(long)]
    cursor: bool,

    /// Seconds to wait before capturing (to set up menus etc.).
    #[arg(long, default_value_t = 0)]
    delay: u64,

    /// Directory screenshots are saved into.
    #[arg(long, value_name = "DIR")]
    save_path: Option<PathBuf>,

    /// Annotate an existing image instead of capturing the screen.
    #[arg(long, value_name = "PATH")]
    from_file: Option<PathBuf>,
}

fn default_output_dir() -> PathBuf {
    match std::env::home_dir() {
        Some(home) if home.join("Pictures").is_dir() => home.join("Pictures/Screenshots"),
        Some(home) => home.join("Screenshots"),
        None => PathBuf::from("."),
    }
}

/// Synthetic "desktop" used by the docs demo mode (`SCRANNOTATE_DEMO`) so
/// screenshots don't depend on anyone's real screen.
fn demo_base() -> image::RgbaImage {
    let (w, h) = (1600u32, 1000u32);
    let mut img = image::RgbaImage::from_fn(w, h, |_, y| {
        let t = y as f32 / h as f32;
        let v = |a: f32, b: f32| (a + (b - a) * t) as u8;
        image::Rgba([v(38.0, 24.0), v(42.0, 27.0), v(54.0, 36.0), 255])
    });
    let mut fill = |x0: u32, y0: u32, x1: u32, y1: u32, c: [u8; 3]| {
        for y in y0..y1.min(h) {
            for x in x0..x1.min(w) {
                img.put_pixel(x, y, image::Rgba([c[0], c[1], c[2], 255]));
            }
        }
    };
    // A fake application window: title bar, sidebar, content lines, a button.
    fill(200, 120, 1400, 880, [246, 246, 248]); // window
    fill(200, 120, 1400, 168, [228, 228, 232]); // title bar
    fill(222, 136, 238, 152, [224, 82, 82]); // traffic lights
    fill(248, 136, 264, 152, [240, 190, 70]);
    fill(274, 136, 290, 152, [98, 197, 84]);
    fill(200, 168, 470, 880, [236, 236, 240]); // sidebar
    for i in 0..7u32 {
        fill(228, 200 + i * 56, 442, 224 + i * 56, [205, 205, 212]);
    }
    for i in 0..5u32 {
        let y = 220 + i * 64;
        fill(540, y, 1330 - (i % 3) * 160, y + 26, [206, 206, 214]);
    }
    // "Sensitive" strip the demo blurs: colorful glyph-ish blocks so the
    // pixelation reads clearly.
    let colors = [[210u8, 90, 90], [90, 140, 210], [120, 180, 95], [205, 160, 80]];
    for i in 0..24u32 {
        let x = 545 + i * 19;
        fill(x, 414 + (i % 3) * 5, x + 13, 452 - (i % 2) * 7, colors[(i % 4) as usize]);
    }
    fill(540, 560, 1330, 700, [222, 228, 238]); // panel
    fill(1130, 760, 1350, 830, [47, 82, 224]); // primary button
    fill(1190, 786, 1290, 804, [235, 240, 252]); // button label bar
    img
}

fn main() -> Result<()> {
    // The windows-subsystem binary detaches from any console; reattach to
    // the parent's so --help/--pick-screen/save-path output still shows
    // when run from a terminal (a no-op under a hotkey/shortcut launch).
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
        AttachConsole(ATTACH_PARENT_PROCESS);
    }

    let cli = Cli::parse();
    // Docs/dev hook: SCRANNOTATE_DEMO renders a canned scene (pair with
    // SCRANNOTATE_SHOT to save a window screenshot and exit). Read once and
    // passed down so the two layers can't disagree about demo mode.
    let demo_mode = std::env::var("SCRANNOTATE_DEMO").ok();
    let demo = demo_mode.is_some();

    // Where monitors are enumerable, --pick-screen is a listing, not a
    // chooser: screen numbers are deterministic, so show what they mean.
    #[cfg(any(target_os = "macos", windows))]
    if cli.pick_screen && cli.from_file.is_none() && !demo {
        print!("{}", capture::screen_list()?);
        return Ok(());
    }

    let (img, display) = match &cli.from_file {
        Some(path) => (
            image::open(path)
                .with_context(|| format!("opening {}", path.display()))?
                .to_rgba8(),
            None,
        ),
        None if demo => (demo_base(), None),
        None => {
            if cli.delay > 0 {
                std::thread::sleep(std::time::Duration::from_secs(cli.delay));
            }
            let capture::Capture { image, display } = capture::capture(&capture::CaptureOptions {
                cursor: cli.cursor,
                pick_screen: cli.pick_screen,
                screen: cli.screen,
            })?;
            (image, display)
        }
    };
    let out_dir = cli.save_path.unwrap_or_else(default_output_dir);

    // Everything happens in one fullscreen frozen-frame view. Fresh captures
    // start with no region (drag one out; Enter still copies the whole
    // screen); --from-file images open with everything selected so the
    // toolbar is up immediately.
    let select_full = cli.from_file.is_some();

    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_app_id("scrannotate")
        .with_title("scrannotate");
    viewport = if demo {
        // Windowed, deterministic size for docs screenshots; exactly the
        // demo image's size, so the frame fills the canvas edge to edge.
        viewport.with_inner_size([1600.0, 1000.0])
    } else if cfg!(target_os = "macos") {
        // Native macOS fullscreen animates onto its own Space — wrong for an
        // instant screenshot overlay. The window opens undecorated and flips
        // to winit's "simple fullscreen" on its first frame, on the captured
        // monitor (app::ScreencapApp::place_window).
        viewport.with_decorations(false)
    } else {
        // Fullscreen right away; on Windows the first frame may move it to
        // the captured monitor (app::ScreencapApp::place_window).
        viewport.with_fullscreen(true)
    };
    #[allow(unused_mut)]
    let mut options = eframe::NativeOptions { viewport, ..Default::default() };
    #[cfg(target_os = "macos")]
    {
        // Without the default menu bar, Cmd+Q reaches egui's shortcut
        // handling (which saves prefs on close) instead of terminating the
        // process behind eframe's back. The editor has no menus anyway.
        options.event_loop_builder = Some(Box::new(|builder| {
            use winit::platform::macos::EventLoopBuilderExtMacOS;
            builder.with_default_menu(false);
        }));
    }
    eframe::run_native(
        "scrannotate",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(app::ScreencapApp::new(img, out_dir, select_full, demo_mode, display)))
        }),
    )
    .map_err(|err| anyhow!("running ui: {err}"))
}
