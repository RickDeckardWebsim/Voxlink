// ─────────────────────────────────────────────────────────────────────────────
// ui/login.rs — Login / username entry screen  (egui 0.34 compatible)
// ─────────────────────────────────────────────────────────────────────────────

use egui::{Color32, CornerRadius, Frame, Key, Margin, RichText, Vec2};

use crate::state::{AppState, Screen};
use super::{components, theme};

#[allow(deprecated)] // egui 0.34: CentralPanel::show + allocate_new_ui still functional
#[allow(dead_code)] // used in Phase 3+
pub fn render(ctx: &egui::Context, state: &mut AppState) {
    egui::CentralPanel::default()
        .frame(Frame::default().fill(theme::DARK_BG))
        .show(ctx, |ui| {
            let available = ui.max_rect();
            let card_w = 420.0_f32;
            let card_h = 400.0_f32;
            let card_rect = egui::Rect::from_center_size(
                available.center(),
                Vec2::new(card_w, card_h),
            );

            // Ambient glow blobs behind the card
            let painter = ui.painter();
            painter.circle_filled(
                available.center() + Vec2::new(-60.0, -40.0),
                200.0,
                Color32::from_rgba_premultiplied(0x58, 0x65, 0xf2, 0x18),
            );
            painter.circle_filled(
                available.center() + Vec2::new(80.0, 60.0),
                150.0,
                Color32::from_rgba_premultiplied(0x3b, 0xa5, 0x5d, 0x10),
            );

            // Use allocate_new_ui (0.34 API) instead of deprecated allocate_ui_at_rect
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(card_rect), |ui| {
                Frame::default()
                    .fill(theme::SIDEBAR_BG)
                    .corner_radius(CornerRadius::same(16u8))
                    .inner_margin(Margin::same(40i8))
                    .shadow(egui::epaint::Shadow {
                        offset: [0i8, 12i8],
                        blur:   40u8,
                        spread: 0u8,
                        color:  Color32::from_black_alpha(100),
                    })
                    .show(ui, |ui| {
                        ui.set_min_size(Vec2::new(card_w - 80.0, card_h - 80.0));
                        login_card_content(ctx, ui, state);
                    });
            });
        });
}

fn login_card_content(ctx: &egui::Context, ui: &mut egui::Ui, state: &mut AppState) {
    ui.vertical_centered(|ui| {
        // Logo
        let (logo_rect, _) = ui.allocate_exact_size(Vec2::new(64.0, 64.0), egui::Sense::hover());
        draw_logo(ui.painter(), logo_rect.center());

        ui.add_space(16.0);
        ui.label(RichText::new("VoxLink").size(26.0).color(Color32::WHITE).strong());
        ui.label(
            RichText::new("Private P2P voice & text — zero servers, zero cost")
                .size(13.0)
                .color(theme::TEXT_MUTED),
        );

        ui.add_space(28.0);

        // Username label
        ui.label(
            RichText::new("USERNAME").size(11.0).color(theme::TEXT_MUTED).strong(),
        );
        ui.add_space(4.0);

        let field_id = egui::Id::new("login_username_field");
        let response = ui.add(
            egui::TextEdit::singleline(&mut state.username_input)
                .id(field_id)
                .hint_text("e.g. Alice")
                .desired_width(f32::INFINITY)
                .font(egui::TextStyle::Body)
                .margin(egui::Margin::symmetric(12i8, 8i8)),
        );

        // Auto-focus on first frame
        if state.focus_username {
            ctx.memory_mut(|m| m.request_focus(field_id));
            state.focus_username = false;
        }

        ui.add_space(8.0);

        let username = state.username_input.trim().to_string();
        let is_valid = !username.is_empty() && username.len() <= 32;

        if !state.username_input.is_empty() && !is_valid {
            ui.label(
                RichText::new("Username must be 1–32 characters")
                    .size(12.0)
                    .color(theme::RED_DANGER),
            );
            ui.add_space(4.0);
        }

        ui.add_space(4.0);
        ui.add_enabled_ui(is_valid, |ui| {
            let enter_pressed = response.lost_focus()
                && ctx.input(|i| i.key_pressed(Key::Enter));

            if components::accent_button(ui, "Enter VoxLink ✨").clicked() || enter_pressed {
                if is_valid {
                    commit_login(state, username);
                }
            }
        });

        ui.add_space(16.0);
        ui.label(
            RichText::new(
                "Your name is only shared with peers you connect to.\nNo account required.",
            )
            .size(11.0)
            .color(theme::TEXT_MUTED),
        );
    });
}

fn commit_login(state: &mut AppState, username: String) {
    state.username = username.clone();
    state.push_system(format!("You joined as {}. Connecting to signaling…", username));
    state.screen = Screen::Chat;
    state.peers.clear();
    state.needs_connect = true; // triggers signaling spawn in app.rs
}

fn draw_logo(painter: &egui::Painter, center: egui::Pos2) {
    let r = 28.0_f32;
    painter.circle_filled(center, r, theme::BLURPLE);
    painter.circle_stroke(center, r - 2.0, egui::Stroke::new(1.0, Color32::from_white_alpha(30)));
    let stroke = egui::Stroke::new(3.5, Color32::WHITE);
    painter.line_segment([center + Vec2::new(-12.0, -8.0), center + Vec2::new(0.0, 10.0)], stroke);
    painter.line_segment([center + Vec2::new(12.0, -8.0),  center + Vec2::new(0.0, 10.0)], stroke);
}
