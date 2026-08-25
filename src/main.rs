// A GUI app must not flash a console window on every hotkey launch; main()
// reattaches to a parent terminal so CLI output still works. (Not under
// cfg(test): the test harness needs a console.)
#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

mod annotate;
mod app;
mod capture;
mod clipboard;
mod diag;
mod document;
mod editor;
mod export;
mod hotkey;
#[cfg(any(target_os = "macos", windows))]
mod settings;
#[cfg(any(target_os = "macos", windows))]
mod tray;
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

    /// Set up a system hotkey that launches scrannotate, then exit. On GNOME
    /// the binding is installed automatically; elsewhere the exact one-time
    /// setup for your platform is printed. Pair with --combo and --screen.
    #[arg(long)]
    setup_hotkey: bool,

    /// The key combination for --setup-hotkey / --tray, e.g. "Ctrl+Shift+S"
    /// or "Cmd+Shift+4". Any modifier(s) plus one key.
    #[arg(long, value_name = "COMBO", default_value = "Ctrl+Shift+S")]
    combo: String,

    /// macOS/Windows: run resident in the tray/menubar. The saved hotkey (or
    /// the tray menu) launches a capture; the process stays until you quit it.
    /// A bare launch with no arguments does this too.
    #[arg(long)]
    tray: bool,

    /// macOS/Windows: open the hotkey settings window (used by the tray's
    /// "Set hotkey…" item), then exit.
    #[arg(long)]
    settings: bool,
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

    // A bare launch (double-clicking the app, no arguments) opens the tray on
    // macOS/Windows — scrannotate then lives in the menubar and a hotkey pops a
    // capture. Explicit arguments (including the ones the tray itself passes
    // to spawn a capture) skip this, as does the env-driven docs demo. Linux
    // has no tray, so it still captures.
    let bare_launch =
        std::env::args_os().count() == 1 && std::env::var_os("SCRANNOTATE_DEMO").is_none();

    let cli = Cli::parse();

    // Pick a role tag for the log, then initialize diagnostics (truncates the
    // log for a primary launch, appends for a tray-spawned child).
    let tag = if cli.setup_hotkey {
        "setup"
    } else if cli.settings {
        "settings"
    } else if cli.from_file.is_some() {
        "file"
    } else if cli.tray || bare_launch {
        "tray"
    } else {
        "capture"
    };
    diag::init(tag);
    diag!(
        "cli: tray={} settings={} setup_hotkey={} from_file={:?} screen={} cursor={} delay={} save_path={:?} bare_launch={}",
        cli.tray, cli.settings, cli.setup_hotkey, cli.from_file, cli.screen, cli.cursor, cli.delay, cli.save_path, bare_launch,
    );

    // Hotkey setup is a launcher-config action, not a capture: install (or
    // print) the binding for the chosen combination and exit before any
    // screen work happens.
    if cli.setup_hotkey {
        diag!("mode=setup-hotkey combo={:?} screen={}", cli.combo, cli.screen);
        return hotkey::setup(&cli.combo, cli.screen);
    }

    #[cfg(any(target_os = "macos", windows))]
    {
        if cli.settings {
            diag!("mode=settings — opening hotkey settings window");
            return settings::run();
        }
        // Resident tray/menubar launcher. Blocks until quit. The hotkey comes
        // from prefs (set in the Settings window), falling back to --combo.
        if cli.tray || bare_launch {
            let saved = prefs::load().hotkey.unwrap_or_else(|| cli.combo.clone());
            diag!("mode=tray — saved hotkey={:?} screen={}", saved, cli.screen);
            let combo = hotkey::Combo::parse(&saved)
                .or_else(|_| hotkey::Combo::parse(tray::DEFAULT_COMBO))?;
            diag!("tray: launching resident supervisor…");
            let r = tray::run(combo, cli.screen);
            diag!("tray: run() returned {:?}", r.as_ref().map(|_| ()));
            return r;
        }
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        if cli.tray {
            return Err(anyhow!(
                "--tray is macOS/Windows only; on Linux bind a launch hotkey with --setup-hotkey"
            ));
        }
        let _ = bare_launch; // Linux: a bare launch captures, as before.
    }

    // Docs/dev hook: SCRANNOTATE_DEMO renders a canned scene (pair with
    // SCRANNOTATE_SHOT to save a window screenshot and exit). Read once and
    // passed down so the two layers can't disagree about demo mode.
    let demo_mode = std::env::var("SCRANNOTATE_DEMO").ok();
    let demo = demo_mode.is_some();

    // Where monitors are enumerable, --pick-screen is a listing, not a
    // chooser: screen numbers are deterministic, so show what they mean.
    #[cfg(any(target_os = "macos", windows))]
    if cli.pick_screen && cli.from_file.is_none() && !demo {
        diag!("mode=pick-screen — listing displays");
        print!("{}", capture::screen_list()?);
        return Ok(());
    }

    let (img, display) = match &cli.from_file {
        Some(path) => {
            diag!("loading image from file {:?}", path);
            let img = image::open(path)
                .with_context(|| format!("opening {}", path.display()))?
                .to_rgba8();
            diag!("loaded file image {}x{}", img.width(), img.height());
            (img, None)
        }
        None if demo => {
            diag!("demo mode — rendering canned scene");
            (demo_base(), None)
        }
        None => {
            if cli.delay > 0 {
                diag!("delay: sleeping {}s before capture", cli.delay);
                std::thread::sleep(std::time::Duration::from_secs(cli.delay));
            }
            diag!("capture: START screen={} cursor={} pick_screen={}", cli.screen, cli.cursor, cli.pick_screen);
            let captured = capture::capture(&capture::CaptureOptions {
                cursor: cli.cursor,
                pick_screen: cli.pick_screen,
                screen: cli.screen,
            });
            match &captured {
                Ok(c) => diag!(
                    "capture: DONE {}x{} display={:?}",
                    c.image.width(),
                    c.image.height(),
                    c.display.as_ref().map(|_| "some")
                ),
                Err(e) => diag!("capture: FAILED — {e:#}"),
            }
            let capture::Capture { image, display } = captured?;
            (image, display)
        }
    };
    let out_dir = cli.save_path.unwrap_or_else(default_output_dir);
    diag!("output dir = {:?}", out_dir);

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
    diag!("editor: starting eframe window (select_full={select_full}, demo={demo})");
    let result = eframe::run_native(
        "scrannotate",
        options,
        Box::new(move |_cc| {
            diag!("editor: creating ScreencapApp");
            Ok(Box::new(app::ScreencapApp::new(img, out_dir, select_full, demo_mode, display)))
        }),
    );
    diag!("editor: eframe returned {:?}", result.as_ref().map(|_| ()).map_err(|e| e.to_string()));
    result
    .map_err(|err| anyhow!("running ui: {err}"))
}
