//! Design tokens. Tenon's own palette (graphite chrome, teal accent); every widget reads colours
//! from here.

use egui::{Color32, FontFamily, FontId};

#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub title_bar: Color32,
    pub tab_strip: Color32,
    pub ribbon: Color32,
    pub panel: Color32,
    pub panel_header: Color32,
    pub border: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_disabled: Color32,
    pub icon: Color32,
    pub icon_disabled: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub viewport_top: Color32,
    pub viewport_bottom: Color32,
    pub viewport_text: Color32,
    pub cube_face: Color32,
    pub cube_face_hover: Color32,
    pub cube_edge: Color32,
    pub cube_text: Color32,
    pub axis_x: Color32,
    pub axis_y: Color32,
    pub axis_z: Color32,
    pub history_marker: Color32,
}

impl Tokens {
    pub const DARK: Tokens = Tokens {
        title_bar: Color32::from_rgb(0x1d, 0x21, 0x27),
        tab_strip: Color32::from_rgb(0x23, 0x28, 0x2f),
        ribbon: Color32::from_rgb(0x2b, 0x31, 0x39),
        panel: Color32::from_rgb(0x26, 0x2b, 0x32),
        panel_header: Color32::from_rgb(0x2f, 0x35, 0x3e),
        border: Color32::from_rgb(0x15, 0x18, 0x1c),
        hover: Color32::from_rgb(0x3a, 0x42, 0x4d),
        pressed: Color32::from_rgb(0x45, 0x4f, 0x5c),
        text: Color32::from_rgb(0xdc, 0xe1, 0xe6),
        text_dim: Color32::from_rgb(0x9a, 0xa3, 0xad),
        text_disabled: Color32::from_rgb(0x6b, 0x73, 0x7d),
        icon: Color32::from_rgb(0xc9, 0xd2, 0xdb),
        icon_disabled: Color32::from_rgb(0x74, 0x7d, 0x88),
        accent: Color32::from_rgb(0x2f, 0xb8, 0xb0),
        accent_text: Color32::from_rgb(0x0e, 0x1a, 0x1c),
        viewport_top: Color32::from_rgb(0x4a, 0x55, 0x63),
        viewport_bottom: Color32::from_rgb(0x1f, 0x24, 0x2b),
        viewport_text: Color32::from_rgb(0xb8, 0xc2, 0xcc),
        cube_face: Color32::from_rgb(0xd5, 0xdb, 0xe1),
        cube_face_hover: Color32::from_rgb(0x8f, 0xdc, 0xd6),
        cube_edge: Color32::from_rgb(0x5d, 0x67, 0x72),
        cube_text: Color32::from_rgb(0x3a, 0x42, 0x4c),
        axis_x: Color32::from_rgb(0xe0, 0x5a, 0x4f),
        axis_y: Color32::from_rgb(0x5c, 0xc0, 0x6a),
        axis_z: Color32::from_rgb(0x4f, 0x8f, 0xe8),
        history_marker: Color32::from_rgb(0xe0, 0x8a, 0x3c),
    };
}

pub fn small() -> FontId {
    FontId::new(11.0, FontFamily::Proportional)
}
pub fn body() -> FontId {
    FontId::new(12.5, FontFamily::Proportional)
}
pub fn heading() -> FontId {
    FontId::new(13.5, FontFamily::Proportional)
}

/// Dark visuals with Tenon's colours for egui's own widgets (windows, menus, tooltips).
pub fn apply(ctx: &egui::Context) {
    let t = Tokens::DARK;
    let mut v = egui::Visuals::dark();
    v.panel_fill = t.panel;
    v.window_fill = t.panel;
    v.extreme_bg_color = t.title_bar;
    v.selection.bg_fill = t.accent;
    v.hyperlink_color = t.accent;
    v.widgets.hovered.weak_bg_fill = t.hover;
    v.widgets.active.weak_bg_fill = t.pressed;
    ctx.set_visuals(v);
}
