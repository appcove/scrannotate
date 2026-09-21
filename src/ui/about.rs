//! Privacy and license information remains available before selecting a region.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Align2, Context, Id, Rect, RichText, Vec2};

#[derive(Default)]
pub struct About {
    open: bool,
    tab: Tab,
    notices: Option<String>,
}

#[derive(Default, PartialEq)]
enum Tab {
    #[default]
    Privacy,
    License,
    ThirdParty,
}

impl About {
    pub fn show(&mut self, ctx: &Context, canvas: Rect) {
        egui::Area::new(Id::new("about-button"))
            .anchor(Align2::RIGHT_TOP, Vec2::new(-12.0, 12.0))
            .constrain_to(canvas)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                if ui.button("About & Privacy").clicked() {
                    self.open = true;
                }
            });
        if !self.open {
            return;
        }

        let response = egui::Modal::new(Id::new("about-dialog")).show(ctx, |ui| {
            ui.set_width(600.0_f32.min((canvas.width() - 48.0).max(1.0)));
            ui.heading(concat!("Scrannotate ", env!("CARGO_PKG_VERSION")));
            ui.label("Capture a screen, annotate it, then copy or save.");
            ui.horizontal_wrapped(|ui| {
                ui.hyperlink_to("Support", "https://github.com/appcove/scrannotate/issues");
                if let Some(url) =
                    option_env!("SCRANNOTATE_PRIVACY_URL").filter(|url| !url.trim().is_empty())
                {
                    ui.hyperlink_to("Privacy policy online", url);
                }
            });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Privacy, "Privacy policy");
                ui.selectable_value(&mut self.tab, Tab::License, "App license");
                ui.selectable_value(&mut self.tab, Tab::ThirdParty, "Third-party notices");
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("about-content")
                .max_height((canvas.height() - 220.0).max(40.0))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let text = match self.tab {
                        Tab::Privacy => include_str!("../../PRIVACY.md"),
                        Tab::License => include_str!("../../LICENSE"),
                        Tab::ThirdParty => self.notices.get_or_insert_with(load_notices),
                    };
                    ui.label(RichText::new(text).size(14.0));
                });
            ui.separator();
            ui.button("Close").clicked()
        });
        if response.inner || response.should_close() {
            self.open = false;
        }
    }
}

fn load_notices() -> String {
    package_root()
        .and_then(|root| std::fs::read_to_string(root.join("THIRD_PARTY_NOTICES.txt")).ok())
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|executable| read_notices(&executable))
        })
        .unwrap_or_else(|| {
            "Third-party notices are unavailable in this build. Distributed release packages include THIRD_PARTY_NOTICES.txt beside the executable or in the app bundle's Resources folder."
                .to_owned()
        })
}

/// The MSIX install directory when running packaged, whatever the launch
/// route (Start menu, or the `scrannotate` execution alias whose reported
/// executable path may differ from the package layout). `None` elsewhere.
#[cfg(windows)]
fn package_root() -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt as _;
    use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS};
    use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentPackagePath;

    let mut length: u32 = 0;
    // SAFETY: a zero-length query only writes the required length.
    if unsafe { GetCurrentPackagePath(&mut length, std::ptr::null_mut()) }
        != ERROR_INSUFFICIENT_BUFFER
    {
        return None;
    }
    let mut buffer = vec![0u16; usize::try_from(length).ok()?];
    // SAFETY: `buffer` holds `length` u16 slots, as requested above.
    if unsafe { GetCurrentPackagePath(&mut length, buffer.as_mut_ptr()) } != ERROR_SUCCESS {
        return None;
    }
    let end = buffer.iter().position(|&unit| unit == 0)?;
    Some(PathBuf::from(OsString::from_wide(&buffer[..end])))
}

#[cfg(not(windows))]
fn package_root() -> Option<PathBuf> {
    None
}

fn read_notices(executable: &Path) -> Option<String> {
    let directory = executable.parent()?;
    [
        directory.join("THIRD_PARTY_NOTICES.txt"),
        directory.join("../Resources/THIRD_PARTY_NOTICES.txt"),
    ]
    .iter()
    .find_map(|path| std::fs::read_to_string(path).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privacy_dialog_fits_a_high_dpi_display() {
        let ctx = Context::default();
        let canvas = Rect::from_min_size(Default::default(), Vec2::new(960.0, 540.0));
        let mut about = About {
            open: true,
            ..Default::default()
        };
        for _ in 0..3 {
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(canvas),
                    ..Default::default()
                },
                |ui| {
                    about.show(ui.ctx(), canvas);
                },
            );
        }
        let rect = ctx
            .memory(|m| m.area_rect(Id::new("about-dialog")))
            .unwrap();
        assert!(
            canvas.contains_rect(rect),
            "privacy dialog overflows: {rect:?}"
        );
        assert!(ctx.memory(|m| m.top_modal_layer().is_some()));
    }
}
