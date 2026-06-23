// ─────────────────────────────────────────────────────────────────────────────
// ui/components.rs — Reusable UI widgets  (egui 0.34 compatible)
// ─────────────────────────────────────────────────────────────────────────────

use egui::{Color32, CornerRadius, FontId, Painter, Pos2, Rect, Response, RichText, Ui, Vec2};

use crate::state::{ChatMessage, MessageKind};
use super::theme;

// ── Avatar ────────────────────────────────────────────────────────────────────

pub fn draw_avatar(ui: &mut Ui, username: &str, avatar_url: Option<&str>, size: f32) -> Rect {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());

    if ui.is_rect_visible(rect) {
        let mut drawn_image = false;
        
        if let Some(url) = avatar_url {
            if let Some(tex) = super::image_loader::get_avatar_texture(ui.ctx(), url) {
                let mut mesh = egui::Mesh::with_texture(tex.id());
                let color = Color32::WHITE;
                let center = rect.center();
                let r = size / 2.0;
                let uv_r = 0.5;
                let uv_center = Pos2::new(0.5, 0.5);
                
                let n = 32;
                for i in 0..n {
                    let a0 = i as f32 * std::f32::consts::TAU / n as f32;
                    let a1 = (i + 1) as f32 * std::f32::consts::TAU / n as f32;
                    let p0 = center + Vec2::new(a0.cos(), a0.sin()) * r;
                    let p1 = center + Vec2::new(a1.cos(), a1.sin()) * r;
                    let uv0 = uv_center + Vec2::new(a0.cos(), a0.sin()) * uv_r;
                    let uv1 = uv_center + Vec2::new(a1.cos(), a1.sin()) * uv_r;
                    mesh.add_triangle(2, 0, 1);
                    mesh.vertices.push(egui::epaint::Vertex { pos: p0, uv: uv0, color });
                    mesh.vertices.push(egui::epaint::Vertex { pos: p1, uv: uv1, color });
                    mesh.vertices.push(egui::epaint::Vertex { pos: center, uv: uv_center, color });
                }
                ui.painter().add(mesh);
                drawn_image = true;
            }
        }
        
        if !drawn_image {
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
        ui.label(
            RichText::new("→")
                .size(13.0)
                .color(theme::TEXT_SYSTEM)
                .strong(),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(&msg.content)
                .size(13.0)
                .color(theme::TEXT_SYSTEM)
                .italics(),
        );
    });
    ui.add_space(4.0);
}

fn render_chat_message(ui: &mut Ui, msg: &ChatMessage, show_header: bool) {
    ui.add_space(if show_header { 10.0 } else { 1.0 });

    let author_color = theme::avatar_color(&msg.author);

    ui.horizontal_top(|ui| {
        ui.add_space(12.0);

        if show_header {
            // In a real app we'd fetch the peer's avatar from state, but ChatMessage doesn't store avatar_url currently.
            // Let's modify components.rs later if needed, but for now fallback to None for chat messages
            draw_avatar(ui, &msg.author, None, theme::AVATAR_SIZE);
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
            if let Some(ref att) = msg.attachment {
                render_attachment(ui, att);
            }
        });

        ui.add_space(12.0);
    });
}

fn render_attachment(ui: &mut Ui, att: &crate::state::Attachment) {
    ui.add_space(4.0);
    match att.kind {
        crate::state::AttachmentKind::Image => {
            if let Some(tex) = super::image_loader::get_avatar_texture(ui.ctx(), &att.url) {
                let nat   = tex.size_vec2();
                let max_w = ui.available_width().min(420.0);
                let scale = if nat.x > 0.0 { (max_w / nat.x).min(1.0) } else { 1.0 };
                let mut display = nat * scale;
                if display.y > 320.0 {
                    display = display * (320.0 / display.y);
                }
                let sized = egui::load::SizedTexture::new(tex.id(), display);
                ui.add(egui::Image::new(sized));
            } else {
                ui.label(RichText::new("Loading image…").size(13.0).color(theme::TEXT_MUTED));
            }
        }
        crate::state::AttachmentKind::Audio => {
            egui::Frame::default()
                .fill(theme::ELEVATED_BG)
                .corner_radius(CornerRadius::same(8u8))
                .inner_margin(egui::Margin::symmetric(12i8, 8i8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // "♪" is U+266A, well within the BMP and present in most fonts
                        ui.label(RichText::new("\u{266A} Audio").size(13.0).color(theme::TEXT_MUTED));
                        ui.add_space(6.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(&att.filename).size(13.0).color(theme::TEXT_PRIMARY)
                            )
                            .wrap_mode(egui::TextWrapMode::Truncate),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Open").clicked() {
                                open_externally(&att.url);
                            }
                        });
                    });
                });
        }
        crate::state::AttachmentKind::Video => {
            egui::Frame::default()
                .fill(theme::ELEVATED_BG)
                .corner_radius(CornerRadius::same(8u8))
                .inner_margin(egui::Margin::symmetric(12i8, 8i8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // "▶" is U+25B6, present in all standard fonts
                        ui.label(RichText::new("\u{25B6} Video").size(13.0).color(theme::TEXT_MUTED));
                        ui.add_space(6.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(&att.filename).size(13.0).color(theme::TEXT_PRIMARY)
                            )
                            .wrap_mode(egui::TextWrapMode::Truncate),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Open").clicked() {
                                open_externally(&att.url);
                            }
                        });
                    });
                });
        }
    }
}

fn open_externally(url: &str) {
    #[cfg(target_os = "windows")]
    { let _ = std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn(); }
    #[cfg(target_os = "macos")]
    { let _ = std::process::Command::new("open").arg(url).spawn(); }
    #[cfg(target_os = "linux")]
    { let _ = std::process::Command::new("xdg-open").arg(url).spawn(); }
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

pub fn sidebar_user_row(ui: &mut Ui, username: &str, avatar_url: Option<&str>, is_self: bool, voice_active: bool) {
    egui::Frame::default()
        .fill(Color32::TRANSPARENT)
        .corner_radius(CornerRadius::same(6u8))
        .inner_margin(egui::Margin::symmetric(8i8, 4i8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let avatar_rect = draw_avatar(ui, username, avatar_url, 28.0);
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
                        // U+25CF BLACK CIRCLE — universally renderable BMP character
                        ui.label(RichText::new("\u{25CF}").size(10.0).color(theme::GREEN_ONLINE));
                    });
                }
            });
        });
}

// ── Voice Toggle ──────────────────────────────────────────────────────────────

/// Returns `true` if clicked (toggled) this frame.
#[allow(dead_code)]
pub fn voice_toggle_button(ui: &mut Ui, active: bool) -> bool {
    let (label, fill, text_color) = if active {
        ("Disconnect Voice", theme::RED_DANGER, Color32::WHITE)
    } else {
        ("Connect Voice", Color32::TRANSPARENT, theme::TEXT_PRIMARY)
    };

    let btn = egui::Button::new(
        RichText::new(label)
            .color(text_color)
            .size(13.0),
    )
    .fill(fill)
    .stroke(egui::Stroke::new(if active { 0.0 } else { 1.0 }, theme::ELEVATED_BG))
    .corner_radius(CornerRadius::same(6u8))
    .min_size(Vec2::new(ui.available_width(), 34.0));

    ui.add(btn).clicked()
}
