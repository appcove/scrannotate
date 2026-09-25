//! egui front-end: translates input into editor calls and paints the
//! canvas, toolbar, color picker, and inline text editor.

pub mod about;
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

/// Content width of the toolbar popup at [`UiScale::Medium`]. Sized so the
/// longest action label ("Save+Close Ctrl+S") fits inside its half of a
/// two-button row — that is what lets every button in the panel share one
/// width. [`Toolbar::show`](toolbar::Toolbar::show) scales this by the
/// user's chosen [`UiScale`].
pub const TOOLBAR_W: f32 = 300.0;
/// Height of every toolbar button, tools and actions alike, at
/// [`UiScale::Medium`].
pub const BTN_H: f32 = 32.0;

/// User-selectable toolbar density: scales the popup's width, buttons, and
/// text together, so someone who wants a smaller footprint or bigger,
/// easier-to-read text can have it. Persisted via `prefs`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum UiScale {
    Small,
    #[default]
    Medium,
    Large,
}

impl UiScale {
    pub const ALL: [UiScale; 3] = [UiScale::Small, UiScale::Medium, UiScale::Large];

    /// Multiplier applied to every size in the toolbar (width, button
    /// height, padding, font sizes).
    pub fn factor(self) -> f32 {
        match self {
            UiScale::Small => 0.85,
            UiScale::Medium => 1.0,
            UiScale::Large => 1.2,
        }
    }

    /// Full name, shown on the dropdown trigger and its menu items.
    pub fn name(self) -> &'static str {
        match self {
            UiScale::Small => "Small",
            UiScale::Medium => "Medium",
            UiScale::Large => "Large",
        }
    }

    /// `prefs` file encoding.
    pub fn as_str(self) -> &'static str {
        match self {
            UiScale::Small => "small",
            UiScale::Medium => "medium",
            UiScale::Large => "large",
        }
    }

    /// Inverse of [`Self::as_str`]; unrecognized text (a stale or hand-edited
    /// prefs file) falls back to `None` rather than a guess.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "small" => Some(UiScale::Small),
            "medium" => Some(UiScale::Medium),
            "large" => Some(UiScale::Large),
            _ => None,
        }
    }
}

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
