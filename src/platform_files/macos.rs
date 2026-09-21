//! App Sandbox authorization. Retain the actual NSURL returned by NSOpenPanel:
//! converting it to a path too early discards the panel's security scope.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, anyhow, ensure};
use image::RgbaImage;
use objc2::{MainThreadMarker, rc::Retained, runtime::Bool};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSModalResponseOK, NSOpenPanel};
use objc2_foundation::{
    NSArray, NSData, NSDataWritingOptions, NSString, NSURL, NSURLBookmarkCreationOptions,
    NSURLBookmarkResolutionOptions,
};

/// Owns one access grant. A panel starts its grant implicitly; bookmark
/// resolution requires an explicit start. Both are stopped exactly once.
struct ScopedUrl {
    url: Retained<NSURL>,
}

impl ScopedUrl {
    fn from_panel(url: Retained<NSURL>) -> Self {
        // NSOpenPanel starts security-scoped access before returning the URL.
        Self { url }
    }

    fn from_bookmark(url: Retained<NSURL>) -> Result<Self> {
        // SAFETY: This retained URL was resolved with WithSecurityScope. A
        // successful start is paired with one stop in Drop on every path.
        ensure!(
            unsafe { url.startAccessingSecurityScopedResource() },
            "Access to the saved output folder is no longer authorized"
        );
        Ok(Self { url })
    }

    fn path(&self) -> Result<PathBuf> {
        self.url
            .to_file_path()
            .context("The selected location is not a local file URL")
    }
}

impl Drop for ScopedUrl {
    fn drop(&mut self) {
        // SAFETY: Exactly one explicit or panel-owned implicit start is
        // represented by this guard, which is never cloned.
        unsafe { self.url.stopAccessingSecurityScopedResource() };
    }
}

pub(super) fn save_image(image: &RgbaImage, suggested_dir: &Path) -> Result<Option<PathBuf>> {
    let directory = match restore_output_folder() {
        Ok(Some(directory)) => Some(directory),
        Ok(None) => authorize_output_folder(suggested_dir, false)?,
        Err(error) => {
            // Do not fall back to a guessed path or create a replacement for
            // a missing bookmarked directory. Require a new panel selection.
            eprintln!("Saved output folder needs authorization: {error:#}");
            authorize_output_folder(suggested_dir, true)?
        }
    };
    let Some(directory) = directory else {
        return Ok(None);
    };
    crate::export::save_timestamped(image, &directory.path()?).map(Some)
    // `directory` remains alive through encoding, flush, and cleanup.
}

pub(super) fn open_image(initial: Option<&Path>) -> Result<Option<RgbaImage>> {
    let Some(file) = select_location(false, initial, false)? else {
        return Ok(None);
    };
    super::load_image(&file.path()?).map(Some)
}

pub(super) fn choose_output_directory(current: &Path) -> Result<Option<PathBuf>> {
    authorize_output_folder(current, false)?
        .map(|directory| directory.path())
        .transpose()
}

fn authorize_output_folder(initial: &Path, reauthorize: bool) -> Result<Option<ScopedUrl>> {
    let Some(directory) = select_location(true, Some(initial), reauthorize)? else {
        return Ok(None);
    };
    ensure!(
        directory.path()?.is_dir(),
        "The selected folder is unavailable"
    );
    persist_bookmark(&directory.url)?;
    Ok(Some(directory))
}

fn select_location(
    directory: bool,
    initial: Option<&Path>,
    reauthorize: bool,
) -> Result<Option<ScopedUrl>> {
    let main_thread =
        MainThreadMarker::new().context("File dialogs must run on the main thread")?;
    let app = NSApplication::sharedApplication(main_thread);
    let previous_window = app.keyWindow();
    let previous_policy = app.activationPolicy();
    let needs_activation = previous_policy == NSApplicationActivationPolicy::Prohibited;
    if needs_activation {
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    }
    let panel = NSOpenPanel::openPanel(main_thread);
    if let Some(window) = &previous_window {
        panel.setLevel(panel.level().max(window.level().saturating_add(1)));
    }
    panel.setAllowsMultipleSelection(false);
    panel.setCanChooseDirectories(directory);
    panel.setCanChooseFiles(!directory);
    panel.setCanCreateDirectories(directory);
    panel.setTitle(Some(&NSString::from_str(if directory {
        "Choose screenshot output folder"
    } else {
        "Open PNG image"
    })));
    if directory {
        panel.setMessage(Some(&NSString::from_str(if reauthorize {
            "Your saved output folder is unavailable or needs permission again. Select it again, choose another folder, or cancel to keep editing."
        } else {
            "Choose where Scrannotate saves screenshots. This folder will also be used on future launches."
        })));
    } else {
        let types = NSArray::from_retained_slice(&[NSString::from_str("png")]);
        // This extension filter also supports the project's macOS 12.3 floor,
        // and matches rfd's native PNG filter implementation.
        #[allow(deprecated)]
        panel.setAllowedFileTypes(Some(&types));
    }
    if let Some(initial) = initial.and_then(NSURL::from_directory_path) {
        // A starting location is a hint only; no access is inferred from it.
        panel.setDirectoryURL(Some(&initial));
    }
    let response = panel.runModal();
    if let Some(window) = previous_window {
        window.makeKeyAndOrderFront(None);
    }
    if needs_activation {
        app.setActivationPolicy(previous_policy);
    }
    if response != NSModalResponseOK {
        return Ok(None);
    }
    let url = panel
        .URL()
        .context("The file dialog returned no selected location")?;
    Ok(Some(ScopedUrl::from_panel(url)))
}

fn bookmark_path() -> Result<PathBuf> {
    Ok(crate::prefs::state_dir()
        .context("The app's settings directory is unavailable")?
        .join("output-folder.bookmark"))
}

fn restore_output_folder() -> Result<Option<ScopedUrl>> {
    let path = bookmark_path()?;
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("Reading the saved output folder authorization"),
    };
    let data = NSData::with_bytes(&bytes);
    let mut stale = Bool::NO;
    // SAFETY: `stale` is live and writable throughout this synchronous call.
    // WithoutUI/WithoutMounting prevent surprise prompts or mounting a drive;
    // failures go through our explicit reauthorization panel instead.
    let url = unsafe {
        NSURL::URLByResolvingBookmarkData_options_relativeToURL_bookmarkDataIsStale_error(
            &data,
            NSURLBookmarkResolutionOptions::WithSecurityScope
                | NSURLBookmarkResolutionOptions::WithoutUI
                | NSURLBookmarkResolutionOptions::WithoutMounting,
            None,
            &mut stale,
        )
    }
    .map_err(|error| anyhow!("Resolving saved output folder: {error}"))?;
    let directory = ScopedUrl::from_bookmark(url)?;
    ensure!(
        directory.path()?.is_dir(),
        "The saved output folder is unavailable"
    );
    if stale.as_bool() {
        // Refresh while the resolved URL is still authorized.
        persist_bookmark(&directory.url)?;
    }
    Ok(Some(directory))
}

fn persist_bookmark(url: &NSURL) -> Result<()> {
    let data = url
        .bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
            NSURLBookmarkCreationOptions::WithSecurityScope,
            None,
            None,
        )
        .map_err(|error| anyhow!("Remembering output folder permission: {error}"))?;
    let path = bookmark_path()?;
    let parent = path
        .parent()
        .context("Invalid output folder authorization path")?;
    std::fs::create_dir_all(parent).context("Creating the app's settings directory")?;
    let file_url =
        NSURL::from_file_path(&path).context("Invalid output folder authorization path")?;
    // Atomic replacement preserves the previous grant if persistence fails.
    data.writeToURL_options_error(&file_url, NSDataWritingOptions::Atomic)
        .map_err(|error| anyhow!("Saving output folder permission: {error}"))
}
