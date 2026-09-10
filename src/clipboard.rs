//! Copy the rendered PNG to the clipboard.
//!
//! On Linux a clipboard dies with the process that owns it — Wayland and
//! X11 alike — and the app quits right after copying. So the primary path
//! is an external tool that forks a child to keep serving the clipboard:
//! `wl-copy` on Wayland, `xclip` on X11. Trying both beats detecting the
//! session — the wrong one fails fast (no socket / no display). arboard is
//! the fallback for setups with neither tool, where the contents only
//! outlive us if a clipboard manager takes them. On macOS and Windows the
//! OS owns clipboard contents past process exit, so arboard alone is
//! enough.

use anyhow::Result;
use image::RgbaImage;

#[cfg(all(unix, not(target_os = "macos")))]
pub fn copy_image(img: &RgbaImage) -> Result<()> {
    use anyhow::Context;

    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .context("encoding png")?;
    let wl_err = match copy_with_tool("wl-copy", &["-t", "image/png"], &png) {
        Ok(()) => return Ok(()),
        Err(err) => err,
    };
    let xclip_err =
        match copy_with_tool("xclip", &["-selection", "clipboard", "-t", "image/png"], &png) {
            Ok(()) => return Ok(()),
            Err(err) => err,
        };
    copy_with_arboard(img).map_err(|arboard_err| {
        anyhow::anyhow!("wl-copy: {wl_err}; xclip: {xclip_err}; arboard: {arboard_err}")
    })
}

#[cfg(any(target_os = "macos", windows))]
pub fn copy_image(img: &RgbaImage) -> Result<()> {
    copy_with_arboard(img)
}

fn copy_with_arboard(img: &RgbaImage) -> Result<()> {
    use anyhow::Context;

    let mut clipboard = arboard::Clipboard::new().context("opening clipboard")?;
    clipboard
        .set_image(arboard::ImageData {
            width: img.width() as usize,
            height: img.height() as usize,
            bytes: std::borrow::Cow::Borrowed(img.as_raw()),
        })
        .context("setting clipboard image")
}

/// Pipe `png` into a clipboard tool that forks a child to keep serving the
/// contents after this process exits (both wl-copy and xclip do).
#[cfg(all(unix, not(target_os = "macos")))]
fn copy_with_tool(tool: &str, args: &[&str], png: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    use anyhow::{Context, anyhow};

    let mut child = Command::new(tool)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("spawning {tool} (is it installed?)"))?;
    child
        .stdin
        .take()
        .with_context(|| format!("{tool} stdin unavailable"))?
        .write_all(png)
        .with_context(|| format!("writing png to {tool}"))?;
    let status = child.wait().with_context(|| format!("waiting for {tool}"))?;
    if !status.success() {
        return Err(anyhow!("{tool} exited with {status}"));
    }
    Ok(())
}
