//! Native file selection and the lifetime of sandbox file authorization.

use std::path::{Path, PathBuf};

use anyhow::Result;
use image::RgbaImage;

#[cfg(all(target_os = "macos", feature = "mac-app-store"))]
mod macos;

/// Save to the configured directory, asking for authorization in sandboxed
/// macOS builds. `None` means the user cancelled the folder chooser.
pub fn save_image(image: &RgbaImage, suggested_dir: &Path) -> Result<Option<PathBuf>> {
    #[cfg(all(target_os = "macos", feature = "mac-app-store"))]
    {
        macos::save_image(image, suggested_dir)
    }
    #[cfg(not(all(target_os = "macos", feature = "mac-app-store")))]
    {
        crate::export::save_timestamped(image, suggested_dir).map(Some)
    }
}

/// Load a PNG selected through the platform's file dialog, starting in
/// `initial` when given (a hint only; no access is inferred from it). The
/// image is decoded before the panel's temporary authorization is released.
pub fn open_image(initial: Option<&Path>) -> Result<Option<RgbaImage>> {
    #[cfg(all(target_os = "macos", feature = "mac-app-store"))]
    {
        macos::open_image(initial)
    }
    #[cfg(any(windows, all(target_os = "macos", not(feature = "mac-app-store"))))]
    {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Open PNG image")
            .add_filter("PNG images", &["png"]);
        if let Some(initial) = initial {
            dialog = dialog.set_directory(initial);
        }
        dialog.pick_file().map(|path| load_image(&path)).transpose()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = initial;
        anyhow::bail!("Native image selection is available on Windows and macOS; use --from-file")
    }
}

/// Change the output folder. macOS store builds also retain authorization
/// for future launches; other desktop builds return a session preference.
#[cfg(any(target_os = "macos", windows))]
pub fn choose_output_directory(current: &Path) -> Result<Option<PathBuf>> {
    #[cfg(all(target_os = "macos", feature = "mac-app-store"))]
    {
        macos::choose_output_directory(current)
    }
    #[cfg(any(windows, all(target_os = "macos", not(feature = "mac-app-store"))))]
    {
        Ok(rfd::FileDialog::new()
            .set_title("Choose screenshot output folder")
            .set_directory(current)
            .pick_folder())
    }
}

#[cfg(any(windows, target_os = "macos"))]
fn load_image(path: &Path) -> Result<RgbaImage> {
    use anyhow::Context as _;
    image::open(path)
        .with_context(|| format!("opening PNG image {}", path.display()))
        .map(|image| image.to_rgba8())
}
