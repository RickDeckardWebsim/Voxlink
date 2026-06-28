// ─────────────────────────────────────────────────────────────────────────────
// ui/components.rs — Reusable UI widgets  (egui 0.34 compatible)
// ─────────────────────────────────────────────────────────────────────────────

use egui::{Color32, CornerRadius, FontId, Painter, Pos2, Rect, Response, RichText, Ui, Vec2};

use crate::state::{AppState, ChatMessage, MessageKind};
use super::theme;

/// Action returned by `render_message` when the user interacts with a message.
pub enum MessageAction {
    /// The user clicked a reaction pill (or quick-emoji in the context menu).
    ReactionToggle { message_id: String, emoji: String, active: bool },
    /// The user clicked "Reply" — caller should set `reply_target` and focus the input.
    Reply { db_id: String, author: String, content: String },
}

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
                
                // Triangle fan: center vertex shared, one triangle per edge segment.
                // Vertex layout per triangle: [p0, p1, center] at [base, base+1, base+2].
                // Indices must be computed per-iteration — hardcoding them breaks every
                // triangle after the first because the index buffer is global.
                let n = 32;
                for i in 0..n {
                    let a0 = i as f32 * std::f32::consts::TAU / n as f32;
                    let a1 = (i + 1) as f32 * std::f32::consts::TAU / n as f32;
                    let p0 = center + Vec2::new(a0.cos(), a0.sin()) * r;
                    let p1 = center + Vec2::new(a1.cos(), a1.sin()) * r;
                    let uv0 = uv_center + Vec2::new(a0.cos(), a0.sin()) * uv_r;
                    let uv1 = uv_center + Vec2::new(a1.cos(), a1.sin()) * uv_r;
                    let base = mesh.vertices.len() as u32;
                    mesh.vertices.push(egui::epaint::Vertex { pos: p0,     uv: uv0,       color });
                    mesh.vertices.push(egui::epaint::Vertex { pos: p1,     uv: uv1,       color });
                    mesh.vertices.push(egui::epaint::Vertex { pos: center, uv: uv_center, color });
                    mesh.add_triangle(base + 2, base, base + 1);
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

pub fn render_message(
    ui: &mut Ui,
    msg: &ChatMessage,
    show_header: bool,
    avatar_url: Option<&str>,
    local_username: &str,
    known_users: &[String],
    mention_color: Color32,
) -> Option<MessageAction> {
    match msg.kind {
        MessageKind::System => { render_system_message(ui, msg); None }
        MessageKind::Own | MessageKind::Peer => render_chat_message(ui, msg, show_header, avatar_url, local_username, known_users, mention_color),
    }
}

fn render_system_message(ui: &mut Ui, msg: &ChatMessage) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        ui.label(
            RichText::new(">")
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

fn render_chat_message(
    ui: &mut Ui,
    msg: &ChatMessage,
    show_header: bool,
    avatar_url: Option<&str>,
    local_username: &str,
    known_users: &[String],
    mention_color: Color32,
) -> Option<MessageAction> {
    ui.add_space(if show_header { 10.0 } else { 1.0 });

    let author_color = theme::avatar_color(&msg.author);
    let mut action = None;

    ui.horizontal_top(|ui| {
        ui.add_space(12.0);

        if show_header {
            draw_avatar(ui, &msg.author, avatar_url, theme::AVATAR_SIZE);
            ui.add_space(8.0);
        } else {
            ui.add_space(theme::AVATAR_SIZE + 8.0);
        }

        ui.vertical(|ui| {
            // ── Reply reference (above the body) ──────────────────────────────
            if let (Some(_to), Some(author), Some(snippet)) = (&msg.reply_to, &msg.reply_to_author, &msg.reply_to_content) {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("↪").size(11.0).color(theme::TEXT_MUTED));
                    ui.label(RichText::new(format!("@{}: {}", author, snippet)).size(11.0).color(theme::TEXT_MUTED));
                });
                ui.add_space(2.0);
            }
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
            // ── Content with @mention highlighting ───────────────────────────
            // Split content into plain-text and mention segments. A `@<token>`
            // is a mention iff <token> matches a known user (self or a peer);
            // unmatched `@foo` renders as plain text. Mentions are styled in
            // the user-customizable mention color; plain text in theme::TEXT_PRIMARY.
            let segments = split_mentions(&msg.content, known_users);
            let content_resp = ui.horizontal_wrapped(|ui| {
                for (text, is_mention) in segments {
                    let color = if is_mention { mention_color } else { theme::TEXT_PRIMARY };
                    ui.add(
                        egui::Label::new(RichText::new(text).size(14.0).color(color))
                            .wrap_mode(egui::TextWrapMode::Wrap),
                    );
                }
            })
            .response;
            if let Some(att) = &msg.attachment {
                render_attachment(ui, att);
            }

            // ── Reactions ────────────────────────────────────────────────────
            if !msg.reactions.is_empty() {
                if let Some((message_id, emoji, active)) = render_reactions(ui, msg, local_username) {
                    action = Some(MessageAction::ReactionToggle { message_id, emoji, active });
                }
            }

            // ── Right-click reaction picker ──────────────────────────────────
            content_resp.context_menu(|ui| {
                ui.horizontal(|ui| {
                    for emoji in QUICK_EMOJIS {
                        if ui.button(RichText::new(*emoji).size(18.0)).clicked() {
                            if let Some(db_id) = &msg.db_id {
                                let already = msg.reactions.iter().any(|r| r.user == local_username && r.emoji == *emoji);
                                action = Some(MessageAction::ReactionToggle { message_id: db_id.clone(), emoji: emoji.to_string(), active: !already });
                            }
                        }
                    }
                });
                ui.separator();
                if ui.button("Reply").clicked() {
                    if let Some(db_id) = &msg.db_id {
                        let snippet: String = msg.content.chars().take(100).collect();
                        action = Some(MessageAction::Reply { db_id: db_id.clone(), author: msg.author.clone(), content: snippet });
                    }
                }
            });
        });

        ui.add_space(12.0);
    });

    action
}

/// Split message content into `(text, is_mention)` segments for highlighting.
///
/// A `@<token>` is a mention iff:
///   • the `@` is at the start of the content or preceded by a word boundary
///     (whitespace or non-alphanumeric), and
///   • `<token>` (the maximal run of username chars `[A-Za-z0-9_.\-]`) exactly
///     equals a known username.
/// Unmatched `@foo` (no known user) and everything else renders as plain text.
fn split_mentions(content: &str, known_users: &[String]) -> Vec<(String, bool)> {
    // Lookup set of known usernames for O(1) exact membership.
    let known: std::collections::HashSet<&str> =
        known_users.iter().map(|s| s.as_str()).collect();

    let is_token_char = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-';

    let mut segments: Vec<(String, bool)> = Vec::new();
    let mut plain = String::new();
    let chars: Vec<char> = content.chars().collect();
    let mut i = 0;
    let n = chars.len();

    while i < n {
        if chars[i] == '@' {
            // Word-boundary check: @ must start the content or follow a
            // non-alphanumeric, non-token char (e.g. whitespace, punctuation).
            let at_boundary = i == 0 || !is_token_char(chars[i - 1]);
            if at_boundary {
                // Read the maximal token after '@'.
                let start = i + 1;
                let mut end = start;
                while end < n && is_token_char(chars[end]) {
                    end += 1;
                }
                if end > start {
                    let token: String = chars[start..end].iter().collect();
                    if known.contains(token.as_str()) {
                        // Flush accumulated plain text first.
                        if !plain.is_empty() {
                            segments.push((std::mem::take(&mut plain), false));
                        }
                        // Emit the mention (include the '@').
                        segments.push((format!("@{}", token), true));
                        i = end;
                        continue;
                    }
                }
            }
        }
        plain.push(chars[i]);
        i += 1;
    }

    if !plain.is_empty() {
        segments.push((plain, false));
    }

    // If nothing was produced (e.g. empty content), return one empty plain
    // segment so the content row still lays out correctly.
    if segments.is_empty() {
        segments.push((String::new(), false));
    }

    segments
}

/// True if `content` contains an @mention of `username` at a word boundary.
/// Mirrors the web `mentionsUser` regex semantics — `@jo` does NOT match
/// `@joseph`. Uses the same token-char and boundary rules as `split_mentions`.
pub fn is_mentioned(content: &str, username: &str) -> bool {
    if username.is_empty() { return false; }
    let is_token_char = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-';
    let chars: Vec<char> = content.chars().collect();
    let n = chars.len();
    let target: Vec<char> = username.chars().collect();
    let mut i = 0;
    while i < n {
        if chars[i] == '@' {
            let at_boundary = i == 0 || !is_token_char(chars[i - 1]);
            if at_boundary {
                // Check if the token starting at i+1 exactly matches username
                // AND is followed by a non-token char (word boundary on the right too).
                let start = i + 1;
                let mut end = start;
                while end < n && is_token_char(chars[end]) {
                    end += 1;
                }
                if end - start == target.len() {
                    let token: String = chars[start..end].iter().collect();
                    if token == username {
                        return true;
                    }
                }
            }
        }
        i += 1;
    }
    false
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

const QUICK_EMOJIS: &[&str] = &["👍", "❤️", "😂", "😮", "😢", "🙏"];

/// Render reaction pills below a message. Returns Some((db_id, emoji, active)) when a pill is clicked.
fn render_reactions(ui: &mut Ui, msg: &ChatMessage, local_username: &str) -> Option<(String, String, bool)> {
    let mut clicked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        // Group reactions by emoji, count distinct users.
        let mut groups: std::collections::HashMap<&str, Vec<&str>> = std::collections::HashMap::new();
        for r in &msg.reactions {
            groups.entry(r.emoji.as_str()).or_default().push(r.user.as_str());
        }
        for (emoji, users) in &groups {
            let count = users.len();
            let reacted = users.iter().any(|u| *u == local_username);
            let label = format!("{} {}", emoji, count);
            let pill = egui::Button::new(RichText::new(&label).size(12.0))
                .fill(if reacted { theme::ELEVATED_BG } else { Color32::TRANSPARENT })
                .stroke(egui::Stroke::new(1.0, theme::SEPARATOR))
                .corner_radius(CornerRadius::same(10u8));
            if ui.add(pill).clicked() {
                if let Some(db_id) = &msg.db_id {
                    clicked = Some((db_id.clone(), emoji.to_string(), !reacted));
                }
            }
        }
    });
    clicked
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

pub fn sidebar_user_row(
    ui: &mut Ui,
    username: &str,
    avatar_url: Option<&str>,
    is_self: bool,
    in_voice: bool,
    is_speaking: bool,
    is_muted: bool,
) {
    egui::Frame::default()
        .fill(Color32::TRANSPARENT)
        .corner_radius(CornerRadius::same(6u8))
        .inner_margin(egui::Margin::symmetric(8i8, 4i8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let avatar_rect = draw_avatar(ui, username, avatar_url, 28.0);

                // Animated speaking ring — fades in/out smoothly.
                let t = ui.ctx().animate_bool(
                    egui::Id::new(("speaking_ring", username)),
                    is_speaking && !is_muted,
                );
                if t > 0.0 {
                    ui.painter().circle_stroke(
                        avatar_rect.center(),
                        avatar_rect.width() / 2.0 + 2.5,
                        egui::Stroke::new(2.5 * t, theme::GREEN_ONLINE),
                    );
                }

                // Mute badge — small red circle bottom-right of avatar.
                if is_muted {
                    let br = avatar_rect.right_bottom() + Vec2::new(-1.0, -1.0);
                    ui.painter().circle_filled(br, 6.0, theme::SIDEBAR_BG);
                    ui.painter().circle_filled(br, 4.5, theme::RED_DANGER);
                    // Horizontal slash
                    ui.painter().line_segment(
                        [br - Vec2::new(2.5, 0.0), br + Vec2::new(2.5, 0.0)],
                        egui::Stroke::new(1.5, Color32::WHITE),
                    );
                } else {
                    let dot_center = avatar_rect.right_bottom() + Vec2::new(-2.0, -2.0);
                    draw_status_dot(ui.painter(), dot_center, 5.0, theme::GREEN_ONLINE);
                }

                ui.add_space(4.0);

                let display = if is_self { format!("{} (you)", username) } else { username.to_string() };
                ui.label(
                    RichText::new(display)
                        .size(13.0)
                        .color(if is_speaking && !is_muted { theme::GREEN_ONLINE }
                               else if is_self { theme::TEXT_MUTED }
                               else { theme::TEXT_PRIMARY }),
                );

                if in_voice {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new("\u{25CF}").size(10.0).color(theme::GREEN_ONLINE));
                    });
                }
            });
        });
}

// ── Inspect Panel ─────────────────────────────────────────────────────────────

pub fn render_inspect_panel(ctx: &egui::Context, state: &mut AppState) {
    let username = match &state.inspected_peer {
        Some(u) => u.clone(),
        None => return,
    };
    // Build display info from session (if own) or peers list
    let (avatar_url, description) = if username == state.username {
        (
            state.session.as_ref().and_then(|s| s.avatar_url.clone()),
            Some(state.profile_description.clone()),
        )
    } else {
        let peer = state.peers.iter().find(|p| p.username == username);
        (
            peer.and_then(|p| p.avatar_url.clone()),
            peer.and_then(|p| p.description.clone()),
        )
    };

    let mut open = true;
    egui::Window::new("User Profile")
        .id(egui::Id::new("inspect_panel"))
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .anchor(egui::Align2::LEFT_TOP, [theme::SIDEBAR_WIDTH + 8.0, 80.0])
        .frame(egui::Frame::window(&ctx.style()).fill(theme::SIDEBAR_BG).inner_margin(16.0))
        .show(ctx, |ui| {
            ui.set_min_width(260.0);
            ui.set_max_width(300.0);
            ui.horizontal(|ui| {
                draw_avatar(ui, &username, avatar_url.as_deref(), 56.0);
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(&username).size(16.0).color(Color32::WHITE).strong());
                    ui.label(RichText::new("Online").size(11.0).color(theme::GREEN_ONLINE));
                });
            });
            if let Some(desc) = &description {
                if !desc.is_empty() {
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(6.0);
                    ui.label(RichText::new("About Me").size(11.0).color(theme::TEXT_MUTED).strong());
                    ui.add_space(4.0);
                    ui.add(
                        egui::Label::new(RichText::new(desc).size(13.0).color(theme::TEXT_PRIMARY))
                            .wrap_mode(egui::TextWrapMode::Wrap),
                    );
                }
            }
        });
    if !open {
        state.inspected_peer = None;
    }
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
