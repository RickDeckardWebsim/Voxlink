// ─────────────────────────────────────────────────────────────────────────────
// app.rs — Top-level eframe application struct  (egui 0.34 + Phase 2)
//
// Responsibilities:
//   • Apply theme and style on startup
//   • Own the Tokio runtime (native builds)
//   • Spawn the signaling task when needs_connect is set
//   • Route incoming NetEvents to AppState
//   • Delegate rendering to the active screen module
// ─────────────────────────────────────────────────────────────────────────────

use crate::state::{AppState, NetEvent, PeerInfo, Screen, UiCommand};
use crate::ui;

#[cfg(not(target_arch = "wasm32"))]
use crate::net;

pub struct VoxLinkApp {
    pub state: AppState,

    /// Tokio async runtime — drives the signaling (and later WebRTC) task.
    /// On Wasm this will be replaced by `wasm_bindgen_futures::spawn_local`.
    #[cfg(not(target_arch = "wasm32"))]
    tokio_rt: tokio::runtime::Runtime,
}

impl VoxLinkApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Load system fonts before applying the theme so egui has a font with
        // comprehensive Unicode coverage. Falls back to bundled Ubuntu-Light.
        setup_fonts(&cc.egui_ctx);
        // Apply VoxLink dark theme + font sizes
        cc.egui_ctx.set_visuals(ui::theme::voxlink_visuals());
        cc.egui_ctx.global_style_mut(ui::theme::voxlink_style);

        Self {
            state: AppState::default(),
            #[cfg(not(target_arch = "wasm32"))]
            tokio_rt: tokio::runtime::Runtime::new()
                .expect("Fatal: failed to create Tokio runtime"),
        }
    }

    /// Spins up the async signaling task with a fresh pair of mpsc channels,
    /// then stores the receiver/sender ends in AppState for UI integration.
    #[cfg(not(target_arch = "wasm32"))]
    fn spawn_signaling(&mut self, ctx: egui::Context) {
        let (net_tx, net_rx) = std::sync::mpsc::channel();
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();

        self.state.net_rx = Some(net_rx);
        self.state.cmd_tx = Some(cmd_tx);

        let username    = self.state.username.clone();
        let avatar_url  = self.state.session.as_ref().and_then(|s| s.avatar_url.clone());
        let description = self.state.session.as_ref().and_then(|s| {
            if s.description.is_empty() { None } else { Some(s.description.clone()) }
        });
        self.tokio_rt.spawn(net::webrtc::run(username, avatar_url, description, net_tx, cmd_rx, ctx));

        log::info!("[app] Signaling task spawned");
    }
}

// ── eframe::App ───────────────────────────────────────────────────────────────

impl eframe::App for VoxLinkApp {
    /// Required entry point in egui 0.34.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // ── Poll background token refresh ─────────────────────────────────────
        let refresh_result = self.state.session_refresh_rx
            .as_ref()
            .and_then(|rx| rx.try_recv().ok());
        if let Some(result) = refresh_result {
            self.state.session_refresh_rx = None;
            match result {
                Ok((access_token, refresh_token)) => {
                    if let Some(ref mut s) = self.state.session {
                        s.access_token  = access_token;
                        s.refresh_token = refresh_token;
                        s.save();
                        log::info!("[app] Session token refreshed successfully.");
                    }
                }
                Err(e) => {
                    // Refresh token itself expired — force re-login.
                    log::warn!("[app] Token refresh failed ({}); signing out.", e);
                    crate::state::Session::clear();
                    self.state.session = None;
                    self.state.screen  = crate::state::Screen::Login;
                    self.state.push_system("Session expired. Please sign in again.");
                }
            }
        }

        // ── Spawn signaling on first frame after login ─────────────────────────
        if self.state.needs_connect {
            self.state.needs_connect = false;
            #[cfg(not(target_arch = "wasm32"))]
            self.spawn_signaling(ctx.clone());
        }

        // ── Drain incoming network events → update state ───────────────────────
        if self.state.process_net_events() {
            ctx.request_repaint();
        }

        // ── Render active screen ───────────────────────────────────────────────
        match self.state.screen {
            Screen::Login => ui::login::render(&ctx, &mut self.state),
            Screen::Chat  => ui::chat::render(&ctx, &mut self.state),
        }
    }

    /// Graceful shutdown: send Disconnect command so the signaling task
    /// announces departure before the Tokio runtime is dropped.
    fn on_exit(&mut self) {
        if let Some(tx) = &self.state.cmd_tx {
            let _ = tx.send(UiCommand::Disconnect);
        }
    }
}

// ── Font setup ───────────────────────────────────────────────────────────────
//
// egui's bundled Ubuntu-Light lacks many Unicode code points; this function
// inserts the platform's system UI font as the first (highest-priority) fallback
// so all printable characters render correctly without needing to bundle fonts.

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // Candidate paths, tried in order; first readable file wins.
    #[cfg(target_os = "windows")]
    let candidates: &[&str] = &[
        "C:/Windows/Fonts/segoeui.ttf",  // Segoe UI — default Windows UI font
        "C:/Windows/Fonts/arial.ttf",
    ];
    #[cfg(target_os = "macos")]
    let candidates: &[&str] = &[
        "/System/Library/Fonts/Helvetica.ttc",
        "/Library/Fonts/Arial.ttf",
    ];
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let candidates: &[&str] = &[
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
    ];

    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert(
                "SystemUI".to_owned(),
                std::sync::Arc::new(egui::FontData::from_owned(bytes)),
            );
            // Insert at index 0 so it's tried before Ubuntu-Light.
            // Ubuntu-Light and NotoEmoji remain as fallbacks for any missing glyphs.
            if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                family.insert(0, "SystemUI".to_owned());
            }
            log::info!("[app] Loaded system UI font: {}", path);
            break;
        }
    }

    ctx.set_fonts(fonts);
}

// ── NetEvent → AppState wiring ────────────────────────────────────────────────
//
// This is called by AppState::process_net_events() each frame.
// Keeping it here (rather than in state.rs) lets us add rich UI side-effects
// (sounds, notifications, etc.) without polluting the pure data model.

impl AppState {
    /// Called by process_net_events for every event drained from net_rx.
    pub fn apply_net_event(&mut self, event: NetEvent) {
        match event {
            NetEvent::Connected => {
                self.push_system("Connected to VoxLink signaling. Waiting for peers…");
                // Fetch chat history in the background so the user sees prior messages
                // immediately after connecting without blocking the UI.
                if let Some(ref s) = self.session {
                    let access_token = s.access_token.clone();
                    let username     = self.username.clone();
                    let (tx, rx)     = std::sync::mpsc::channel();
                    std::thread::spawn(move || {
                        let messages = crate::net::supabase::fetch_recent_messages(
                            &access_token, &username,
                        ).map_err(|e| e.to_string());

                        let result = match messages {
                            Ok(msgs) => {
                                let ids: Vec<String> = msgs.iter().filter_map(|m| m.db_id.clone()).collect();
                                let reactions = if ids.is_empty() {
                                    Ok(std::collections::HashMap::new())
                                } else {
                                    crate::net::supabase::fetch_reactions(&access_token, &ids)
                                        .map_err(|e| e.to_string())
                                };
                                match reactions {
                                    Ok(rxn_map) => {
                                        let mut hydrated = msgs;
                                        for msg in &mut hydrated {
                                            if let Some(ref db_id) = msg.db_id {
                                                if let Some(rxn) = rxn_map.get(db_id) {
                                                    msg.reactions = rxn.clone();
                                                }
                                            }
                                        }
                                        Ok(hydrated)
                                    }
                                    Err(e) => Err(e),
                                }
                            }
                            Err(e) => Err(e),
                        };
                        let _ = tx.send(result);
                    });
                    self.history_rx = Some(rx);
                }
            }

            NetEvent::ProfileUpdated { from, new_username, avatar_url, description } => {
                if let Some(peer) = self.peers.iter_mut().find(|p| p.username == from) {
                    peer.username = new_username;
                    if avatar_url.is_some() {
                        peer.avatar_url = avatar_url;
                    }
                    if description.is_some() {
                        peer.description = description;
                    }
                }
            }

            NetEvent::Disconnected => {
                self.push_system("Disconnected from signaling server.");
                // Clear peer list so we don't show stale entries
                self.peers.clear();
            }

            NetEvent::PeerJoined { from, avatar_url, description } => {
                if let Some(peer) = self.peers.iter_mut().find(|p| p.username == from) {
                    // Already known — update avatar/description if we now have them.
                    if avatar_url.is_some() {
                        peer.avatar_url = avatar_url;
                    }
                    if description.is_some() {
                        peer.description = description;
                    }
                } else {
                    self.peers.push(PeerInfo {
                        username:    from.clone(),
                        avatar_url,
                        description,
                        in_voice:    false,
                        is_speaking: false,
                        is_muted:    false,
                        peer_id:     None,
                    });
                    self.push_system(format!("{} joined the room.", from));
                }
            }

            NetEvent::PeerLeft(username) => {
                self.peers.retain(|p| p.username != username);
                self.push_system(format!("{} left the room.", username));
            }

            NetEvent::MessageReceived { from, content, attachment, message_id, reply_to, reply_to_author, reply_to_content } => {
                // Mention ping: word-boundary check so "@jo" doesn't match "@joseph".
                if crate::ui::components::is_mentioned(&content, &self.username) {
                    crate::audio::notification::play_notification();
                    self.ping_toast = Some(crate::state::PingToast {
                        author: from.clone(),
                        content: content.chars().take(100).collect(),
                        set_at: std::time::Instant::now(),
                    });
                }
                let reply = match (reply_to, reply_to_author, reply_to_content) {
                    (Some(to), Some(author), Some(c)) => Some((to, author, c)),
                    _ => None,
                };
                self.push_peer_media(from, content, attachment, message_id, reply);
            }

            NetEvent::Error(msg) => {
                self.push_system(format!("Signaling error: {}", msg));
            }

            NetEvent::VoiceStateUpdate { from, speaking, muted, in_voice } => {
                if from == self.username {
                    // Reflect own state back (e.g. from the capture thread echo).
                    self.is_speaking = speaking;
                } else {
                    if let Some(peer) = self.peers.iter_mut().find(|p| p.username == from) {
                        peer.is_speaking = speaking;
                        peer.is_muted    = muted;
                        peer.in_voice    = in_voice;
                    }
                }
            }

            // ── Typing + reactions (fully wired in Task 6) ───────────────────
            NetEvent::TypingUpdate { from, is_typing } => {
                if is_typing {
                    if !self.typing_users.contains(&from) {
                        self.typing_users.push(from);
                    }
                } else {
                    self.typing_users.retain(|u| u != &from);
                }
            }

            NetEvent::ReactionUpdate { from, message_id, emoji, active } => {
                if let Some(msg) = self.messages.iter_mut().find(|m| m.db_id.as_deref() == Some(&message_id)) {
                    if active {
                        if !msg.reactions.iter().any(|r| r.user == from && r.emoji == emoji) {
                            msg.reactions.push(crate::state::Reaction { user: from, emoji });
                        }
                    } else {
                        msg.reactions.retain(|r| !(r.user == from && r.emoji == emoji));
                    }
                }
            }
        }
    }
}
