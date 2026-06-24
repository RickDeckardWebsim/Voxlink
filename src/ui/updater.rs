// ─────────────────────────────────────────────────────────────────────────────
// ui/updater.rs — In-app update modal
// ─────────────────────────────────────────────────────────────────────────────

use egui::{
    Align, Color32, Frame, Layout, Margin, ProgressBar, RichText, ScrollArea, Vec2,
};

use crate::net::updater::format_bytes;
use crate::state::AppState;
use super::components;
use super::theme;

// ── Sidebar update badge ──────────────────────────────────────────────────────

/// Compact update notification row shown at the bottom of the sidebar channel
/// list when a new version is available. Returns true if the user clicked it.
pub fn render_sidebar_badge(ui: &mut egui::Ui, state: &AppState) -> bool {
    if state.update_available_version.is_none() && !state.update_check_in_progress {
        return false;
    }

    let mut clicked = false;

    Frame::default()
        .fill(theme::ELEVATED_BG)
        .corner_radius(egui::CornerRadius::same(8u8))
        .inner_margin(Margin::symmetric(10i8, 8i8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                if state.update_check_in_progress && state.update_available_version.is_none() {
                    // Subtle "checking" spinner — very small
                    ui.add(egui::widgets::Spinner::new().size(12.0).color(theme::TEXT_MUTED));
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("Checking for updates...")
                            .size(11.0)
                            .color(theme::TEXT_MUTED),
                    );
                } else if let Some(ref ver) = state.update_available_version {
                    // Pulsing orange dot
                    let t = ui.ctx().input(|i| i.time);
                    let pulse = (t * 2.0).sin() * 0.3 + 0.7; // 0.4 – 1.0
                    let dot_color = Color32::from_rgba_unmultiplied(
                        240,
                        140,
                        20,
                        (pulse * 255.0) as u8,
                    );
                    let (dot_rect, _) =
                        ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                    ui.painter()
                        .circle_filled(dot_rect.center(), 4.0, dot_color);
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("Update available")
                                .size(12.0)
                                .color(Color32::from_rgb(240, 180, 60))
                                .strong(),
                        );
                        ui.label(
                            RichText::new(format!("v{}", ver))
                                .size(11.0)
                                .color(theme::TEXT_MUTED),
                        );
                    });

                    // Make the whole row clickable
                    ui.ctx().request_repaint(); // keep pulsing
                }
            });

            if state.update_available_version.is_some() {
                let response = ui.interact(
                    ui.min_rect(),
                    egui::Id::new("update_badge_click"),
                    egui::Sense::click(),
                );
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if response.clicked() {
                    clicked = true;
                }
            }
        });

    clicked
}

// ── Update modal window ───────────────────────────────────────────────────────

pub fn render_update_modal(ctx: &egui::Context, state: &mut AppState) {
    if !state.show_update_modal { return; }

    egui::Window::new("VoxLink Update")
        .id(egui::Id::new("update_modal"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .frame(
            Frame::window(&ctx.style())
                .fill(theme::SIDEBAR_BG)
                .inner_margin(24.0)
                .stroke(egui::Stroke::NONE),
        )
        .show(ctx, |ui| {
            ui.set_min_width(480.0);
            ui.set_max_width(520.0);

            // ── Header ────────────────────────────────────────────────────────
            ui.horizontal(|ui| {
                // ⬆ icon via blurple circle
                let (r, _) = ui.allocate_exact_size(Vec2::splat(28.0), egui::Sense::hover());
                ui.painter().circle_filled(r.center(), 14.0, theme::BLURPLE);
                ui.painter().text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    "\u{2B06}", // ⬆
                    egui::FontId::proportional(14.0),
                    Color32::WHITE,
                );

                ui.add_space(10.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("VoxLink Update").size(17.0).color(Color32::WHITE).strong(),
                    );
                    let sub = if state.update_in_progress {
                        state.update_phase.clone()
                    } else if state.update_error.is_some() {
                        "Update failed".to_string()
                    } else if state.update_available_version.is_some() {
                        "A new version is ready to install".to_string()
                    } else {
                        "You're up to date".to_string()
                    };
                    ui.label(RichText::new(sub).size(12.0).color(theme::TEXT_MUTED));
                });

                // Close button — disabled while download is running
                if !state.update_in_progress {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("✕").size(13.0).color(theme::TEXT_MUTED),
                                )
                                .frame(false),
                            )
                            .clicked()
                        {
                            state.show_update_modal = false;
                        }
                    });
                }
            });

            ui.add_space(16.0);
            ui.add(egui::Separator::default().horizontal().spacing(0.0));
            ui.add_space(12.0);

            // ── Version row ───────────────────────────────────────────────────
            if let Some(ref latest) = state.update_available_version.clone() {
                ui.horizontal(|ui| {
                    version_chip(ui, &format!("v{}", env!("CARGO_PKG_VERSION")), theme::ELEVATED_BG, theme::TEXT_MUTED);
                    ui.add_space(6.0);
                    ui.label(RichText::new("→").size(14.0).color(theme::TEXT_MUTED));
                    ui.add_space(6.0);
                    version_chip(ui, &format!("v{}", latest), theme::BLURPLE, Color32::WHITE);
                });
                ui.add_space(14.0);

                // ── Release notes ─────────────────────────────────────────────
                if let Some(ref notes) = state.update_release_notes.clone() {
                    ui.label(
                        RichText::new("What's New").size(12.0).color(theme::TEXT_MUTED).strong(),
                    );
                    ui.add_space(6.0);
                    Frame::default()
                        .fill(theme::ELEVATED_BG)
                        .corner_radius(egui::CornerRadius::same(8u8))
                        .inner_margin(Margin::same(12i8))
                        .show(ui, |ui| {
                            ScrollArea::vertical()
                                .id_salt("update_notes_scroll")
                                .max_height(160.0)
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.label(
                                        RichText::new(notes)
                                            .size(12.0)
                                            .color(theme::TEXT_PRIMARY),
                                    );
                                });
                        });
                    ui.add_space(16.0);
                }
            } else {
                // Up-to-date state
                ui.vertical_centered(|ui| {
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(format!(
                            "VoxLink {} is the latest version.",
                            env!("CARGO_PKG_VERSION")
                        ))
                        .size(14.0)
                        .color(theme::TEXT_PRIMARY),
                    );
                    ui.add_space(8.0);
                });
            }

            // ── Progress section (visible while downloading) ──────────────────
            if state.update_in_progress {
                let (pct, label) = if state.update_download_total > 0 {
                    let frac =
                        state.update_download_progress as f32 / state.update_download_total as f32;
                    let label = format!(
                        "{} / {}",
                        format_bytes(state.update_download_progress),
                        format_bytes(state.update_download_total),
                    );
                    (frac.clamp(0.0, 1.0), label)
                } else {
                    // Indeterminate (extracting / installing phase)
                    let t = ctx.input(|i| i.time) as f32;
                    let frac = ((t * 0.5).sin() * 0.5 + 0.5) * 0.15 + 0.85; // 0.85–1.0
                    (frac, state.update_phase.clone())
                };

                ui.add(
                    ProgressBar::new(pct)
                        .fill(theme::BLURPLE)
                        .desired_width(f32::INFINITY)
                        .corner_radius(egui::CornerRadius::same(4u8)),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add(egui::widgets::Spinner::new().size(14.0).color(theme::BLURPLE));
                    ui.add_space(6.0);
                    ui.label(RichText::new(&label).size(12.0).color(theme::TEXT_MUTED));
                });
                ui.add_space(8.0);
                ctx.request_repaint(); // animate the progress bar
            }

            // ── Error display ─────────────────────────────────────────────────
            if let Some(ref err) = state.update_error.clone() {
                Frame::default()
                    .fill(Color32::from_rgba_premultiplied(180, 30, 30, 60))
                    .corner_radius(egui::CornerRadius::same(6u8))
                    .inner_margin(Margin::same(10i8))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!("Error: {}", err))
                                .size(12.0)
                                .color(Color32::from_rgb(255, 120, 120)),
                        );
                    });
                ui.add_space(12.0);
            }

            // ── Action buttons ────────────────────────────────────────────────
            if !state.update_in_progress {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if state.update_available_version.is_some() {
                        if components::accent_button(ui, "Update Now").clicked() {
                            if let Some(url) = state.update_asset_url.clone() {
                                state.update_in_progress       = true;
                                state.update_error             = None;
                                state.update_download_progress = 0;
                                state.update_download_total    = 0;
                                state.update_phase             = "Preparing...".to_string();
                                crate::net::updater::run_update(
                                    url,
                                    state.update_asset_size,
                                    state.updater_tx.clone(),
                                );
                            }
                        }
                        ui.add_space(8.0);
                        if components::ghost_button(ui, "Later").clicked() {
                            state.show_update_modal = false;
                        }
                    } else {
                        // "Up to date" or error state — just a close button
                        let label = if state.update_error.is_some() { "Retry" } else { "Close" };
                        if components::accent_button(ui, label).clicked() {
                            if state.update_error.is_some() {
                                // Re-attempt: clear error, restart check
                                state.update_error = None;
                                if let Some(url) = state.update_asset_url.clone() {
                                    state.update_in_progress       = true;
                                    state.update_download_progress = 0;
                                    state.update_download_total    = 0;
                                    state.update_phase             = "Preparing...".to_string();
                                    crate::net::updater::run_update(
                                        url,
                                        state.update_asset_size,
                                        state.updater_tx.clone(),
                                    );
                                }
                            } else {
                                state.show_update_modal = false;
                            }
                        }
                    }

                    // "Check again" link when not downloading
                    if !state.update_check_in_progress && state.update_available_version.is_none() {
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Check again")
                                            .size(12.0)
                                            .color(theme::BLURPLE),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                            {
                                state.update_check_in_progress = true;
                                state.last_update_check        = std::time::Instant::now();
                                crate::net::updater::check_for_updates(
                                    state.updater_tx.clone(),
                                );
                            }
                        });
                    }
                });
            }
        });
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn version_chip(ui: &mut egui::Ui, label: &str, bg: Color32, fg: Color32) {
    Frame::default()
        .fill(bg)
        .corner_radius(egui::CornerRadius::same(6u8))
        .inner_margin(Margin::symmetric(10i8, 4i8))
        .show(ui, |ui| {
            ui.label(RichText::new(label).size(13.0).color(fg).strong());
        });
}
