// ─────────────────────────────────────────────────────────────────────────────
// ui/profile.rs — Profile & Settings Modal
// ─────────────────────────────────────────────────────────────────────────────

use egui::{Color32, CornerRadius, RichText, Vec2};
use std::sync::mpsc;
use std::thread;
use std::fs;
use std::path::Path;

use crate::state::AppState;
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
                        session.save();
                        // Tell the webrtc task so it re-broadcasts to peers immediately.
                        if let Some(tx) = &state.cmd_tx {
                            let _ = tx.send(crate::state::UiCommand::ProfileUpdated {
                                new_username: session.username.clone(),
                                avatar_url:   session.avatar_url.clone(),
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
            ui.set_min_width(320.0);
            
            ui.vertical_centered(|ui| {
                // Avatar preview & upload
                let current_url = state.session.as_ref().and_then(|s| s.avatar_url.clone());
                
                // We'll use our draw_avatar (which will soon support images)
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
                            // Revert username
                            if let Some(s) = &state.session {
                                state.username = s.username.clone();
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
                
                ui.add_space(24.0);
                if ui.button(RichText::new("Sign Out").color(theme::RED_DANGER)).clicked() {
                    crate::state::Session::clear();
                    state.session = None;
                    state.show_profile_modal = false;
                    state.screen = crate::state::Screen::Login;
                }
            });
        });
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
                            &session.user_id, token, &session.username, Some(&url),
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
    
    let new_username = state.username.clone();
    
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
        )
        .map(|_| crate::state::ProfileUploadResult { avatar_url: None, new_tokens: None })
        .map_err(|e| e.to_string());

        let _ = tx.send(result);
        ctx_clone.request_repaint();
    });
    
    // Optimistically close modal, save to local session
    if let Some(mut s) = state.session.take() {
        s.username = new_username;
        s.save();
        state.session = Some(s);
    }
    state.show_profile_modal = false;
}
