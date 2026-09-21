//! Shared helpers for unit tests that drive egui headlessly.

use eframe::egui::{Context, FullOutput, RawInput, Ui};

/// Runs one frame like [`Context::run_ui`], then discards the texture
/// deltas. Tests have no renderer to upload them to, and epaint 0.36+
/// debug-asserts that a dropped `TexturesDelta` was handled.
pub(crate) fn run_ui(
    ctx: &Context,
    input: RawInput,
    add_contents: impl FnMut(&mut Ui),
) -> FullOutput {
    let mut output = ctx.run_ui(input, add_contents);
    output.textures_delta.clear();
    output
}
