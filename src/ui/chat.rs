// ─────────────────────────────────────────────────────────────────────────────
// ui/chat.rs — Main chat interface  (egui 0.34 compatible)
// ─────────────────────────────────────────────────────────────────────────────

use egui::{Color32, CornerRadius, Frame, Key, Margin, RichText, ScrollArea, Vec2};

use crate::state::{AppState, MessageKind};
use super::{components, theme};

#[allow(deprecated)] // egui 0.34: Panel::show still works, show_inside() is new preferred API
pub fn render(ctx: &egui::Context, state: &mut AppState) {
    // ── 1. Left sidebar panel ─────────────────────────────────────────────────
    egui::SidePanel::left("sidebar")
        .exact_size(theme::SIDEBAR_WIDTH)
        .resizable(false)
        .frame(Frame::default().fill(theme::SIDEBAR_BG).inner_margin(Margin::same(0i8)))
        .show(ctx, |ui| {
            render_sidebar(ui, state);
        });

    // ── 2. Channel header (top panel) ─────────────────────────────────────────
    egui::TopBottomPanel::top("channel_header")
        .exact_size(theme::CHANNEL_HEADER_HEIGHT)
        .frame(
            Frame::default()
                .fill(theme::DARK_BG)
                .inner_margin(Margin::symmetric(16i8, 0i8))
                .stroke(egui::Stroke::new(1.0, theme::SEPARATOR)),
        )
        .show(ctx, |ui| {
            render_channel_header(ui, state);
        });

    // ── 3. Central panel — messages + input ───────────────────────────────────
    egui::CentralPanel::default()
        .frame(Frame::default().fill(theme::DARK_BG).inner_margin(Margin::same(0i8)))
        .show(ctx, |ui| {
            render_message_area(ctx, ui, state);
        });
}

// ── Sidebar ───────────────────────────────────────────────────────────────────

fn render_sidebar(ui: &mut egui::Ui, state: &mut AppState) {
    // App header
    Frame::default()
        .fill(theme::HEADER_BG)
        .inner_margin(Margin::symmetric(16i8, 14i8))
        .stroke(egui::Stroke::new(1.0, theme::SEPARATOR))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                let (lr, _) = ui.allocate_exact_size(Vec2::splat(22.0), egui::Sense::hover());
                if ui.is_rect_visible(lr) {
                    ui.painter().circle_filled(lr.center(), 11.0, theme::BLURPLE);
                    ui.painter().text(
                        lr.center(),
                        egui::Align2::CENTER_CENTER,
                        "V",
                        egui::FontId::proportional(13.0),
                        Color32::WHITE,
                    );
                }
                ui.add_space(6.0);
                ui.label(RichText::new("VoxLink").size(15.0).color(Color32::WHITE).strong());
            });
        });

    // Scrollable user / channel list — reserve space for bottom control bar
    let bottom_h = 96.0;
    let scroll_max = (ui.available_height() - bottom_h).max(40.0);
    ScrollArea::vertical()
        .id_salt("sidebar_scroll")
        .auto_shrink([false, false])
        .max_height(scroll_max)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.add_space(8.0);

            sidebar_section_header(ui, "CHANNELS");
            sidebar_channel_item(ui, "general", true);

            ui.add_space(12.0);

            let online = state.peers.len() + 1;
            sidebar_section_header(ui, &format!("ONLINE — {}", online));

            ui.add_space(2.0);
            components::sidebar_user_row(ui, &state.username, true, state.voice_active);

            let peers = state.peers.clone();
            for peer in &peers {
                ui.add_space(2.0);
                components::sidebar_user_row(ui, &peer.username, false, peer.voice_active);
            }
            ui.add_space(12.0);
        });

    // Push bottom bar to the very bottom
    let remaining = ui.available_height() - bottom_h;
    if remaining > 0.0 {
        ui.add_space(remaining);
    }

    Frame::default()
        .fill(theme::HEADER_BG)
        .inner_margin(Margin::symmetric(10i8, 10i8))
        .stroke(egui::Stroke::new(1.0, theme::SEPARATOR))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.vertical(|ui| {
                if components::voice_toggle_button(ui, state.voice_active) {
                    state.voice_active = !state.voice_active;
                    if let Some(tx) = &state.cmd_tx {
                        let _ = tx.send(crate::state::UiCommand::ToggleVoice(state.voice_active));
                    }
                    state.push_system(if state.voice_active {
                        "🎙  Voice connected — streaming audio P2P"
                    } else {
                        "🔇  Voice disconnected."
                    });
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    components::draw_avatar(ui, &state.username, 28.0);
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(&state.username).size(13.0).color(Color32::WHITE).strong(),
                        );
                        ui.label(
                            RichText::new(if state.voice_active { "🟢 In voice" } else { "● Online" })
                                .size(11.0)
                                .color(theme::TEXT_MUTED),
                        );
                    });
                });
            });
        });
}

fn sidebar_section_header(ui: &mut egui::Ui, title: &str) {
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(RichText::new(title).size(11.0).color(theme::TEXT_MUTED).strong());
    });
    ui.add_space(2.0);
}

fn sidebar_channel_item(ui: &mut egui::Ui, name: &str, active: bool) {
    Frame::default()
        .fill(if active { theme::ACTIVE_BG } else { Color32::TRANSPARENT })
        .corner_radius(CornerRadius::same(6u8))
        .inner_margin(Margin::symmetric(8i8, 4i8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width() - 16.0);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!("# {}", name))
                        .size(14.0)
                        .color(if active { Color32::WHITE } else { theme::TEXT_MUTED }),
                );
            });
        });
}

// ── Channel Header ────────────────────────────────────────────────────────────

fn render_channel_header(ui: &mut egui::Ui, state: &AppState) {
    ui.vertical_centered_justified(|ui| {
        ui.set_height(theme::CHANNEL_HEADER_HEIGHT);
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(RichText::new("#").size(18.0).color(theme::TEXT_MUTED).strong());
            ui.add_space(4.0);
            ui.label(RichText::new("general").size(15.0).color(Color32::WHITE).strong());
            ui.label(RichText::new("|").size(16.0).color(theme::SEPARATOR));
            ui.label(
                RichText::new("VoxLink P2P — messages route directly between you and your peers")
                    .size(13.0)
                    .color(theme::TEXT_MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("👥 {}", state.peers.len() + 1))
                        .size(13.0)
                        .color(theme::TEXT_MUTED),
                );
                ui.add_space(8.0);
            });
        });
    });
}

// ── Message Area ─────────────────────────────────────────────────────────────

fn render_message_area(ctx: &egui::Context, ui: &mut egui::Ui, state: &mut AppState) {
    let input_height  = theme::INPUT_BAR_HEIGHT;
    let msg_height    = (ui.available_height() - input_height).max(0.0);

    // Messages scroll area
    ScrollArea::vertical()
        .id_salt("messages_scroll")
        .auto_shrink([false, false])
        .max_height(msg_height)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            ui.set_min_height(msg_height);
            ui.add_space(8.0);

            let messages = state.messages.clone();
            let mut prev_author: Option<&str> = None;
            let mut prev_kind:   Option<&MessageKind> = None;

            for msg in &messages {
                let is_system = msg.kind == MessageKind::System;
                let same_author = prev_author == Some(msg.author.as_str())
                    && prev_kind == Some(&msg.kind)
                    && !is_system;

                components::render_message(ui, msg, !same_author);

                prev_author = Some(msg.author.as_str());
                prev_kind   = Some(&msg.kind);
            }

            // Force-scroll to bottom when a new message arrives
            if state.scroll_to_bottom {
                ui.scroll_to_cursor(Some(egui::Align::Max));
                state.scroll_to_bottom = false;
            }

            ui.add_space(8.0);
        });

    // Input bar
    Frame::default()
        .fill(theme::DARK_BG)
        .inner_margin(Margin::symmetric(16i8, 10i8))
        .stroke(egui::Stroke::new(1.0, theme::SEPARATOR))
        .show(ui, |ui| {
            render_input_bar(ctx, ui, state);
        });
}

fn render_input_bar(ctx: &egui::Context, ui: &mut egui::Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        // Input field
        Frame::default()
            .fill(theme::INPUT_BG)
            .corner_radius(CornerRadius::same(10u8))
            .inner_margin(Margin::symmetric(12i8, 8i8))
            .show(ui, |ui| {
                let input_id   = egui::Id::new("message_input_field");
                let avail_w    = (ui.available_width() - 56.0).max(100.0);

                let response = ui.add(
                    egui::TextEdit::singleline(&mut state.message_input)
                        .id(input_id)
                        .hint_text("Message #general…")
                        .desired_width(avail_w)
                        .font(egui::TextStyle::Body),
                );

                if response.lost_focus() && ctx.input(|i| i.key_pressed(Key::Enter)) {
                    try_send_message(state);
                    ctx.memory_mut(|m| m.request_focus(input_id));
                }
            });

        ui.add_space(6.0);

        // Send button
        let can_send = !state.message_input.trim().is_empty();
        ui.add_enabled_ui(can_send, |ui| {
            let btn = egui::Button::new(RichText::new("➤").size(18.0).color(Color32::WHITE))
                .fill(if can_send { theme::BLURPLE } else { theme::ELEVATED_BG })
                .corner_radius(CornerRadius::same(10u8))
                .min_size(Vec2::new(40.0, 40.0));

            if ui.add(btn).clicked() {
                try_send_message(state);
            }
        });
    });
}

fn try_send_message(state: &mut AppState) {
    let content = state.message_input.trim().to_string();
    if content.is_empty() {
        return;
    }
    state.message_input.clear();

    // Phase 2: route through signaling task → Supabase broadcast → peers
    // Phase 3: this will change to cmd_tx.send(UiCommand::SendMessage) → P2P data channel
    if let Some(tx) = &state.cmd_tx {
        let _ = tx.send(crate::state::UiCommand::SendMessage(content.clone()));
    }

    // Always show own message locally (optimistic UI — sender doesn't receive own broadcast)
    state.push_own(content);
}
