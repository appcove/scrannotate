//! Copy the rendered PNG to the clipboard. A Wayland clipboard dies with
//! the process that owns it, and the app quits right after copying — so
//! `wl-copy` (which forks a child that keeps serving the clipboard) is the
//! primary path, and arboard the fallback for setups without wl-clipboard
//! (where it only outlives us if a clipboard manager takes the contents).

use std::borrow::Cow;
use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow};
use image::RgbaImage;

pub fn copy_image(img: &RgbaImage) -> Result<()> {
    let wl_err = match copy_with_wl_copy(img) {
        Ok(()) => return Ok(()),
        Err(err) => err,
    };
    copy_with_arboard(img).map_err(|arboard_err| {
        anyhow!("wl-copy: {wl_err}; arboard: {arboard_err}")
    })
}

fn copy_with_arboard(img: &RgbaImage) -> Result<()> {
    let mut clipboard = arboard::Clipboard::new().context("opening clipboard")?;
    clipboard
        .set_image(arboard::ImageData {
            width: img.width() as usize,
            height: img.height() as usize,
            bytes: Cow::Borrowed(img.as_raw()),
        })
        .context("setting clipboard image")
}

fn copy_with_wl_copy(img: &RgbaImage) -> Result<()> {
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .context("encoding png")?;
    let mut child = Command::new("wl-copy")
        .args(["-t", "image/png"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("spawning wl-copy (is wl-clipboard installed?)")?;
    child
        .stdin
        .take()
        .context("wl-copy stdin unavailable")?
        .write_all(&png)
        .context("writing png to wl-copy")?;
    let status = child.wait().context("waiting for wl-copy")?;
    if !status.success() {
        return Err(anyhow!("wl-copy exited with {status}"));
    }
    Ok(())
}
