mod annotate;
mod app;
mod capture;
mod clipboard;
mod export;
mod prefs;

use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use clap::Parser;

/// Wayland screenshot + annotation tool (portal/PipeWire capture via pinray).
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Annotate an existing image instead of capturing the screen.
    #[arg(long, value_name = "PATH")]
    from_file: Option<PathBuf>,

    /// Seconds to wait before capturing (to set up menus etc.).
    #[arg(long, default_value_t = 0)]
    delay: u64,

    /// Directory screenshots are saved into.
    #[arg(long, value_name = "DIR")]
    output: Option<PathBuf>,

    /// Include the mouse cursor in the capture.
    #[arg(long)]
    cursor: bool,

    /// Re-open the portal's monitor chooser instead of reusing the saved
    /// grant (use this to capture a different screen).
    #[arg(long)]
    pick_monitor: bool,

    /// Start with the whole screen already selected.
    #[arg(long, conflicts_with = "region")]
    full: bool,

    /// Start --from-file images with no region selected.
    #[arg(long)]
    region: bool,
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
    let cli = Cli::parse();
    // Docs/dev hook: SCRANNOTATE_DEMO renders a canned scene (pair with
    // SCRANNOTATE_SHOT to save a window screenshot and exit).
    let demo = std::env::var_os("SCRANNOTATE_DEMO").is_some();

    let img = match &cli.from_file {
        Some(path) => image::open(path)
            .with_context(|| format!("opening {}", path.display()))?
            .to_rgba8(),
        None if demo => demo_base(),
        None => {
            if cli.delay > 0 {
                std::thread::sleep(std::time::Duration::from_secs(cli.delay));
            }
            capture::capture_screenshot(cli.cursor, cli.pick_monitor)?
        }
    };
    let out_dir = cli.output.unwrap_or_else(default_output_dir);

    // Everything happens in one fullscreen frozen-frame view. Fresh captures
    // start with no region (drag one out); --full and --from-file start with
    // the whole image selected so the toolbar is up immediately.
    let select_full = (cli.from_file.is_some() && !cli.region) || cli.full;

    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_app_id("scrannotate")
        .with_title("scrannotate");
    viewport = if demo {
        // Windowed, deterministic size for docs screenshots (run under Xvfb).
        viewport.with_inner_size([1680.0, 1160.0])
    } else {
        viewport.with_fullscreen(true)
    };
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "scrannotate",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(app::ScreencapApp::new(img, out_dir, select_full, cli.cursor)))
        }),
    )
    .map_err(|err| anyhow!("running ui: {err}"))
}
