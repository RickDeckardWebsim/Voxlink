// ─────────────────────────────────────────────────────────────────────────────
// ui/chat.rs — Main chat interface  (egui 0.34 compatible)
// ─────────────────────────────────────────────────────────────────────────────

use egui::{Color32, CornerRadius, Frame, Key, Margin, RichText, ScrollArea, Vec2};
use std::thread;

use crate::state::{AppState, MessageKind};
use super::{components, theme};

#[allow(deprecated)] // egui 0.34: Panel::show still works, show_inside() is new preferred API
pub fn render(ctx: &egui::Context, state: &mut AppState) {
    poll_media_upload(ctx, state);
    // ── 1. Left sidebar panel ─────────────────────────────────────────────────
    egui::SidePanel::left("sidebar")
        .exact_size(theme::SIDEBAR_WIDTH)
        .resizable(false)
        .frame(Frame::default().fill(theme::SIDEBAR_BG).inner_margin(Margin::same(0i8)))
        .show(ctx, |ui| {
            render_sidebar(ui, state);
        });

    // ── Update Banner ─────────────────────────────────────────────────────────
    if let Some(ref version) = state.update_available_version {
        egui::TopBottomPanel::top("update_banner")
            .exact_size(40.0)
            .frame(
                Frame::default()
                    .fill(Color32::from_rgb(45, 120, 60)) // Green banner
                    .inner_margin(Margin::symmetric(16i8, 0i8)),
            )
            .show(ctx, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if state.update_in_progress {
                        ui.spinner();
                        ui.add_space(8.0);
                        ui.label(RichText::new("Downloading update...").color(Color32::WHITE).strong());
                    } else if let Some(ref err) = state.update_error {
                        ui.label(RichText::new(format!("Update failed: {}", err)).color(theme::RED_DANGER).strong());
                    } else {
                        let btn = ui.add(
                            egui::Button::new(RichText::new("Update Now").color(Color32::WHITE).strong())
                                .fill(theme::BLURPLE)
                                .corner_radius(CornerRadius::same(4))
                        );
                        if btn.clicked() {
                            state.update_in_progress = true;
                            crate::net::updater::run_update(state.updater_tx.clone());
                        }
                        ui.add_space(8.0);
                        ui.label(RichText::new(format!("Version {} is available!", version)).color(Color32::WHITE).strong());
                    }
                });
            });
    }

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

    crate::ui::profile::render_modal(ctx, state);
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
    let bottom_h = theme::SIDEBAR_BOTTOM_H
        + if state.voice_active { theme::SIDEBAR_VOICE_H } else { 0.0 };
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
            let avatar_url = state.session.as_ref().and_then(|s| s.avatar_url.as_deref());
            components::sidebar_user_row(ui, &state.username, avatar_url, true, state.voice_active);

            let peers = state.peers.clone();
            for peer in &peers {
                ui.add_space(2.0);
                components::sidebar_user_row(ui, &peer.username, peer.avatar_url.as_deref(), false, peer.voice_active);
            }
            ui.add_space(12.0);
        });

    // Push bottom bar to the very bottom
    let remaining = ui.available_height() - bottom_h;
    if remaining > 0.0 {
        ui.add_space(remaining);
    }

    // Voice connection panel (only visible if active)
    if state.voice_active {
        Frame::default()
            .fill(theme::HEADER_BG)
            .inner_margin(Margin::symmetric(16i8, 8i8))
            .stroke(egui::Stroke::new(1.0, theme::SEPARATOR))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // U+25CF BLACK CIRCLE as a rendered mic indicator (BMP, always available)
                    ui.label(RichText::new("\u{25CF}").size(14.0).color(theme::GREEN_ONLINE));
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Voice Connected").size(13.0).color(theme::GREEN_ONLINE).strong());
                        ui.label(RichText::new("RTC / P2P").size(11.0).color(theme::TEXT_MUTED));
                    });
                });
            });
    }

    // Bottom profile bar
    Frame::default()
        .fill(theme::HEADER_BG)
        .inner_margin(Margin::symmetric(14i8, 12i8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                let avatar_url = state.session.as_ref().and_then(|s| s.avatar_url.as_deref());
                let resp = ui.horizontal(|ui| {
                    components::draw_avatar(ui, &state.username, avatar_url, 32.0);
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(&state.username).size(14.0).color(Color32::WHITE).strong(),
                        );
                        ui.horizontal(|ui| {
                            let color = if state.voice_active { theme::GREEN_ONLINE } else { theme::TEXT_PRIMARY };
                            let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                            ui.painter().circle_filled(rect.center(), 4.0, color);
                            ui.add_space(2.0);
                            ui.label(
                                RichText::new(if state.voice_active { "In voice" } else { "Online" })
                                    .size(11.0)
                                    .color(theme::TEXT_MUTED),
                            );
                        });
                    });
                }).response;

                if ui.interact(resp.rect, egui::Id::new("user_row_click"), egui::Sense::click()).clicked() {
                    state.show_profile_modal = true;
                }
                if ui.rect_contains_pointer(resp.rect) {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }

                // Voice Toggle button on the right
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Text labels: "[ ]" = off, "[•]" = live — no supplementary-plane emoji
                    let (btn_text, btn_color) = if state.voice_active {
                        ("[End]", theme::RED_DANGER)
                    } else {
                        ("[Mic]", theme::TEXT_PRIMARY)
                    };
                    let btn = egui::Button::new(
                        RichText::new(btn_text).size(13.0).color(btn_color)
                    ).fill(Color32::TRANSPARENT).stroke(egui::Stroke::NONE).corner_radius(CornerRadius::same(6u8));
                    
                    let response = ui.add(btn);
                    if response.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if response.clicked() {
                        state.voice_active = !state.voice_active;
                        if let Some(tx) = &state.cmd_tx {
                            let _ = tx.send(crate::state::UiCommand::ToggleVoice(state.voice_active));
                        }
                    }
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
            // RTL sub-layout: peer count pins to the right, description fills the rest
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(theme::SAFE_MARGIN);
                ui.label(
                    // U+25A3 WHITE SQUARE CONTAINING BLACK SMALL SQUARE — BMP "people" indicator
                    RichText::new(format!("{} online", state.peers.len() + 1))
                        .size(13.0)
                        .color(theme::TEXT_MUTED),
                );
                ui.add_space(8.0);
                // Description label takes all remaining width; truncates before overflowing
                ui.add(
                    egui::Label::new(
                        RichText::new("VoxLink P2P — messages route directly between you and your peers")
                            .size(13.0)
                            .color(theme::TEXT_MUTED),
                    )
                    .wrap_mode(egui::TextWrapMode::Truncate),
                );
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
            ui.spacing_mut().item_spacing.y = 2.0;
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

    // Input bar wrapper to add padding from the edges
    egui::Frame::NONE
        .inner_margin(Margin { left: 16, right: 16, top: 8, bottom: 24 })
        .show(ui, |ui| {
            render_input_bar(ctx, ui, state);
        });
}

fn render_input_bar(ctx: &egui::Context, ui: &mut egui::Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        Frame::default()
            .fill(theme::INPUT_BG)
            .corner_radius(CornerRadius::same(8u8))
            .inner_margin(Margin { left: 10, right: 14, top: 10, bottom: 10 })
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    // ── Attachment button ────────────────────────────────────
                    if state.media_in_progress {
                        ui.spinner();
                    } else {
                        // "+" is universally renderable; tooltip clarifies it's for attachments
                        let attach = ui.add(
                            egui::Button::new(
                                RichText::new("+").size(20.0).color(theme::TEXT_MUTED).strong(),
                            )
                            .fill(Color32::TRANSPARENT)
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(CornerRadius::same(4u8)),
                        )
                        .on_hover_text("Attach image, audio, or video");
                        if attach.hovered() { ctx.set_cursor_icon(egui::CursorIcon::PointingHand); }
                        if attach.clicked() { pick_and_upload_media(state, ctx); }
                    }
                    ui.add_space(6.0);

                    // ── Text input ───────────────────────────────────────────
                    let input_id = egui::Id::new("message_input_field");
                    let avail_w  = ui.available_width();
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut state.message_input)
                            .id(input_id)
                            .hint_text("Message #general…")
                            .desired_width(avail_w)
                            .font(egui::FontId::proportional(15.0))
                            .frame(egui::Frame::NONE),
                    );
                    if response.lost_focus() && ctx.input(|i| i.key_pressed(Key::Enter)) {
                        try_send_message(state);
                        ctx.memory_mut(|m| m.request_focus(input_id));
                    }
                });
            });
    });
}

// ── Media upload polling ──────────────────────────────────────────────────────

fn poll_media_upload(ctx: &egui::Context, state: &mut AppState) {
    let result = state.media_rx.as_ref().and_then(|rx| rx.try_recv().ok());
    if let Some(result) = result {
        state.media_in_progress = false;
        state.media_rx = None;
        match result {
            Ok(r) => {
                let att = crate::state::Attachment {
                    url:      r.url.clone(),
                    kind:     r.kind.clone(),
                    filename: r.filename.clone(),
                };
                state.push_own_media(r.caption.clone(), Some(att.clone()));
                if let Some(tx) = &state.cmd_tx {
                    let _ = tx.send(crate::state::UiCommand::SendMedia {
                        caption:  r.caption,
                        url:      att.url,
                        kind:     att.kind,
                        filename: att.filename,
                    });
                }
            }
            Err(e) => {
                state.push_system(format!("⚠ Media upload failed: {}", e));
            }
        }
        ctx.request_repaint();
    }
}

fn pick_and_upload_media(state: &mut AppState, ctx: &egui::Context) {
    let session = match &state.session {
        Some(s) => s.clone(),
        None    => return,
    };

    let path = rfd::FileDialog::new()
        .add_filter("Images",    &["png", "jpg", "jpeg", "gif", "webp"])
        .add_filter("Audio",     &["mp3", "ogg", "wav"])
        .add_filter("Video",     &["mp4", "webm", "mov"])
        .add_filter("All Media", &["png", "jpg", "jpeg", "gif", "webp",
                                    "mp3", "ogg", "wav", "mp4", "webm", "mov"])
        .pick_file();

    let path = match path {
        Some(p) => p,
        None    => return,
    };

    state.media_in_progress = true;
    let caption = state.message_input.trim().to_string();
    state.message_input.clear();

    let (tx, rx) = std::sync::mpsc::channel();
    state.media_rx = Some(rx);
    let ctx_clone = ctx.clone();

    thread::spawn(move || {
        let ext      = path.extension().and_then(|e| e.to_str()).unwrap_or("bin").to_string();
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("attachment").to_string();
        let kind     = kind_for_ext(&ext);

        let result = std::fs::read(&path)
            .map_err(|e| format!("Failed to read file: {}", e))
            .and_then(|bytes| {
                crate::net::supabase::upload_media(
                    &session.user_id,
                    &session.access_token,
                    bytes,
                    &ext,
                    &filename,
                )
                .map_err(|e| e.to_string())
            })
            .map(|url| crate::state::MediaUploadResult { url, kind, filename, caption });

        let _ = tx.send(result);
        ctx_clone.request_repaint();
    });
}

fn kind_for_ext(ext: &str) -> crate::state::AttachmentKind {
    match ext.to_lowercase().as_str() {
        "mp3" | "ogg" | "wav" | "flac" | "aac" => crate::state::AttachmentKind::Audio,
        "mp4" | "webm" | "mov" | "avi" | "mkv" => crate::state::AttachmentKind::Video,
        _                                       => crate::state::AttachmentKind::Image,
    }
}

// ── Message send ──────────────────────────────────────────────────────────────

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
