//! The resident tray/menubar launcher. scrannotate normally runs one-shot, so
//! the tray is deliberately a *supervisor*, not the editor: it shows a tray
//! icon and owns a global hotkey, and each trigger spawns a fresh capture
//! process (the ordinary one-shot flow). No egui runs here, so this never
//! fights eframe's event loop, and every capture stays instant and
//! crash-isolated. Closing a capture window just ends that child — the tray
//! keeps running.
//!
//! macOS/Windows only. Linux uses `--setup-hotkey` instead: a Linux tray would
//! pull in GTK/AppIndicator, and Wayland forbids app-owned global hotkeys.
//!
//! Immediacy without polling: the hotkey and menu fire on the OS's own
//! threads; their handlers forward a [`UserEvent`] through an
//! [`EventLoopProxy`], which wakes the loop at once. The hotkey itself is set
//! in the Settings window (`--settings`), persisted in [`prefs`], and re-read
//! here whenever that window closes.
//!
//! On launch (and whenever the hotkey changes) the current hotkey is briefly
//! "flashed" beside the menubar icon, then cleared, so a user reopening the app
//! is reminded how to capture without having to hunt through the menu.

use std::process::Command;

use anyhow::{Context, Result};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use tray_icon::{
    Icon, TrayIcon, TrayIconBuilder,
    menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem},
};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    window::WindowId,
};

use crate::hotkey::Combo;
use crate::prefs;

/// Fallback when the user has never set a hotkey.
pub const DEFAULT_COMBO: &str = "Ctrl+Shift+S";


/// Loop wake-ups posted from the hotkey/menu OS callbacks.
#[derive(Debug)]
enum UserEvent {
    Capture,
    OpenSettings,
    /// The settings window closed — re-read and re-register the hotkey.
    ReloadHotkey,
    Quit,
}

/// Launch the resident tray. Blocks until the user quits it. The initial
/// hotkey comes from `combo` (which the caller sources from prefs). A capture
/// opens immediately on launch; the tray then stays resident.
pub fn run(combo: Combo, screen: u32) -> Result<()> {
    let hotkey = parse_hotkey(&combo)?;

    let event_loop = {
        let builder = EventLoop::<UserEvent>::with_user_event();
        #[cfg(target_os = "macos")]
        let builder = {
            use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
            let mut b = builder;
            // Menubar agent: no Dock icon, no app-switcher entry.
            b.with_activation_policy(ActivationPolicy::Accessory);
            b
        };
        let mut builder = builder;
        builder.build().context("creating the tray event loop")?
    };

    let proxy = event_loop.create_proxy();
    let mut app = App {
        screen,
        hotkey,
        combo_label: combo.label(),
        proxy,
        started: false,
        tray: None,
        hotkey_manager: None,
        capture_item: None,
        settings_id: None,
        quit_id: None,
    };
    event_loop.run_app(&mut app).context("running the tray event loop")?;
    Ok(())
}

fn parse_hotkey(combo: &Combo) -> Result<HotKey> {
    combo
        .accelerator()
        .parse()
        .with_context(|| format!("invalid hotkey combination '{}'", combo.accelerator()))
}

struct App {
    screen: u32,
    hotkey: HotKey,
    combo_label: String,
    proxy: EventLoopProxy<UserEvent>,
    started: bool,
    // Kept alive for the process's lifetime: dropping the manager unregisters
    // the hotkey, dropping the tray removes the icon. The capture item is held
    // so its label can be refreshed when the hotkey changes.
    tray: Option<TrayIcon>,
    hotkey_manager: Option<GlobalHotKeyManager>,
    capture_item: Option<MenuItem>,
    settings_id: Option<MenuId>,
    quit_id: Option<MenuId>,
}

impl App {
    /// One-time setup, done once the event loop is live (required on macOS
    /// before a tray icon or Carbon hotkey can be created).
    fn start(&mut self) -> Result<()> {
        let manager = GlobalHotKeyManager::new().context("initializing the global hotkey")?;
        manager
            .register(self.hotkey)
            .with_context(|| format!("registering hotkey {}", self.combo_label))?;

        let menu = Menu::new();
        let capture =
            MenuItem::new(self.capture_label(), true, None);
        let settings = MenuItem::new("Set hotkey…", true, None);
        let quit = MenuItem::new("Quit scrannotate", true, None);
        menu.append(&capture).context("building tray menu")?;
        menu.append(&settings).context("building tray menu")?;
        menu.append(&PredefinedMenuItem::separator()).ok();
        menu.append(&quit).context("building tray menu")?;
        self.settings_id = Some(settings.id().clone());
        self.quit_id = Some(quit.id().clone());
        let capture_id = capture.id().clone();

        let builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(self.tooltip())
            .with_icon(tray_icon());
        // macOS tints template icons to match the menubar (light/dark).
        #[cfg(target_os = "macos")]
        let builder = builder.with_icon_as_template(true);
        let tray = builder.build().context("creating the tray icon")?;

        // Forward OS-thread hotkey/menu events into the winit loop so it wakes
        // immediately. These replace the crates' default channels.
        let proxy = self.proxy.clone();
        let hk_proxy = proxy.clone();
        GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
            // Fire on press only; the release would double-trigger.
            if e.state == HotKeyState::Pressed {
                let _ = hk_proxy.send_event(UserEvent::Capture);
            }
        }));
        let settings_id = self.settings_id.clone();
        let quit_id = self.quit_id.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let ev = if Some(&e.id) == quit_id.as_ref() {
                UserEvent::Quit
            } else if Some(&e.id) == settings_id.as_ref() {
                UserEvent::OpenSettings
            } else if e.id == capture_id {
                UserEvent::Capture
            } else {
                return;
            };
            let _ = proxy.send_event(ev);
        }));

        self.hotkey_manager = Some(manager);
        self.capture_item = Some(capture);
        self.tray = Some(tray);

        // Every launch from a quit state opens a capture right away (so the
        // app is immediately useful), then stays resident in the tray. The
        // capture is a separate process, so closing it leaves the tray running.
        self.spawn_capture();
        // Hotkey-reminder flash disabled for now (kept for later).
        // self.spawn_flash();
        Ok(())
    }

    /// Show the centered hotkey-reminder window (`--flash`). Launch-only, so the
    /// user sees the shortcut once per fresh start, not on every capture.
    #[allow(dead_code)]
    fn spawn_flash(&self) {
        self.spawn(&["--flash"]);
    }

    fn capture_label(&self) -> String {
        format!("Capture screen {}   {}", self.screen, self.combo_label)
    }

    fn tooltip(&self) -> String {
        format!("scrannotate — {} to capture", self.combo_label)
    }

    /// Spawn a fresh one-shot capture. Uses this exact executable so the child
    /// shares scrannotate's screen-recording grant identity.
    fn spawn_capture(&self) {
        self.spawn(&["--screen", &self.screen.to_string()]);
    }

    /// Open the settings window in its own process, then reload the hotkey when
    /// it closes. Waiting happens on a thread so the tray stays responsive.
    fn open_settings(&self) {
        let Ok(exe) = std::env::current_exe() else {
            eprintln!("scrannotate: cannot find own executable to open settings");
            return;
        };
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let _ = Command::new(exe).arg("--settings").status();
            let _ = proxy.send_event(UserEvent::ReloadHotkey);
        });
    }

    /// Re-read the saved hotkey and swap the registration if it changed.
    fn reload_hotkey(&mut self) {
        let combo = prefs::load()
            .hotkey
            .and_then(|s| Combo::parse(&s).ok())
            .or_else(|| Combo::parse(DEFAULT_COMBO).ok());
        let Some(combo) = combo else { return };
        let Ok(new_hotkey) = parse_hotkey(&combo) else { return };
        if new_hotkey == self.hotkey {
            return;
        }
        let Some(manager) = &self.hotkey_manager else { return };
        if let Err(e) = manager.register(new_hotkey) {
            eprintln!("scrannotate: could not register {}: {e}", combo.label());
            return;
        }
        let _ = manager.unregister(self.hotkey);
        self.hotkey = new_hotkey;
        self.combo_label = combo.label();
        if let Some(item) = &self.capture_item {
            item.set_text(self.capture_label());
        }
        if let Some(tray) = &self.tray {
            let _ = tray.set_tooltip(Some(self.tooltip()));
        }
    }

    fn spawn(&self, args: &[&str]) {
        let exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("scrannotate: cannot find own executable: {e}");
                return;
            }
        };
        if let Err(e) = Command::new(exe).args(args).spawn() {
            eprintln!("scrannotate: failed to launch: {e}");
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Wait);
        if !self.started {
            self.started = true;
            if let Err(e) = self.start() {
                eprintln!("scrannotate: {e:#}");
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Capture => self.spawn_capture(),
            UserEvent::OpenSettings => self.open_settings(),
            UserEvent::ReloadHotkey => self.reload_hotkey(),
            UserEvent::Quit => event_loop.exit(),
        }
    }

    // The supervisor owns no windows; nothing to handle.
    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
}

/// An "S" inside a ring — the logo, rasterized at runtime from the embedded
/// font so the tray needs no image asset. On macOS it's drawn black and flagged
/// as a template (see `with_icon_as_template`), letting the system tint it for
/// the light/dark menubar; elsewhere it's drawn white for the typically-dark
/// taskbar tray.
fn tray_icon() -> Icon {
    use ab_glyph::{Font, FontRef, PxScale};

    const N: u32 = 36;
    // Single coverage (alpha) buffer; the ring and the glyph both write into it.
    let mut alpha = vec![0.0f32; (N * N) as usize];
    let put = |alpha: &mut [f32], x: i32, y: i32, a: f32| {
        if x >= 0 && y >= 0 && (x as u32) < N && (y as u32) < N && a > 0.0 {
            let i = (y as u32 * N + x as u32) as usize;
            alpha[i] = alpha[i].max(a);
        }
    };

    // The ring.
    let c = (N as f32 - 1.0) / 2.0;
    let radius = N as f32 / 2.0 - 2.0;
    let half_thick = 1.4;
    for y in 0..N as i32 {
        for x in 0..N as i32 {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
            let cov = (half_thick + 0.5 - (d - radius).abs()).clamp(0.0, 1.0);
            put(&mut alpha, x, y, cov);
        }
    }

    // The "S", scaled to sit inside the ring.
    if let Ok(font) = FontRef::try_from_slice(epaint_default_fonts::HACK_REGULAR) {
        let glyph = font.glyph_id('S').with_scale(PxScale::from(N as f32 * 0.66));
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            let off_x = ((N as f32 - bounds.width()) / 2.0).round() as i32;
            let off_y = ((N as f32 - bounds.height()) / 2.0).round() as i32;
            outlined.draw(|x, y, coverage| {
                put(&mut alpha, off_x + x as i32, off_y + y as i32, coverage);
            });
        }
    }

    let [r, g, b] = if cfg!(target_os = "macos") { [0u8, 0, 0] } else { [255u8, 255, 255] };
    let mut rgba = Vec::with_capacity((N * N * 4) as usize);
    for a in alpha {
        rgba.extend_from_slice(&[r, g, b, (a * 255.0) as u8]);
    }
    Icon::from_rgba(rgba, N, N).expect("valid tray icon")
}
