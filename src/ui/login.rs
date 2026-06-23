// ─────────────────────────────────────────────────────────────────────────────
// ui/login.rs — Email/Password Login & Registration
// ─────────────────────────────────────────────────────────────────────────────

use egui::{Color32, CornerRadius, Frame, Key, Margin, RichText, Vec2};
use std::sync::mpsc;
use std::thread;

use crate::state::{AppState, Screen, Session};
use crate::net::supabase;
use super::{components, theme};

pub fn render(ctx: &egui::Context, state: &mut AppState) {
    // Poll for auth result
    if let Some(rx) = &state.auth_rx {
        if let Ok(result) = rx.try_recv() {
            state.auth_in_progress = false;
            state.auth_rx = None;
            
            match result {
                Ok(session) => {
                    session.save(); // Persist to disk
                    state.session = Some(session.clone());
                    state.username = session.username.clone();
                    state.push_system(format!("Welcome {}, connecting to signaling…", session.username));
                    state.screen = Screen::Chat;
                    state.peers.clear();
                    state.needs_connect = true; // triggers signaling spawn
                }
                Err(e) => {
                    state.auth_error = Some(e);
                }
            }
        }
    }

    egui::CentralPanel::default()
        .frame(Frame::default().fill(theme::DARK_BG))
        .show(ctx, |ui| {
            let available = ui.max_rect();
            let safe = theme::SAFE_MARGIN;

            // Card dimensions scale down when window is small, always maintaining
            // at least SAFE_MARGIN clearance on every side.
            let ideal_card_h = if state.is_registering { 540.0_f32 } else { 480.0_f32 };
            let card_w = (available.width()  - 2.0 * safe).min(420.0_f32).max(280.0);
            let card_h = (available.height() - 2.0 * safe).min(ideal_card_h).max(200.0);

            // Scale inner padding proportionally so content never overflows the card.
            let card_pad_f32 = (card_h / ideal_card_h * 40.0).clamp(12.0, 40.0);
            let card_pad = card_pad_f32.round() as i8;

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

            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(card_rect), |ui| {
                Frame::default()
                    .fill(theme::SIDEBAR_BG)
                    .corner_radius(CornerRadius::same(16u8))
                    .inner_margin(Margin::same(card_pad))
                    .shadow(egui::epaint::Shadow {
                        offset: [0i8, 12i8],
                        blur:   40u8,
                        spread: 0u8,
                        color:  Color32::from_black_alpha(100),
                    })
                    .show(ui, |ui| {
                        let inner_w = (card_w - 2.0 * card_pad_f32).max(0.0);
                        let inner_h = (card_h - 2.0 * card_pad_f32).max(0.0);
                        ui.set_min_size(Vec2::new(inner_w, inner_h));
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
        ui.label(RichText::new(if state.is_registering { "Create an Account" } else { "Welcome Back" }).size(26.0).color(Color32::WHITE).strong());
        ui.label(
            RichText::new("Private P2P voice & text — zero cost")
                .size(13.0)
                .color(theme::TEXT_MUTED),
        );

        ui.add_space(28.0);
    });

    ui.vertical(|ui| {
        let mut enter_pressed = false;

        // Email field
        ui.label(RichText::new("EMAIL").size(11.0).color(theme::TEXT_MUTED).strong());
        ui.add_space(4.0);
        let email_id = egui::Id::new("login_email_field");
        let resp = ui.add(
            egui::TextEdit::singleline(&mut state.email_input)
                .id(email_id)
                .hint_text("you@example.com")
                .desired_width(f32::INFINITY)
                .font(egui::TextStyle::Body)
                .margin(egui::Margin::symmetric(12i8, 8i8)),
        );
        if state.focus_input {
            ctx.memory_mut(|m| m.request_focus(email_id));
            state.focus_input = false;
        }
        if resp.lost_focus() && ctx.input(|i| i.key_pressed(Key::Enter)) { enter_pressed = true; }
        ui.add_space(12.0);

        // Username field (only if registering)
        if state.is_registering {
            ui.label(RichText::new("USERNAME").size(11.0).color(theme::TEXT_MUTED).strong());
            ui.add_space(4.0);
            let resp = ui.add(
                egui::TextEdit::singleline(&mut state.username_input)
                    .hint_text("Display Name")
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Body)
                    .margin(egui::Margin::symmetric(12i8, 8i8)),
            );
            if resp.lost_focus() && ctx.input(|i| i.key_pressed(Key::Enter)) { enter_pressed = true; }
            ui.add_space(12.0);
        }

        // Password field
        ui.label(RichText::new("PASSWORD").size(11.0).color(theme::TEXT_MUTED).strong());
        ui.add_space(4.0);
        let resp = ui.add(
            egui::TextEdit::singleline(&mut state.password_input)
                .password(true)
                .hint_text("••••••••")
                .desired_width(f32::INFINITY)
                .font(egui::TextStyle::Body)
                .margin(egui::Margin::symmetric(12i8, 8i8)),
        );
        if resp.lost_focus() && ctx.input(|i| i.key_pressed(Key::Enter)) { enter_pressed = true; }

        ui.add_space(16.0);

        // Error message
        if let Some(err) = &state.auth_error {
            ui.label(RichText::new(err).color(theme::RED_DANGER).size(13.0));
            ui.add_space(8.0);
        }

        ui.vertical_centered(|ui| {
            if state.auth_in_progress {
                ui.spinner();
            } else {
                let btn_text = if state.is_registering { "Register" } else { "Login" };
                let is_valid = !state.email_input.is_empty() && !state.password_input.is_empty() && (!state.is_registering || !state.username_input.is_empty());
                
                ui.add_enabled_ui(is_valid, |ui| {
                    if components::accent_button(ui, btn_text).clicked() || (is_valid && enter_pressed) {
                        commit_auth(state, ctx);
                    }
                });
            }

            ui.add_space(16.0);
            
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(RichText::new(if state.is_registering { "Already have an account?" } else { "Need an account?" }).color(theme::TEXT_MUTED).size(12.0));
                
                if ui.link(RichText::new(if state.is_registering { "Login" } else { "Register" }).color(theme::BLURPLE).size(12.0)).clicked() {
                    state.is_registering = !state.is_registering;
                    state.auth_error = None;
                }
            });
        });
    });
}

fn commit_auth(state: &mut AppState, ctx: &egui::Context) {
    state.auth_in_progress = true;
    state.auth_error = None;

    let email = state.email_input.clone();
    let password = state.password_input.clone();
    let username = state.username_input.clone();
    let is_registering = state.is_registering;

    let (tx, rx) = mpsc::channel();
    state.auth_rx = Some(rx);
    let ctx_clone = ctx.clone();

    thread::spawn(move || {
        let result = if is_registering {
            supabase::sign_up(&email, &password, &username)
        } else {
            supabase::sign_in(&email, &password)
        };

        match result {
            Ok(auth_res) => {
                // If it's sign in, we also need to fetch their profile to get their username and avatar
                let (uname, avatar) = if !is_registering {
                    match supabase::get_profile(&auth_res.user.id, &auth_res.access_token) {
                        Ok(prof) => (prof.username, prof.avatar_url),
                        Err(_) => (email.clone(), None) // Fallback if no profile
                    }
                } else {
                    (username, None)
                };

                let session = Session {
                    access_token: auth_res.access_token,
                    refresh_token: auth_res.refresh_token,
                    user_id: auth_res.user.id,
                    email,
                    username: uname,
                    avatar_url: avatar,
                };
                let _ = tx.send(Ok(session));
            }
            Err(e) => {
                let _ = tx.send(Err(e.to_string()));
            }
        }
        ctx_clone.request_repaint();
    });
}

fn draw_logo(painter: &egui::Painter, center: egui::Pos2) {
    let r = 28.0_f32;
    painter.circle_filled(center, r, theme::BLURPLE);
    painter.circle_stroke(center, r - 2.0, egui::Stroke::new(1.0, Color32::from_white_alpha(30)));
    let stroke = egui::Stroke::new(3.5, Color32::WHITE);
    painter.line_segment([center + Vec2::new(-12.0, -8.0), center + Vec2::new(0.0, 10.0)], stroke);
    painter.line_segment([center + Vec2::new(12.0, -8.0),  center + Vec2::new(0.0, 10.0)], stroke);
}
