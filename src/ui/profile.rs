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

    // Poll for profile picture upload result
    if let Some(rx) = &state.profile_rx {
        if let Ok(result) = rx.try_recv() {
            state.profile_in_progress = false;
            state.profile_rx = None;
            
            match result {
                Ok(Some(url)) => {
                    if let Some(mut session) = state.session.take() {
                        session.avatar_url = Some(url);
                        session.save();
                        state.session = Some(session);
                    }
                }
                Ok(None) => {} // Just a username update
                Err(e) => {
                    state.profile_error = Some(e);
                }
            }
        }
    }

    // Darkened overlay
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("overlay")));
    painter.rect_filled(ctx.screen_rect(), 0.0, Color32::from_black_alpha(90));

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
                        "✏️ Edit",
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
        let result = match fs::read(&path_buf) {
            Ok(bytes) => {
                match supabase::upload_avatar(&session.user_id, &session.access_token, bytes, &ext) {
                    Ok(url) => {
                        // Update profile with new avatar URL
                        if let Err(e) = supabase::update_profile(&session.user_id, &session.access_token, &session.username, Some(&url)) {
                            Err(e.to_string())
                        } else {
                            Ok(Some(url))
                        }
                    }
                    Err(e) => Err(e.to_string())
                }
            }
            Err(e) => Err(format!("Failed to read file: {}", e)),
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
        match supabase::update_profile(&session_clone.user_id, &session_clone.access_token, &new_username_clone, session_clone.avatar_url.as_deref()) {
            Ok(_) => {
                let _ = tx.send(Ok(None)); // None means just username updated
            }
            Err(e) => {
                let _ = tx.send(Err(e.to_string()));
            }
        }
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
