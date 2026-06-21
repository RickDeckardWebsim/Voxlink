// ─────────────────────────────────────────────────────────────────────────────
// ui/components.rs — Reusable UI widgets  (egui 0.34 compatible)
// ─────────────────────────────────────────────────────────────────────────────

use egui::{Color32, CornerRadius, FontId, Painter, Pos2, Rect, Response, RichText, Ui, Vec2};

use crate::state::{ChatMessage, MessageKind};
use super::theme;

// ── Avatar ────────────────────────────────────────────────────────────────────

pub fn draw_avatar(ui: &mut Ui, username: &str, size: f32) -> Rect {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let color = theme::avatar_color(username);
        painter.circle_filled(rect.center(), size / 2.0, color);
        let letter = username.chars().next().unwrap_or('?').to_uppercase().next().unwrap_or('?');
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            letter.to_string(),
            FontId::proportional(size * 0.44),
            Color32::WHITE,
        );
    }

    rect
}

// ── Status Dot ────────────────────────────────────────────────────────────────

pub fn draw_status_dot(painter: &Painter, center: Pos2, radius: f32, color: Color32) {
    painter.circle_filled(center, radius + 2.0, theme::SIDEBAR_BG);
    painter.circle_filled(center, radius, color);
}

// ── Message Bubble ────────────────────────────────────────────────────────────

pub fn render_message(ui: &mut Ui, msg: &ChatMessage, show_header: bool) {
    match msg.kind {
        MessageKind::System => render_system_message(ui, msg),
        _ => render_chat_message(ui, msg, show_header),
    }
}

fn render_system_message(ui: &mut Ui, msg: &ChatMessage) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        let (lr, _) = ui.allocate_exact_size(Vec2::new(32.0, 1.0), egui::Sense::hover());
        if ui.is_rect_visible(lr) {
            ui.painter()
                .hline(lr.x_range(), lr.center().y, egui::Stroke::new(1.0, theme::SEPARATOR));
        }
        ui.add_space(8.0);
        ui.label(
            RichText::new(&msg.content)
                .size(12.0)
                .color(theme::TEXT_SYSTEM)
                .italics(),
        );
        ui.add_space(8.0);
        let remaining = (ui.available_width() - 16.0).max(0.0);
        let (rr, _) = ui.allocate_exact_size(Vec2::new(remaining, 1.0), egui::Sense::hover());
        if ui.is_rect_visible(rr) {
            ui.painter()
                .hline(rr.x_range(), rr.center().y, egui::Stroke::new(1.0, theme::SEPARATOR));
        }
    });
    ui.add_space(4.0);
}

fn render_chat_message(ui: &mut Ui, msg: &ChatMessage, show_header: bool) {
    ui.add_space(if show_header { 12.0 } else { 1.0 });

    let author_color = match msg.kind {
        MessageKind::Own => theme::TEXT_OWN_AUTHOR,
        _                => theme::TEXT_PEER_AUTHOR,
    };

    ui.horizontal_top(|ui| {
        ui.add_space(12.0);

        if show_header {
            draw_avatar(ui, &msg.author, theme::AVATAR_SIZE);
            ui.add_space(8.0);
        } else {
            ui.add_space(theme::AVATAR_SIZE + 8.0);
        }

        ui.vertical(|ui| {
            if show_header {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&msg.author).size(15.0).color(author_color).strong(),
                    );
                    ui.label(
                        RichText::new(&msg.timestamp).size(11.0).color(theme::TEXT_MUTED),
                    );
                });
            }
            ui.add(
                egui::Label::new(
                    RichText::new(&msg.content).size(14.0).color(theme::TEXT_PRIMARY),
                )
                .wrap_mode(egui::TextWrapMode::Wrap),
            );
        });

        ui.add_space(12.0);
    });
}

// ── Buttons ───────────────────────────────────────────────────────────────────

/// Filled blurple accent button.
pub fn accent_button(ui: &mut Ui, label: &str) -> Response {
    let btn = egui::Button::new(RichText::new(label).color(Color32::WHITE).size(14.0))
        .fill(theme::BLURPLE)
        .corner_radius(CornerRadius::same(theme::CORNER_RADIUS))
        .min_size(Vec2::new(140.0, 38.0));

    let response = ui.add(btn);
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

/// Transparent ghost button with a border.
#[allow(dead_code)] // used in Phase 3+ dialogs
pub fn ghost_button(ui: &mut Ui, label: &str) -> Response {
    let btn = egui::Button::new(RichText::new(label).color(theme::TEXT_PRIMARY).size(13.0))
        .fill(Color32::TRANSPARENT)
        .stroke(egui::Stroke::new(1.0, theme::ELEVATED_BG))
        .corner_radius(CornerRadius::same(theme::CORNER_RADIUS));
    ui.add(btn)
}

// ── Sidebar User Row ──────────────────────────────────────────────────────────

pub fn sidebar_user_row(ui: &mut Ui, username: &str, is_self: bool, voice_active: bool) {
    egui::Frame::default()
        .fill(Color32::TRANSPARENT)
        .corner_radius(CornerRadius::same(6u8))
        .inner_margin(egui::Margin::symmetric(8i8, 4i8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let avatar_rect = draw_avatar(ui, username, 28.0);
                let dot_center  = avatar_rect.right_bottom() + Vec2::new(-2.0, -2.0);
                draw_status_dot(ui.painter(), dot_center, 5.0, theme::GREEN_ONLINE);
                ui.add_space(4.0);

                let display = if is_self {
                    format!("{} (you)", username)
                } else {
                    username.to_string()
                };
                ui.label(
                    RichText::new(display)
                        .size(13.0)
                        .color(if is_self { theme::TEXT_MUTED } else { theme::TEXT_PRIMARY }),
                );

                if voice_active {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new("🎙").size(12.0));
                    });
                }
            });
        });
}

// ── Voice Toggle ──────────────────────────────────────────────────────────────

/// Returns `true` if clicked (toggled) this frame.
pub fn voice_toggle_button(ui: &mut Ui, active: bool) -> bool {
    let (icon, label, fill, text_color) = if active {
        ("🔴", " Disconnect Voice", theme::RED_DANGER, Color32::WHITE)
    } else {
        ("🎙", " Connect Voice", Color32::TRANSPARENT, theme::TEXT_PRIMARY)
    };

    let btn = egui::Button::new(
        RichText::new(format!("{}{}", icon, label))
            .color(text_color)
            .size(13.0),
    )
    .fill(fill)
    .stroke(egui::Stroke::new(if active { 0.0 } else { 1.0 }, theme::ELEVATED_BG))
    .corner_radius(CornerRadius::same(6u8))
    .min_size(Vec2::new(ui.available_width(), 34.0));

    ui.add(btn).clicked()
}
