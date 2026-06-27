// ─────────────────────────────────────────────────────────────────────────────
// ui/chat.rs — Main chat interface  (egui 0.34 compatible)
// ─────────────────────────────────────────────────────────────────────────────

use egui::{Color32, CornerRadius, Frame, Key, Margin, RichText, ScrollArea, Vec2};
use std::thread;

use crate::state::{AppState, MessageKind, ThemeOverride};
use super::{components, theme, updater as update_ui};

#[allow(deprecated)] // egui 0.34: Panel::show still works, show_inside() is new preferred API
pub fn render(ctx: &egui::Context, state: &mut AppState) {
    poll_media_upload(ctx, state);
    // Clear typing entries for peers who've left.
    state.typing_users.retain(|u| state.peers.iter().any(|p| &p.username == u));
    // ── 1. Left sidebar panel ─────────────────────────────────────────────────
    let sidebar_fill = state.theme_override.sidebar_bg.map(ThemeOverride::c32).unwrap_or(theme::SIDEBAR_BG);
    egui::SidePanel::left("sidebar")
        .exact_size(theme::SIDEBAR_WIDTH)
        .resizable(false)
        .frame(Frame::default().fill(sidebar_fill).inner_margin(Margin::same(0i8)))
        .show(ctx, |ui| {
            render_sidebar(ui, state);
        });

    // ── Update modal (rendered above everything else) ─────────────────────────
    update_ui::render_update_modal(ctx, state);

    let chat_fill = state.theme_override.chat_bg.map(ThemeOverride::c32).unwrap_or(theme::DARK_BG);

    // ── 2. Channel header (top) ───────────────────────────────────────────────
    egui::TopBottomPanel::top("channel_header")
        .exact_size(theme::CHANNEL_HEADER_HEIGHT)
        .frame(
            Frame::default()
                .fill(chat_fill)
                .inner_margin(Margin { left: 16, right: 16, top: 14, bottom: 14 })
                .stroke(egui::Stroke::new(1.0, theme::SEPARATOR)),
        )
        .show(ctx, |ui| {
            render_channel_header(ui, state);
        });

    // ── Typing indicator (above input bar) ────────────────────────────────────
    if !state.typing_users.is_empty() {
        egui::TopBottomPanel::bottom("typing_bar")
            .resizable(false)
            .exact_size(22.0)
            .frame(Frame::default().fill(chat_fill).inner_margin(Margin { left: 16, right: 16, top: 2, bottom: 0 }))
            .show(ctx, |ui| {
                let text = match state.typing_users.len() {
                    1 => format!("{} is typing…", state.typing_users[0]),
                    2 => format!("{} and {} are typing…", state.typing_users[0], state.typing_users[1]),
                    _ => "Several people are typing…".to_string(),
                };
                ui.label(RichText::new(text).size(12.0).color(theme::TEXT_MUTED));
            });
    }

    // ── 3. Input bar (BOTTOM) — anchored to the window bottom edge ────────────
    egui::TopBottomPanel::bottom("input_bar")
        .resizable(false)
        .frame(Frame::default().fill(chat_fill).inner_margin(Margin::same(0i8)))
        .show(ctx, |ui| {
            egui::Frame::NONE
                .inner_margin(Margin {
                    left:   16,
                    right:  16,
                    top:    8,
                    bottom: theme::SAFE_MARGIN as i8,
                })
                .show(ui, |ui| {
                    render_input_bar(ctx, ui, state);
                });
        });

    // ── 4. Central panel — messages scroll area only ──────────────────────────
    // CentralPanel fills whatever space remains between the top and bottom panels.
    // No manual height math needed — the scroll area fills the full height.
    egui::CentralPanel::default()
        .frame(Frame::default().fill(chat_fill).inner_margin(Margin::same(0i8)))
        .show(ctx, |ui| {
            render_message_area(ctx, ui, state);
        });

    crate::ui::profile::render_modal(ctx, state);
    components::render_inspect_panel(ctx, state);
}

// ── Sidebar ───────────────────────────────────────────────────────────────────

fn render_sidebar(ui: &mut egui::Ui, state: &mut AppState) {
    let sidebar_bg  = state.theme_override.sidebar_bg.map(ThemeOverride::c32).unwrap_or(theme::SIDEBAR_BG);
    let logo_bg     = state.theme_override.logo_bg.map(ThemeOverride::c32).unwrap_or(theme::HEADER_BG);
    let logo_text   = state.theme_override.logo_text.map(ThemeOverride::c32).unwrap_or(Color32::WHITE);

    // ── Server / app header ──────────────────────────────────────────────────
    Frame::default()
        .fill(logo_bg)
        .inner_margin(Margin::symmetric(16i8, 14i8))
        .stroke(egui::Stroke::new(1.0, theme::SEPARATOR))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                let (lr, _) = ui.allocate_exact_size(Vec2::splat(22.0), egui::Sense::hover());
                if ui.is_rect_visible(lr) {
                    let rect = egui::Rect::from_center_size(lr.center(), Vec2::splat(22.0));
                    // Badge uses logo_bg if set, otherwise falls back to BLURPLE accent
                    let badge_bg = state.theme_override.logo_bg.map(ThemeOverride::c32).unwrap_or(theme::BLURPLE);
                    ui.painter().rect_filled(rect, egui::CornerRadius::same(6u8), badge_bg);
                    let tl  = rect.min + Vec2::new(4.5, 5.0);
                    let tr  = rect.min + Vec2::new(17.5, 5.0);
                    let bot = egui::pos2(rect.center().x, rect.max.y - 5.0);
                    ui.painter().line_segment([tl, bot], egui::Stroke::new(2.5, logo_text));
                    ui.painter().line_segment([tr, bot], egui::Stroke::new(2.5, logo_text));
                }
                ui.add_space(6.0);
                ui.label(RichText::new("VoxLink").size(15.0).color(logo_text).strong());
            });
        });

    let _ = sidebar_bg; // used by the SidePanel fill in render()

    // ── Scrollable channel + member list ─────────────────────────────────────
    let scroll_max = (ui.available_height() - theme::SIDEBAR_BOTTOM_H).max(40.0);
    ScrollArea::vertical()
        .id_salt("sidebar_scroll")
        .auto_shrink([false, false])
        .max_height(scroll_max)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.add_space(8.0);

            // ── Text channels ───────────────────────────────────────────────
            sidebar_section_header(ui, "TEXT CHANNELS");
            sidebar_channel_item(ui, "general", true);

            ui.add_space(12.0);

            // ── Voice channels ──────────────────────────────────────────────
            sidebar_section_header(ui, "VOICE CHANNELS");
            render_voice_channel(ui, state);

            ui.add_space(12.0);

            // ── Online members ──────────────────────────────────────────────
            let online = state.peers.len() + 1;
            sidebar_section_header(ui, &format!("ONLINE  {}", online));
            ui.add_space(2.0);

            let avatar_url = state.session.as_ref().and_then(|s| s.avatar_url.as_deref());
            components::sidebar_user_row(
                ui, &state.username, avatar_url, true,
                state.voice_active, state.is_speaking, state.is_muted,
            );
            let peers = state.peers.clone();
            for peer in &peers {
                ui.add_space(2.0);
                let row_resp = ui.scope(|ui| {
                    components::sidebar_user_row(
                        ui, &peer.username, peer.avatar_url.as_deref(), false,
                        peer.in_voice, peer.is_speaking, peer.is_muted,
                    );
                }).response;
                let click_resp = ui.interact(
                    row_resp.rect,
                    egui::Id::new(("inspect", peer.username.as_str())),
                    egui::Sense::click(),
                );
                if click_resp.clicked() {
                    state.inspected_peer = Some(peer.username.clone());
                }
                if click_resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
            }
            ui.add_space(12.0);

            // ── Update badge (above profile bar) ─────────────────────────────
            if state.update_available_version.is_some() || state.update_check_in_progress {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    egui::Frame::NONE
                        .outer_margin(Margin { left: 0, right: 8, top: 0, bottom: 0 })
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            if update_ui::render_sidebar_badge(ui, state) {
                                state.show_update_modal = true;
                            }
                        });
                });
                ui.add_space(4.0);
            }
        });

    // Push profile bar to the very bottom
    let remaining = ui.available_height() - theme::SIDEBAR_BOTTOM_H;
    if remaining > 0.0 { ui.add_space(remaining); }

    // ── Bottom profile bar ────────────────────────────────────────────────────
    let profile_bar_bg = state.theme_override.logo_bg.map(ThemeOverride::c32).unwrap_or(theme::HEADER_BG);
    Frame::default()
        .fill(profile_bar_bg)
        .inner_margin(Margin::symmetric(14i8, 12i8))
        .outer_margin(Margin { left: 0, right: 0, top: 0, bottom: theme::SAFE_MARGIN as i8 })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                let avatar_url = state.session.as_ref().and_then(|s| s.avatar_url.as_deref());
                let resp = ui.horizontal(|ui| {
                    components::draw_avatar(ui, &state.username, avatar_url, 32.0);
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&state.username).size(14.0).color(Color32::WHITE).strong());
                        ui.horizontal(|ui| {
                            let color = if state.voice_active { theme::GREEN_ONLINE } else { theme::TEXT_PRIMARY };
                            let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                            ui.painter().circle_filled(rect.center(), 4.0, color);
                            ui.add_space(2.0);
                            ui.label(
                                RichText::new(if state.voice_active { "In voice" } else { "Online" })
                                    .size(11.0).color(theme::TEXT_MUTED),
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
            });
        });
}

// ── Voice channel block ───────────────────────────────────────────────────────

fn render_voice_channel(ui: &mut egui::Ui, state: &mut AppState) {
    // Channel row (join / leave)
    let bg = if state.voice_active { theme::ACTIVE_BG } else { Color32::TRANSPARENT };
    Frame::default()
        .fill(bg)
        .corner_radius(CornerRadius::same(6u8))
        .inner_margin(Margin { left: 8, right: 8, top: 4, bottom: 4 })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width() - 16.0);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                // Speaker / connected indicator
                let (icon_color, name_color) = if state.voice_active {
                    (theme::GREEN_ONLINE, Color32::WHITE)
                } else {
                    (theme::TEXT_MUTED, theme::TEXT_MUTED)
                };
                // U+25BA BLACK RIGHT-POINTING POINTER — clean BMP speaker icon
                ui.label(RichText::new("\u{25BA}").size(11.0).color(icon_color));
                ui.add_space(4.0);
                ui.label(RichText::new("General").size(14.0).color(name_color));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (label, color) = if state.voice_active {
                        ("Leave", theme::RED_DANGER)
                    } else {
                        ("Join", theme::GREEN_ONLINE)
                    };
                    let btn = ui.add(
                        egui::Button::new(RichText::new(label).size(11.0).color(color))
                            .fill(Color32::TRANSPARENT)
                            .stroke(egui::Stroke::new(1.0, color))
                            .corner_radius(CornerRadius::same(4u8)),
                    );
                    if btn.hovered() { ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand); }
                    if btn.clicked() {
                        state.voice_active = !state.voice_active;
                        if !state.voice_active {
                            state.is_speaking = false;
                        }
                        if let Some(tx) = &state.cmd_tx {
                            let _ = tx.send(crate::state::UiCommand::ToggleVoice(state.voice_active));
                        }
                    }
                });
            });
        });

    // Participant list (only when voice is active)
    if state.voice_active {
        // Voice status bar above participants
        Frame::default()
            .fill(Color32::from_rgba_unmultiplied(35, 165, 90, 18))
            .corner_radius(CornerRadius::same(4u8))
            .inner_margin(Margin::symmetric(12i8, 4i8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let t = ui.ctx().animate_bool(egui::Id::new("vc_live_dot"), true);
                    let dot_color = Color32::from_rgb(
                        (theme::GREEN_ONLINE.r() as f32 * t) as u8,
                        (theme::GREEN_ONLINE.g() as f32 * t) as u8,
                        (theme::GREEN_ONLINE.b() as f32 * t) as u8,
                    );
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                    ui.painter().circle_filled(r.center(), 4.0, dot_color);
                    ui.add_space(4.0);
                    ui.label(RichText::new("Voice Connected  RTC / P2P").size(11.0).color(theme::GREEN_ONLINE));
                });
            });

        ui.add_space(2.0);

        // Self participant row
        let avatar_url = state.session.as_ref().and_then(|s| s.avatar_url.as_deref()).map(str::to_owned);
        let mute_toggled = render_voice_participant(
            ui, &state.username, avatar_url.as_deref(), true, state.is_speaking, state.is_muted,
        );
        if let Some(new_muted) = mute_toggled {
            state.is_muted = new_muted;
            if let Some(tx) = &state.cmd_tx {
                let _ = tx.send(crate::state::UiCommand::SetMuted(new_muted));
            }
        }

        // Peer participant rows
        let peers = state.peers.clone();
        for peer in peers.iter().filter(|p| p.in_voice) {
            ui.add_space(1.0);
            render_voice_participant(
                ui, &peer.username, peer.avatar_url.as_deref(), false,
                peer.is_speaking, peer.is_muted,
            );
        }
    }
}

/// Renders a single voice participant row.
/// Returns `Some(new_muted)` when the mute button was clicked (only for self rows).
fn render_voice_participant(
    ui: &mut egui::Ui,
    username: &str,
    avatar_url: Option<&str>,
    is_self: bool,
    is_speaking: bool,
    is_muted: bool,
) -> Option<bool> {
    let mut mute_toggled = None;

    Frame::default()
        .fill(if is_speaking && !is_muted {
            Color32::from_rgba_unmultiplied(35, 165, 90, 15)
        } else {
            Color32::TRANSPARENT
        })
        .corner_radius(CornerRadius::same(4u8))
        .inner_margin(Margin { left: 24, right: 8, top: 3, bottom: 3 })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let avatar_rect = components::draw_avatar(ui, username, avatar_url, 24.0);

                // Speaking ring — animated fade in/out
                let t = ui.ctx().animate_bool(
                    egui::Id::new(("vc_ring", username)),
                    is_speaking && !is_muted,
                );
                if t > 0.0 {
                    ui.painter().circle_stroke(
                        avatar_rect.center(),
                        avatar_rect.width() / 2.0 + 2.5,
                        egui::Stroke::new(2.5 * t, theme::GREEN_ONLINE),
                    );
                }

                ui.add_space(6.0);

                let display = if is_self {
                    format!("{} (You)", username)
                } else {
                    username.to_string()
                };
                ui.add(
                    egui::Label::new(
                        RichText::new(display)
                            .size(13.0)
                            .color(if is_speaking && !is_muted { theme::GREEN_ONLINE } else { Color32::WHITE }),
                    )
                    .wrap_mode(egui::TextWrapMode::Truncate),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if is_self {
                        // Mute toggle button
                        let (label, color) = if is_muted {
                            ("[M]", theme::RED_DANGER)
                        } else {
                            ("[|]", theme::TEXT_MUTED)
                        };
                        let btn = ui.add(
                            egui::Button::new(RichText::new(label).size(11.0).color(color))
                                .fill(Color32::TRANSPARENT)
                                .stroke(egui::Stroke::NONE),
                        ).on_hover_text(if is_muted { "Unmute" } else { "Mute" });
                        if btn.hovered() { ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand); }
                        if btn.clicked() {
                            mute_toggled = Some(!is_muted);
                        }
                    } else if is_muted {
                        // Peer mute indicator
                        ui.label(RichText::new("[M]").size(11.0).color(theme::RED_DANGER))
                            .on_hover_text(format!("{} is muted", username));
                    }
                });
            });
        });

    mute_toggled
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
    let header_text = state.theme_override.channel_header_text.map(ThemeOverride::c32).unwrap_or(Color32::WHITE);
    ui.horizontal(|ui| {
        ui.label(RichText::new("#").size(18.0).color(header_text).strong());
        ui.add_space(4.0);
        ui.label(RichText::new("general").size(15.0).color(header_text).strong());
        ui.label(RichText::new("|").size(16.0).color(theme::SEPARATOR));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(theme::SAFE_MARGIN);
            ui.label(
                RichText::new(format!("{} online", state.peers.len() + 1))
                    .size(13.0)
                    .color(theme::TEXT_MUTED),
            );
            ui.add_space(8.0);
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
}

// ── Message Area ─────────────────────────────────────────────────────────────

// render_message_area: fills the CentralPanel with a full-height scroll area.
// The input bar is now a TopBottomPanel::bottom, so no manual height math needed.
fn render_message_area(_ctx: &egui::Context, ui: &mut egui::Ui, state: &mut AppState) {
    // Build a username → avatar URL lookup from live session + peer list so that
    // every chat message can show the correct profile picture next to its author.
    let mut avatar_map: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    if let Some(ref s) = state.session {
        if let Some(ref url) = s.avatar_url {
            avatar_map.insert(state.username.clone(), url.clone());
        }
    }
    for peer in &state.peers {
        if let Some(ref url) = peer.avatar_url {
            avatar_map.insert(peer.username.clone(), url.clone());
        }
    }

    ScrollArea::vertical()
        .id_salt("messages_scroll")
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add_space(8.0);

            let messages = state.messages.clone();
            let mut prev_author: Option<&str> = None;
            let mut prev_kind:   Option<&MessageKind> = None;

            for msg in &messages {
                let is_system = msg.kind == MessageKind::System;
                let same_author = prev_author == Some(msg.author.as_str())
                    && prev_kind == Some(&msg.kind)
                    && !is_system;

                let avatar_url = avatar_map.get(msg.author.as_str()).map(String::as_str);
                let reaction_toggle = components::render_message(ui, msg, !same_author, avatar_url, &state.username);

                if let Some((message_id, emoji, active)) = reaction_toggle {
                    // Optimistic local update
                    if let Some(m) = state.messages.iter_mut().find(|m| m.db_id.as_deref() == Some(&message_id)) {
                        if active {
                            if !m.reactions.iter().any(|r| r.user == state.username && r.emoji == emoji) {
                                m.reactions.push(crate::state::Reaction { user: state.username.clone(), emoji: emoji.clone() });
                            }
                        } else {
                            m.reactions.retain(|r| !(r.user == state.username && r.emoji == emoji));
                        }
                    }
                    // Broadcast
                    if let Some(tx) = &state.cmd_tx {
                        let _ = tx.send(crate::state::UiCommand::SendReaction {
                            message_id: message_id.clone(),
                            emoji: emoji.clone(),
                            active,
                        });
                    }
                    // DB persist (fire-and-forget)
                    if let Some(ref s) = state.session {
                        let at = s.access_token.clone();
                        let user = state.username.clone();
                        thread::spawn(move || {
                            if active {
                                let _ = crate::net::supabase::insert_reaction(&at, &message_id, &user, &emoji);
                            } else {
                                let _ = crate::net::supabase::delete_reaction(&at, &message_id, &user, &emoji);
                            }
                        });
                    }
                }

                prev_author = Some(msg.author.as_str());
                prev_kind   = Some(&msg.kind);
            }

            if state.scroll_to_bottom {
                ui.scroll_to_cursor(Some(egui::Align::Max));
                state.scroll_to_bottom = false;
            }

            ui.add_space(8.0);
        });
}

fn render_input_bar(ctx: &egui::Context, ui: &mut egui::Ui, state: &mut AppState) {
    let input_bg   = state.theme_override.input_bg.map(ThemeOverride::c32).unwrap_or(theme::INPUT_BG);
    let input_text = state.theme_override.input_text.map(ThemeOverride::c32);

    ui.horizontal(|ui| {
        Frame::default()
            .fill(input_bg)
            .corner_radius(CornerRadius::same(8u8))
            .inner_margin(Margin { left: 10, right: 14, top: 10, bottom: 10 })
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    // ── Attachment button ────────────────────────────────────
                    if state.media_in_progress {
                        ui.spinner();
                    } else {
                        let icon_color = input_text.unwrap_or(theme::TEXT_MUTED);
                        // "+" is universally renderable; tooltip clarifies it's for attachments
                        let attach = ui.add(
                            egui::Button::new(
                                RichText::new("+").size(20.0).color(icon_color).strong(),
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

                    // Apply theme overrides for text color and bg
                    let old_text_color = ui.visuals().override_text_color;
                    ui.visuals_mut().override_text_color = input_text;
                    ui.visuals_mut().extreme_bg_color    = input_bg;

                    let response = ui.add(
                        egui::TextEdit::singleline(&mut state.message_input)
                            .id(input_id)
                            .hint_text("Message #general…")
                            .desired_width(avail_w)
                            .font(egui::FontId::proportional(15.0))
                            .frame(egui::Frame::NONE),
                    );

                    // Restore defaults
                    ui.visuals_mut().override_text_color = old_text_color;

                    if response.lost_focus() && ctx.input(|i| i.key_pressed(Key::Enter)) {
                        try_send_message(state);
                        ctx.memory_mut(|m| m.request_focus(input_id));
                    }
                    // Typing indicator: send a ping on keystroke if throttled (1 / 3s).
                    if response.changed() && !state.message_input.trim().is_empty() {
                        let now = std::time::Instant::now();
                        if now.duration_since(state.last_typing_ping) > std::time::Duration::from_secs(3) {
                            state.last_typing_ping = now;
                            if let Some(tx) = &state.cmd_tx {
                                let _ = tx.send(crate::state::UiCommand::SendTyping(true));
                            }
                        }
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
                // Persist refreshed tokens back into the session so future uploads don't retry.
                if let Some(ref mut s) = state.session {
                    if let Some((at, rt)) = r.new_tokens {
                        s.access_token  = at;
                        s.refresh_token = rt;
                        s.save();
                    }
                }
                let att = crate::state::Attachment {
                    url:      r.url.clone(),
                    kind:     r.kind.clone(),
                    filename: r.filename.clone(),
                };
                state.push_own_media(r.caption.clone(), Some(att.clone()));
                if let Some(tx) = &state.cmd_tx {
                    let _ = tx.send(crate::state::UiCommand::SendMedia {
                        caption:  r.caption.clone(),
                        url:      att.url.clone(),
                        kind:     att.kind.clone(),
                        filename: att.filename.clone(),
                    });
                }

                // Persist media message to DB (fire-and-forget)
                if let Some(ref s) = state.session {
                    let at      = s.access_token.clone();
                    let from    = state.username.clone();
                    let caption = r.caption.clone();
                    let att_db  = att.clone();
                    thread::spawn(move || {
                        if let Err(e) = crate::net::supabase::insert_message(
                            &at, &from, &caption, Some(&att_db),
                        ) {
                            log::warn!("[chat] DB insert (media) failed: {}", e);
                        }
                    });
                }
            }
            Err(e) => {
                state.push_system(format!("Upload failed: {}", e));
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
        let kind     = crate::net::contract::ext_to_kind(&ext);

        let result = std::fs::read(&path)
            .map_err(|e| format!("Could not read file: {}", e))
            .and_then(|bytes| {
                crate::net::supabase::upload_media_auto_refresh(
                    &session.user_id,
                    &session.access_token,
                    &session.refresh_token,
                    bytes,
                    &ext,
                    &filename,
                )
                .map_err(|e| e.to_string())
            })
            .map(|(url, new_tokens)| crate::state::MediaUploadResult {
                url, kind, filename, caption, new_tokens,
            });

        let _ = tx.send(result);
        ctx_clone.request_repaint();
    });
}


// ── Message send ──────────────────────────────────────────────────────────────

fn try_send_message(state: &mut AppState) {
    let content = state.message_input.trim().to_string();
    if content.is_empty() { return; }
    state.message_input.clear();
    // Clear typing indicator for all peers.
    if let Some(tx) = &state.cmd_tx {
        let _ = tx.send(crate::state::UiCommand::SendTyping(false));
    }

    if let Some(tx) = &state.cmd_tx {
        let _ = tx.send(crate::state::UiCommand::SendMessage(content.clone()));
    }

    // Show locally (optimistic — sender doesn't receive own broadcast)
    state.push_own(content.clone());

    // Persist to DB (fire-and-forget — failures are logged, never block the UI)
    if let Some(ref s) = state.session {
        let at   = s.access_token.clone();
        let from = state.username.clone();
        thread::spawn(move || {
            if let Err(e) = crate::net::supabase::insert_message(&at, &from, &content, None) {
                log::warn!("[chat] DB insert failed: {}", e);
            }
        });
    }
}
