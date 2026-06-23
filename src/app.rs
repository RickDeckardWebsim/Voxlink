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

        let username = self.state.username.clone();
        self.tokio_rt.spawn(net::webrtc::run(username, net_tx, cmd_rx, ctx));

        log::info!("[app] Signaling task spawned");
    }
}

// ── eframe::App ───────────────────────────────────────────────────────────────

impl eframe::App for VoxLinkApp {
    /// Required entry point in egui 0.34.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

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
                self.push_system("🟢 Connected to VoxLink signaling. Waiting for peers…");
            }

            NetEvent::Disconnected => {
                self.push_system("🔴 Disconnected from signaling server.");
                // Clear peer list so we don't show stale entries
                self.peers.clear();
            }

            NetEvent::PeerJoined(username) => {
                if !self.peers.iter().any(|p| p.username == username) {
                    self.peers.push(PeerInfo {
                        username: username.clone(),
                        avatar_url: None,
                        voice_active: false,
                        peer_id: None,
                    });
                }
                self.push_system(format!("👋 {} joined the room.", username));
            }

            NetEvent::PeerLeft(username) => {
                self.peers.retain(|p| p.username != username);
                self.push_system(format!("👋 {} left the room.", username));
            }

            NetEvent::MessageReceived { from, content, attachment } => {
                self.push_peer_media(from, content, attachment);
            }

            NetEvent::Error(msg) => {
                self.push_system(format!("⚠ Signaling error: {}", msg));
            }
        }
    }
}
