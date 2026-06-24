// ─────────────────────────────────────────────────────────────────────────────
// ui/profile.rs — Profile & Settings Modal
// ─────────────────────────────────────────────────────────────────────────────

use egui::{Color32, RichText, Vec2};
use std::sync::mpsc;
use std::thread;
use std::fs;
use std::path::Path;

use crate::state::{AppState, ThemeOverride};
use crate::net::supabase;
use super::{components, theme};

pub fn render_modal(ctx: &egui::Context, state: &mut AppState) {
    if !state.show_profile_modal {
        return;
    }

    // Poll for profile picture / username update result
    if let Some(rx) = &state.profile_rx {
        if let Ok(result) = rx.try_recv() {
            state.profile_in_progress = false;
            state.profile_rx = None;

            match result {
                Ok(r) => {
                    if let Some(mut session) = state.session.take() {
                        // If the upload thread had to refresh our JWT, persist the new tokens.
                        if let Some((at, rt)) = r.new_tokens {
                            session.access_token  = at;
                            session.refresh_token = rt;
                        }
                        if let Some(url) = r.avatar_url {
                            // Bust old cached texture so image_loader re-fetches the new avatar.
                            if let Some(old_url) = &session.avatar_url {
                                super::image_loader::invalidate(old_url);
                            }
                            session.avatar_url = Some(url);
                        }
                        session.description = state.profile_description.clone();
                        session.save();
                        // Tell the webrtc task so it re-broadcasts to peers immediately.
                        if let Some(tx) = &state.cmd_tx {
                            let _ = tx.send(crate::state::UiCommand::ProfileUpdated {
                                new_username: session.username.clone(),
                                avatar_url:   session.avatar_url.clone(),
                                description:  Some(state.profile_description.clone()),
                            });
                        }
                        state.session = Some(session);
                    }
                }
                Err(e) => {
                    state.profile_error = Some(e);
                }
            }
        }
    }

    egui::Window::new("Profile Settings")
        .id(egui::Id::new("profile_modal"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .frame(egui::Frame::window(&ctx.style()).fill(theme::SIDEBAR_BG).inner_margin(24.0))
        .show(ctx, |ui| {
            ui.set_min_width(340.0);

            egui::ScrollArea::vertical()
                .max_height(540.0)
                .show(ui, |ui| {
                    ui.set_min_width(320.0);
                    ui.vertical_centered(|ui| {
                        // Avatar preview & upload
                        let current_url = state.session.as_ref().and_then(|s| s.avatar_url.clone());

                        let rect = components::draw_avatar(ui, &state.username, current_url.as_deref(), 80.0);

                        if ui.rect_contains_pointer(rect) {
                            ui.painter().circle_filled(rect.center(), 40.0, Color32::from_black_alpha(150));
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "Edit",
                                egui::FontId::proportional(14.0),
                                Color32::WHITE,
                            );
                        }

                        if ui.interact(rect, egui::Id::new("avatar_click"), egui::Sense::click()).clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("Images", &["png", "jpg", "jpeg"])
                                .pick_file() {
                                    upload_avatar(state, ctx, &path);
                                }
                        }

                        ui.add_space(8.0);
                        ui.label(RichText::new("Click avatar to upload").size(11.0).color(theme::TEXT_MUTED));

                        ui.add_space(24.0);

                        // Username field
                        ui.label(RichText::new("USERNAME").size(11.0).color(theme::TEXT_MUTED).strong());
                        ui.add_space(4.0);
                        ui.add(
                            egui::TextEdit::singleline(&mut state.username)
                                .desired_width(f32::INFINITY)
                                .margin(egui::Margin::symmetric(12i8, 8i8)),
                        );

                        ui.add_space(16.0);

                        // Description / About Me field
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.label(RichText::new("ABOUT ME").size(11.0).color(theme::TEXT_MUTED).strong());
                        });
                        ui.add_space(4.0);
                        let desc_len = state.profile_description.chars().count();
                        let desc_response = ui.add(
                            egui::TextEdit::multiline(&mut state.profile_description)
                                .desired_width(f32::INFINITY)
                                .desired_rows(3)
                                .margin(egui::Margin::symmetric(12i8, 8i8)),
                        );
                        // Enforce 250-char limit
                        if desc_len > 250 {
                            state.profile_description = state.profile_description.chars().take(250).collect();
                        }
                        // Clamp max height by checking if the widget grew too tall
                        let _ = desc_response;
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let len = state.profile_description.chars().count().min(250);
                            ui.label(
                                RichText::new(format!("{} / 250", len))
                                    .size(11.0)
                                    .color(theme::TEXT_MUTED),
                            );
                        });

                        ui.add_space(16.0);
                        ui.separator();
                        ui.add_space(8.0);

                        // ── Appearance section ────────────────────────────────
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.label(RichText::new("APPEARANCE").size(11.0).color(theme::TEXT_MUTED).strong());
                        });
                        ui.add_space(8.0);

                        color_row(ui, state, "Sidebar background",
                            |t: &ThemeOverride| t.sidebar_bg,
                            |t: &mut ThemeOverride, v| t.sidebar_bg = v,
                            theme::SIDEBAR_BG,
                        );
                        color_row(ui, state, "Logo area background",
                            |t| t.logo_bg,
                            |t, v| t.logo_bg = v,
                            theme::HEADER_BG,
                        );
                        color_row(ui, state, "Logo text",
                            |t| t.logo_text,
                            |t, v| t.logo_text = v,
                            Color32::WHITE,
                        );
                        color_row(ui, state, "Input background",
                            |t| t.input_bg,
                            |t, v| t.input_bg = v,
                            theme::INPUT_BG,
                        );
                        color_row(ui, state, "Input text / icon",
                            |t| t.input_text,
                            |t, v| t.input_text = v,
                            theme::TEXT_MUTED,
                        );
                        color_row(ui, state, "Channel header text",
                            |t| t.channel_header_text,
                            |t, v| t.channel_header_text = v,
                            Color32::WHITE,
                        );
                        color_row(ui, state, "Chat area background",
                            |t| t.chat_bg,
                            |t, v| t.chat_bg = v,
                            theme::DARK_BG,
                        );

                        ui.add_space(16.0);

                        if let Some(err) = &state.profile_error {
                            ui.label(RichText::new(err).color(theme::RED_DANGER).size(13.0));
                            ui.add_space(8.0);
                        }

                        if state.profile_in_progress {
                            ui.spinner();
                        } else {
                            ui.horizontal(|ui| {
                                if components::ghost_button(ui, "Cancel").clicked() {
                                    state.show_profile_modal = false;
                                    // Revert username and description
                                    if let Some(s) = &state.session {
                                        state.username = s.username.clone();
                                        state.profile_description = s.description.clone();
                                    }
                                }

                                let is_valid = !state.username.trim().is_empty();
                                ui.add_enabled_ui(is_valid, |ui| {
                                    if components::accent_button(ui, "Save Changes").clicked() {
                                        save_profile(state, ctx);
                                    }
                                });
                            });
                        }

                        ui.add_space(16.0);

                        // ── App management row ───────────────────────────────────────
                        ui.horizontal(|ui| {
                            let check_label = if state.update_check_in_progress {
                                "Checking..."
                            } else if state.update_available_version.is_some() {
                                "Update available"
                            } else {
                                "Check for updates"
                            };
                            let update_btn = ui.add_enabled(
                                !state.update_check_in_progress,
                                egui::Button::new(
                                    RichText::new(check_label)
                                        .size(13.0)
                                        .color(if state.update_available_version.is_some() {
                                            Color32::from_rgb(240, 180, 60)
                                        } else {
                                            theme::TEXT_MUTED
                                        }),
                                )
                                .frame(false),
                            );
                            if update_btn.clicked() {
                                if state.update_available_version.is_some() {
                                    state.show_update_modal  = true;
                                    state.show_profile_modal = false;
                                } else {
                                    state.update_check_in_progress = true;
                                    state.last_update_check        = std::time::Instant::now();
                                    crate::net::updater::check_for_updates(state.updater_tx.clone());
                                }
                            }
                            if state.update_check_in_progress {
                                ui.add(egui::widgets::Spinner::new().size(12.0).color(theme::TEXT_MUTED));
                            }

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(
                                    RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                                        .size(11.0)
                                        .color(theme::TEXT_MUTED),
                                );
                            });
                        });

                        ui.add_space(12.0);
                        if ui.button(RichText::new("Sign Out").color(theme::RED_DANGER)).clicked() {
                            crate::state::Session::clear();
                            state.session = None;
                            state.show_profile_modal = false;
                            state.screen = crate::state::Screen::Login;
                        }
                    });
                });
        });
}

/// Render a color picker row with a label, color button, and "Reset" link.
fn color_row(
    ui: &mut egui::Ui,
    state: &mut AppState,
    label: &str,
    get: impl Fn(&ThemeOverride) -> Option<[u8; 4]>,
    set: impl Fn(&mut ThemeOverride, Option<[u8; 4]>),
    default_color: Color32,
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(12.0).color(theme::TEXT_MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Reset button
            if ui.add(
                egui::Button::new(RichText::new("Reset").size(11.0).color(theme::TEXT_MUTED))
                    .fill(Color32::TRANSPARENT)
                    .stroke(egui::Stroke::NONE),
            ).clicked() {
                set(&mut state.theme_override, None);
                state.theme_override.save();
            }

            ui.add_space(4.0);

            // Color picker button
            let current = get(&state.theme_override);
            let mut color32 = current.map(ThemeOverride::c32).unwrap_or(default_color);
            let old = color32;
            egui::color_picker::color_edit_button_srgba(
                ui,
                &mut color32,
                egui::color_picker::Alpha::Opaque,
            );
            if color32 != old {
                let arr = [color32.r(), color32.g(), color32.b(), color32.a()];
                set(&mut state.theme_override, Some(arr));
                state.theme_override.save();
            }
        });
    });
    ui.add_space(4.0);
}

fn upload_avatar(state: &mut AppState, ctx: &egui::Context, path: &Path) {
    let session = match &state.session {
        Some(s) => s.clone(),
        None => return,
    };

    let path_buf = path.to_path_buf();
    let ext = path_buf.extension().and_then(|s| s.to_str()).unwrap_or("png").to_string();

    state.profile_in_progress = true;
    state.profile_error = None;

    let (tx, rx) = mpsc::channel();
    state.profile_rx = Some(rx);
    let ctx_clone = ctx.clone();
    let description = state.profile_description.clone();

    thread::spawn(move || {
        let result: Result<crate::state::ProfileUploadResult, String> = match fs::read(&path_buf) {
            Ok(bytes) => {
                match supabase::upload_avatar_auto_refresh(
                    &session.user_id,
                    &session.access_token,
                    &session.refresh_token,
                    bytes,
                    &ext,
                ) {
                    Ok((url, new_tokens)) => {
                        // Reuse whichever token is freshest for the profile update
                        let token = new_tokens.as_ref()
                            .map(|(at, _)| at.as_str())
                            .unwrap_or(&session.access_token);
                        if let Err(e) = supabase::update_profile(
                            &session.user_id, token, &session.username, Some(&url), &description,
                        ) {
                            Err(e.to_string())
                        } else {
                            Ok(crate::state::ProfileUploadResult { avatar_url: Some(url), new_tokens })
                        }
                    }
                    Err(e) => Err(e.to_string()),
                }
            }
            Err(e) => Err(format!("Could not read file: {}", e)),
        };

        let _ = tx.send(result);
        ctx_clone.request_repaint();
    });
}

fn save_profile(state: &mut AppState, ctx: &egui::Context) {
    let session = match &state.session {
        Some(s) => s.clone(),
        None => return,
    };

    state.profile_in_progress = true;
    state.profile_error = None;

    let new_username  = state.username.clone();
    let description   = state.profile_description.clone();
    let desc_for_save = description.clone();

    let (tx, rx) = mpsc::channel();
    state.profile_rx = Some(rx);
    let ctx_clone = ctx.clone();

    let new_username_clone = new_username.clone();
    let session_clone = session.clone();

    thread::spawn(move || {
        let result = supabase::update_profile(
            &session_clone.user_id,
            &session_clone.access_token,
            &new_username_clone,
            session_clone.avatar_url.as_deref(),
            &description,
        )
        .map(|_| crate::state::ProfileUploadResult { avatar_url: None, new_tokens: None })
        .map_err(|e| e.to_string());

        let _ = tx.send(result);
        ctx_clone.request_repaint();
    });

    // Optimistically close modal, save to local session
    if let Some(mut s) = state.session.take() {
        s.username    = new_username;
        s.description = desc_for_save;
        s.save();
        state.session = Some(s);
    }
    state.show_profile_modal = false;
}
