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
mod platform_files;
mod prefs;
mod ui;
mod view;

use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use clap::Parser;

/// Screenshot + annotation tool. Captures one screen per shot as a raw
/// frame (no encoding, no disk round-trip — fast) and edits it in place.
/// Screens are numbered: on Linux/Wayland the first use of a number shows
/// the portal's chooser once and remembers your pick; on X11, macOS, and
/// Windows numbers simply follow the display list (--pick-screen shows it).
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Print version and packaging feature information, then exit.
    #[arg(long)]
    build_info: bool,

    /// Report startup errors only on stderr, including when all streams are redirected.
    #[arg(long)]
    no_dialogs: bool,

    /// Which screen to capture. Wayland: the first use of a number asks
    /// you to pick the monitor it means (the grant persists).
    /// X11/macOS/Windows: the Nth display, primary first.
    #[arg(long, value_name = "N", default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    screen: u32,

    /// Wayland: re-open the monitor chooser to re-bind this screen slot.
    /// X11/macOS/Windows: list the numbered screens and exit.
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

    /// Annotate an existing image instead of capturing the screen. Mac App
    /// Store builds cannot read arbitrary paths; there the Open PNG panel
    /// starts at this location and the file must be picked explicitly.
    #[arg(long, value_name = "PATH")]
    from_file: Option<PathBuf>,

    /// Choose a PNG with the native file dialog (Windows/macOS).
    #[arg(long = "open", conflicts_with_all = ["from_file", "pick_screen"])]
    open_image: bool,
}

fn default_output_dir() -> PathBuf {
    // Known Folders on Windows includes redirected/OneDrive Pictures.
    #[cfg(any(target_os = "macos", windows))]
    if let Some(pictures) = dirs::picture_dir() {
        return pictures.join("Screenshots");
    }
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
    let colors = [
        [210u8, 90, 90],
        [90, 140, 210],
        [120, 180, 95],
        [205, 160, 80],
    ];
    for i in 0..24u32 {
        let x = 545 + i * 19;
        fill(
            x,
            414 + (i % 3) * 5,
            x + 13,
            452 - (i % 2) * 7,
            colors[(i % 4) as usize],
        );
    }
    fill(540, 560, 1330, 700, [222, 228, 238]); // panel
    fill(1130, 760, 1350, 830, [47, 82, 224]); // primary button
    fill(1190, 786, 1290, 804, [235, 240, 252]); // button label bar
    img
}

fn main() -> std::process::ExitCode {
    // The windows-subsystem binary detaches from any console; reattach to
    // the parent's so --help/--pick-screen/save-path output still shows
    // when run from a terminal (a no-op under a hotkey/shortcut launch).
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
        AttachConsole(ATTACH_PARENT_PROCESS);
    }

    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            if err.use_stderr() {
                let dialogs = !std::env::args_os().any(|arg| arg == "--no-dialogs");
                report_startup_error(&err.to_string(), dialogs);
                return std::process::ExitCode::FAILURE;
            }
            let _ = err.print();
            return std::process::ExitCode::SUCCESS;
        }
    };
    let dialogs = !cli.no_dialogs;
    match run(cli) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            report_startup_error(&format!("{err:#}"), dialogs);
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(any(target_os = "macos", windows))]
fn has_terminal() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
        || std::io::stdout().is_terminal()
        || std::io::stderr().is_terminal()
}

fn report_startup_error(message: &str, dialogs: bool) {
    eprintln!("scrannotate: {message}");
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = dialogs;
    #[cfg(any(target_os = "macos", windows))]
    {
        // A console user already has the complete diagnostic. Finder/Start
        // launches need a native dialog, even if the graphics renderer failed.
        if dialogs && !has_terminal() {
            rfd::MessageDialog::new()
                .set_title("scrannotate could not start")
                .set_description(message)
                .set_level(rfd::MessageLevel::Error)
                .show();
        }
    }
}

/// Finder/Start launches must offer a way forward when capture permission or
/// hardware initialization fails. CLI launches keep their ordinary error exit.
fn capture_with_recovery(
    options: &capture::CaptureOptions,
    dialogs: bool,
) -> Result<Option<(capture::Capture, bool)>> {
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = dialogs;
        capture::capture(options).map(|capture| Some((capture, false)))
    }

    #[cfg(any(target_os = "macos", windows))]
    loop {
        match capture::capture(options) {
            Ok(capture) => return Ok(Some((capture, false))),
            Err(error) => {
                if !dialogs || has_terminal() {
                    return Err(error);
                }
                let guidance = if cfg!(target_os = "macos") {
                    "Check Screen Recording permission in System Settings → Privacy & Security. After changing permission, you may need to quit and reopen Scrannotate.\n\n"
                } else {
                    "Check that screen capture is allowed and the selected display is connected.\n\n"
                };
                let choice = rfd::MessageDialog::new()
                        .set_title("Scrannotate could not capture the screen")
                        .set_description(format!(
                            "{error:#}\n\n{guidance}Choose Yes to retry, No to open a PNG image, or Cancel to quit."
                        ))
                        .set_level(rfd::MessageLevel::Error)
                        .set_buttons(rfd::MessageButtons::YesNoCancel)
                        .show();
                match choice {
                    rfd::MessageDialogResult::Yes => continue,
                    rfd::MessageDialogResult::No => {
                        return Ok(platform_files::open_image()?.map(|image| {
                            (
                                capture::Capture {
                                    image,
                                    display: None,
                                },
                                true,
                            )
                        }));
                    }
                    _ => return Ok(None),
                }
            }
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    if cli.build_info {
        println!("version={}", env!("CARGO_PKG_VERSION"));
        #[cfg(all(target_os = "macos", feature = "mac-app-store"))]
        println!("SCRANNOTATE_MAC_APP_STORE_BUILD=1");
        #[cfg(not(all(target_os = "macos", feature = "mac-app-store")))]
        println!("SCRANNOTATE_MAC_APP_STORE_BUILD=0");
        println!(
            "privacy_url={}",
            option_env!("SCRANNOTATE_PRIVACY_URL").unwrap_or("")
        );
        return Ok(());
    }
    // Docs/dev hook: SCRANNOTATE_DEMO renders a canned scene (pair with
    // SCRANNOTATE_SHOT to save a window screenshot and exit). Read once and
    // passed down so the two layers can't disagree about demo mode.
    let demo_mode = std::env::var("SCRANNOTATE_DEMO").ok();
    let demo = demo_mode.is_some();

    // Where monitors are enumerable, --pick-screen is a listing, not a
    // chooser: screen numbers are deterministic, so show what they mean.
    // On Linux that's an X11 session; under Wayland the flag instead
    // re-opens the portal chooser inside capture().
    #[cfg(any(target_os = "macos", windows))]
    let enumerable = true;
    #[cfg(target_os = "linux")]
    let enumerable = !capture::is_wayland_session();
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    if enumerable && cli.pick_screen && cli.from_file.is_none() && !demo {
        print!("{}", capture::screen_list()?);
        return Ok(());
    }

    let mut select_full = cli.from_file.is_some() || cli.open_image;
    let (img, display) = match &cli.from_file {
        // A CLI path grants no access under the App Sandbox: opening it
        // directly fails with a permission error. Treat it as a starting
        // location for the Open PNG panel and require an explicit pick.
        #[cfg(all(target_os = "macos", feature = "mac-app-store"))]
        Some(path) => {
            eprintln!(
                "This Mac App Store build cannot open {} directly; choose it in the Open PNG panel.",
                path.display()
            );
            let start = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty());
            match platform_files::open_image(start)? {
                Some(image) => (image, None),
                None => return Ok(()),
            }
        }
        #[cfg(not(all(target_os = "macos", feature = "mac-app-store")))]
        Some(path) => (
            image::open(path)
                .with_context(|| format!("opening {}", path.display()))?
                .to_rgba8(),
            None,
        ),
        None if cli.open_image => match platform_files::open_image(None)? {
            Some(image) => (image, None),
            None => return Ok(()),
        },
        None if demo => (demo_base(), None),
        None => {
            if cli.delay > 0 {
                std::thread::sleep(std::time::Duration::from_secs(cli.delay));
            }
            let Some((capture::Capture { image, display }, opened_file)) = capture_with_recovery(
                &capture::CaptureOptions {
                    cursor: cli.cursor,
                    pick_screen: cli.pick_screen,
                    screen: cli.screen,
                },
                !cli.no_dialogs,
            )?
            else {
                return Ok(());
            };
            select_full = opened_file;
            (image, display)
        }
    };
    let out_dir = cli.save_path.unwrap_or_else(default_output_dir);

    // Everything happens in one fullscreen frozen-frame view. Fresh captures
    // start with no region (drag one out; Enter still copies the whole
    // screen); --from-file images open with everything selected so the
    // toolbar is up immediately.

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
    let mut options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
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
            Ok(Box::new(app::ScreencapApp::new(
                img,
                out_dir,
                select_full,
                demo_mode,
                display,
            )))
        }),
    )
    .map_err(|err| anyhow!("running ui: {err}"))
}
