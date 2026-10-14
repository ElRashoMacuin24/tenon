//! Design tokens. Tenon's own palettes, light and dark (Tools > Application Options); every
//! widget reads colours from here.

use egui::{Color32, FontFamily, FontId};

/// The two UI colour schemes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemeName {
    #[default]
    Dark,
    Light,
}

#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub dark: bool,
    pub title_bar: Color32,
    pub tab_strip: Color32,
    /// Background of the active ribbon tab and the ribbon body.
    pub ribbon: Color32,
    pub panel: Color32,
    pub panel_header: Color32,
    pub border: Color32,
    pub separator: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_disabled: Color32,
    pub icon: Color32,
    pub icon_disabled: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub field: Color32,
    pub status_bar: Color32,
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
    /// Fill tint of solid-feature icons and of work-feature icons.
    pub tint_solid: Color32,
    pub tint_work: Color32,
    pub tint_sketch: Color32,
    pub ok: Color32,
}

impl Tokens {
    pub const DARK: Tokens = Tokens {
        dark: true,
        title_bar: Color32::from_rgb(0x1d, 0x21, 0x27),
        tab_strip: Color32::from_rgb(0x23, 0x28, 0x2f),
        ribbon: Color32::from_rgb(0x2b, 0x31, 0x39),
        panel: Color32::from_rgb(0x26, 0x2b, 0x32),
        panel_header: Color32::from_rgb(0x2f, 0x35, 0x3e),
        border: Color32::from_rgb(0x15, 0x18, 0x1c),
        separator: Color32::from_rgb(0x3b, 0x42, 0x4c),
        hover: Color32::from_rgb(0x3a, 0x42, 0x4d),
        pressed: Color32::from_rgb(0x45, 0x4f, 0x5c),
        text: Color32::from_rgb(0xdc, 0xe1, 0xe6),
        text_dim: Color32::from_rgb(0x9a, 0xa3, 0xad),
        text_disabled: Color32::from_rgb(0x6b, 0x73, 0x7d),
        icon: Color32::from_rgb(0xc9, 0xd2, 0xdb),
        icon_disabled: Color32::from_rgb(0x74, 0x7d, 0x88),
        accent: Color32::from_rgb(0x2f, 0xb8, 0xb0),
        accent_text: Color32::from_rgb(0x0e, 0x1a, 0x1c),
        field: Color32::from_rgb(0x1a, 0x1e, 0x23),
        status_bar: Color32::from_rgb(0x1d, 0x21, 0x27),
        viewport_top: Color32::from_rgb(0x4a, 0x55, 0x63),
        viewport_bottom: Color32::from_rgb(0x1f, 0x24, 0x2b),
        viewport_text: Color32::from_rgb(0xb8, 0xc2, 0xcc),
        cube_face: Color32::from_rgb(0xd5, 0xdb, 0xe1),
        cube_face_hover: Color32::from_rgb(0x8c, 0xc8, 0xf0),
        cube_edge: Color32::from_rgb(0x5d, 0x67, 0x72),
        cube_text: Color32::from_rgb(0x3a, 0x42, 0x4c),
        axis_x: Color32::from_rgb(0xe0, 0x5a, 0x4f),
        axis_y: Color32::from_rgb(0x5c, 0xc0, 0x6a),
        axis_z: Color32::from_rgb(0x4f, 0x8f, 0xe8),
        history_marker: Color32::from_rgb(0xe0, 0x5a, 0x4f),
        tint_solid: Color32::from_rgb(0x4d, 0x95, 0xdc),
        tint_work: Color32::from_rgb(0xe3, 0x9b, 0x3a),
        tint_sketch: Color32::from_rgb(0x62, 0xc0, 0x8a),
        ok: Color32::from_rgb(0x4c, 0xb8, 0x5c),
    };

    pub const LIGHT: Tokens = Tokens {
        dark: false,
        title_bar: Color32::from_rgb(0xf7, 0xf8, 0xf9),
        tab_strip: Color32::from_rgb(0xec, 0xee, 0xf0),
        ribbon: Color32::from_rgb(0xfb, 0xfb, 0xfc),
        panel: Color32::from_rgb(0xff, 0xff, 0xff),
        panel_header: Color32::from_rgb(0xee, 0xf0, 0xf2),
        border: Color32::from_rgb(0xc9, 0xcd, 0xd2),
        separator: Color32::from_rgb(0xdc, 0xdf, 0xe3),
        hover: Color32::from_rgb(0xdf, 0xec, 0xf8),
        pressed: Color32::from_rgb(0xc6, 0xdd, 0xf2),
        text: Color32::from_rgb(0x24, 0x28, 0x2d),
        text_dim: Color32::from_rgb(0x5c, 0x64, 0x6d),
        text_disabled: Color32::from_rgb(0xa3, 0xa9, 0xb0),
        icon: Color32::from_rgb(0x3a, 0x42, 0x4c),
        icon_disabled: Color32::from_rgb(0xae, 0xb4, 0xbb),
        accent: Color32::from_rgb(0x1f, 0x9d, 0x96),
        accent_text: Color32::from_rgb(0xff, 0xff, 0xff),
        field: Color32::from_rgb(0xff, 0xff, 0xff),
        status_bar: Color32::from_rgb(0xec, 0xee, 0xf0),
        viewport_top: Color32::from_rgb(0xc3, 0xcf, 0xdb),
        viewport_bottom: Color32::from_rgb(0xf1, 0xf4, 0xf7),
        viewport_text: Color32::from_rgb(0x3c, 0x46, 0x51),
        cube_face: Color32::from_rgb(0xec, 0xee, 0xf0),
        cube_face_hover: Color32::from_rgb(0x8c, 0xc8, 0xf0),
        cube_edge: Color32::from_rgb(0x8a, 0x93, 0x9d),
        cube_text: Color32::from_rgb(0x4a, 0x52, 0x5c),
        axis_x: Color32::from_rgb(0xd0, 0x3c, 0x30),
        axis_y: Color32::from_rgb(0x2e, 0x9e, 0x44),
        axis_z: Color32::from_rgb(0x2b, 0x6c, 0xd0),
        history_marker: Color32::from_rgb(0xd0, 0x3c, 0x30),
        tint_solid: Color32::from_rgb(0x3f, 0x8a, 0xd8),
        tint_work: Color32::from_rgb(0xe8, 0x95, 0x2c),
        tint_sketch: Color32::from_rgb(0x3a, 0xa6, 0x6a),
        ok: Color32::from_rgb(0x2e, 0x9e, 0x44),
    };

    pub fn of(name: ThemeName) -> Tokens {
        match name {
            ThemeName::Dark => Tokens::DARK,
            ThemeName::Light => Tokens::LIGHT,
        }
    }
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

/// egui's own widgets (windows, menus, tooltips, text fields) in Tenon's colours.
pub fn apply_theme(ctx: &egui::Context, name: ThemeName) {
    let t = Tokens::of(name);
    let mut v = if t.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = t.panel;
    v.window_fill = t.panel;
    v.extreme_bg_color = t.field;
    v.selection.bg_fill = t.accent.gamma_multiply(if t.dark { 1.0 } else { 0.55 });
    v.hyperlink_color = t.accent;
    v.widgets.hovered.weak_bg_fill = t.hover;
    v.widgets.active.weak_bg_fill = t.pressed;
    v.widgets.noninteractive.fg_stroke.color = t.text;
    v.widgets.inactive.fg_stroke.color = t.text;
    ctx.set_visuals(v);
}

/// The default (dark) theme.
pub fn apply(ctx: &egui::Context) {
    apply_theme(ctx, ThemeName::Dark);
}

/// Tenon's mark as the window's icon (the title bar draws it the same way): a tenon in its
/// mortise, in the dark theme's colours.
pub fn app_icon() -> egui::IconData {
    const SIZE: u32 = 64;
    let (accent, slot) = ([0x2f, 0xb8, 0xb0], [0x1d, 0x21, 0x27]);
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    let n = SIZE as f32;
    // (The square's corners rounded by a fifth of it; the slot 0.3 by 0.6 of it, a tenth below
    // the middle; each edge softened over a pixel.)
    let radius = n * 0.2;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let (cx, cy) = ((px - n / 2.0).abs() - (n / 2.0 - radius), (py - n / 2.0).abs() - (n / 2.0 - radius));
            let outside = (cx.max(0.0).hypot(cy.max(0.0)) - radius).max(cx.max(cy).min(0.0) - radius);
            let cover = (0.5 - outside).clamp(0.0, 1.0);
            let (sx, sy) = ((px - n / 2.0).abs() - n * 0.15, (py - n * 0.6).abs() - n * 0.3);
            let in_slot = (0.5 - sx.max(sy)).clamp(0.0, 1.0);
            let mix = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * in_slot).round() as u8;
            rgba.extend([mix(accent[0], slot[0]), mix(accent[1], slot[1]), mix(accent[2], slot[2]), (cover * 255.0).round() as u8]);
        }
    }
    egui::IconData { rgba, width: SIZE, height: SIZE }
}

#[cfg(test)]
mod icon_tests {
    #[test]
    fn the_window_icon_is_the_mark() {
        let icon = super::app_icon();
        assert_eq!((icon.width, icon.height, icon.rgba.len()), (64, 64, 64 * 64 * 4));
        let at = |x: usize, y: usize| &icon.rgba[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4];
        assert_eq!(at(0, 0)[3], 0, "the corners are rounded off");
        assert_eq!(at(8, 32), [0x2f, 0xb8, 0xb0, 255], "the square");
        assert_eq!(at(32, 40), [0x1d, 0x21, 0x27, 255], "the slot");
    }
}
