// ─────────────────────────────────────────────────────────────────────────────
// ui/theme.rs — VoxLink visual identity  (egui 0.34 compatible)
// ─────────────────────────────────────────────────────────────────────────────

use egui::{Color32, CornerRadius, FontFamily, FontId, Margin, Stroke, Style, TextStyle, Visuals};

// ── Color Palette ─────────────────────────────────────────────────────────────

pub const DARK_BG: Color32        = Color32::from_rgb(0x1e, 0x1f, 0x22);
pub const SIDEBAR_BG: Color32     = Color32::from_rgb(0x2b, 0x2d, 0x31);
pub const ELEVATED_BG: Color32    = Color32::from_rgb(0x38, 0x3a, 0x40);
pub const HOVER_BG: Color32       = Color32::from_rgb(0x35, 0x37, 0x3c);
pub const ACTIVE_BG: Color32      = Color32::from_rgb(0x40, 0x43, 0x4a);
pub const INPUT_BG: Color32       = Color32::from_rgb(0x48, 0x4b, 0x54);
pub const HEADER_BG: Color32      = Color32::from_rgb(0x23, 0x24, 0x28);
pub const BLURPLE: Color32        = Color32::from_rgb(0x58, 0x65, 0xf2);
pub const GREEN_ONLINE: Color32   = Color32::from_rgb(0x23, 0xa5, 0x5a);
pub const RED_DANGER: Color32     = Color32::from_rgb(0xed, 0x42, 0x45);
pub const TEXT_PRIMARY: Color32   = Color32::from_rgb(0xdb, 0xde, 0xe1);
pub const TEXT_MUTED: Color32     = Color32::from_rgb(0x80, 0x84, 0x8e);
pub const SEPARATOR: Color32      = Color32::from_rgb(0x1a, 0x1b, 0x1f);
pub const TEXT_SYSTEM: Color32    = Color32::from_rgb(0x72, 0x76, 0x7d);
pub const TEXT_OWN_AUTHOR: Color32  = BLURPLE;
pub const TEXT_PEER_AUTHOR: Color32 = Color32::from_rgb(0xf2, 0xf3, 0xf5);

// ── Spacing ──────────────────────────────────────────────────────────────────

pub const SIDEBAR_WIDTH: f32          = 240.0;
pub const CHANNEL_HEADER_HEIGHT: f32  = 52.0;
pub const INPUT_BAR_HEIGHT: f32       = 68.0;
pub const AVATAR_SIZE: f32            = 36.0;
// Corner radius used with CornerRadius::same() — must be u8
pub const CORNER_RADIUS: u8 = 8;

// ── Avatar Colors ─────────────────────────────────────────────────────────────

const AVATAR_PALETTE: &[Color32] = &[
    Color32::from_rgb(0x58, 0x65, 0xf2),
    Color32::from_rgb(0x3b, 0xa5, 0x5d),
    Color32::from_rgb(0xeb, 0x45, 0x9e),
    Color32::from_rgb(0xf0, 0xb2, 0x32),
    Color32::from_rgb(0xed, 0x42, 0x45),
    Color32::from_rgb(0x17, 0xa8, 0xe3),
    Color32::from_rgb(0x9c, 0x59, 0xd1),
    Color32::from_rgb(0x1a, 0xbc, 0x9c),
];

pub fn avatar_color(username: &str) -> Color32 {
    let hash: usize = username
        .bytes()
        .fold(0usize, |acc, b| acc.wrapping_mul(31).wrapping_add(b as usize));
    AVATAR_PALETTE[hash % AVATAR_PALETTE.len()]
}

// ── Visuals ───────────────────────────────────────────────────────────────────

pub fn voxlink_visuals() -> Visuals {
    let mut v = Visuals::dark();

    v.panel_fill       = DARK_BG;
    v.window_fill      = SIDEBAR_BG;
    v.extreme_bg_color = Color32::from_rgb(0x11, 0x12, 0x14);
    v.faint_bg_color   = Color32::from_rgb(0x25, 0x27, 0x2b);
    v.code_bg_color    = Color32::from_rgb(0x2e, 0x31, 0x38);

    v.hyperlink_color  = BLURPLE;
    v.selection.bg_fill = Color32::from_rgba_premultiplied(0x58, 0x65, 0xf2, 0x55);
    v.selection.stroke  = Stroke::new(1.0, BLURPLE);

    let cr = CornerRadius::same(CORNER_RADIUS);

    // Non-interactive widgets
    v.widgets.noninteractive.bg_fill      = SIDEBAR_BG;
    v.widgets.noninteractive.weak_bg_fill = SIDEBAR_BG;
    v.widgets.noninteractive.bg_stroke    = Stroke::NONE;
    v.widgets.noninteractive.fg_stroke    = Stroke::new(1.0, TEXT_PRIMARY);
    v.widgets.noninteractive.corner_radius = cr;

    // Inactive (default button state)
    v.widgets.inactive.bg_fill      = ELEVATED_BG;
    v.widgets.inactive.weak_bg_fill = ELEVATED_BG;
    v.widgets.inactive.bg_stroke    = Stroke::NONE;
    v.widgets.inactive.fg_stroke    = Stroke::new(1.0, TEXT_PRIMARY);
    v.widgets.inactive.corner_radius = cr;

    // Hovered
    v.widgets.hovered.bg_fill      = HOVER_BG;
    v.widgets.hovered.weak_bg_fill = HOVER_BG;
    v.widgets.hovered.bg_stroke    = Stroke::NONE;
    v.widgets.hovered.fg_stroke    = Stroke::new(1.5, TEXT_PRIMARY);
    v.widgets.hovered.corner_radius = cr;
    v.widgets.hovered.expansion     = 1.0;

    // Active (pressed)
    v.widgets.active.bg_fill      = BLURPLE;
    v.widgets.active.weak_bg_fill = BLURPLE;
    v.widgets.active.fg_stroke    = Stroke::new(1.5, Color32::WHITE);
    v.widgets.active.corner_radius = cr;

    // Open (dropdown, combo)
    v.widgets.open.bg_fill      = ACTIVE_BG;
    v.widgets.open.fg_stroke    = Stroke::new(1.0, TEXT_PRIMARY);
    v.widgets.open.corner_radius = cr;

    // Window chrome — Shadow uses integer types in egui 0.34
    v.window_stroke = Stroke::new(1.0, SEPARATOR);
    v.window_shadow = egui::epaint::Shadow {
        offset:  [0i8, 8i8],
        blur:    24u8,
        spread:  0u8,
        color:   Color32::from_black_alpha(120),
    };

    v.text_cursor.stroke = Stroke::new(2.0, Color32::WHITE);

    v
}

/// Applies VoxLink font sizes to the global egui Style.
pub fn voxlink_style(style: &mut Style) {
    use FontFamily::Proportional;

    style.text_styles = [
        (TextStyle::Heading,   FontId::new(20.0, Proportional)),
        (TextStyle::Body,      FontId::new(14.0, Proportional)),
        (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
        (TextStyle::Button,    FontId::new(14.0, Proportional)),
        (TextStyle::Small,     FontId::new(11.0, Proportional)),
    ]
    .into();

    style.spacing.item_spacing   = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    style.spacing.window_margin  = Margin::same(0i8);
}
