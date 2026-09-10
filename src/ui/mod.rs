//! egui front-end: translates input into editor calls and paints the
//! canvas, toolbar, color picker, and inline text editor.

pub mod canvas;
pub mod color_picker;
pub mod paint;
pub mod text_overlay;
pub mod toolbar;

use eframe::egui::{Color32, Key};

use crate::annotate::Tool;

/// Accent for selection/hover chrome.
pub const ACCENT: Color32 = Color32::from_rgb(0x4c, 0x9e, 0xff);
/// Background fill of the active tool button.
pub const ACTIVE_TOOL_FILL: Color32 = Color32::from_rgb(0x1c, 0x63, 0x9e);

/// Content width of the toolbar popup. Sized so the longest action label
/// ("Save+Close  Ctrl+S") fits inside its half of a two-button row — that
/// is what lets every button in the panel share one width.
pub const TOOLBAR_W: f32 = 350.0;
/// Height of every toolbar button, tools and actions alike.
pub const BTN_H: f32 = 34.0;

/// Tool order in the toolbar (two per row, most-used first) and their
/// shortcut keys.
pub const TOOLS: [(Tool, Key); 10] = [
    (Tool::Arrow, Key::A),
    (Tool::Text, Key::T),
    (Tool::Marker, Key::M),
    (Tool::Line, Key::L),
    (Tool::Select, Key::S),
    (Tool::Pen, Key::P),
    (Tool::Rect, Key::R),
    (Tool::Ellipse, Key::E),
    (Tool::Highlight, Key::H),
    (Tool::Pixelate, Key::B),
];
