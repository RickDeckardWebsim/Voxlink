// ─────────────────────────────────────────────────────────────────────────────
// state.rs — Core application state
//
// This module is intentionally free of any async-runtime types (no Tokio
// imports) so that it can compile cleanly on both native and wasm32 targets.
// The channel types for network communication are added in Phase 2 using
// std::sync::mpsc, which works on all platforms.
// ─────────────────────────────────────────────────────────────────────────────

use serde::{Deserialize, Serialize};
use std::sync::mpsc;

// ── Routing ──────────────────────────────────────────────────────────────────

/// Which top-level screen the app is currently showing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Login,
    Chat,
}

// ── Messages ─────────────────────────────────────────────────────────────────

/// Distinguishes how a message should be visually rendered.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MessageKind {
    /// Sent by the local user — rendered on the right with accent color name.
    Own,
    /// Received from a remote peer — rendered on the left.
    Peer,
    /// VoxLink system notification (join / leave / error).
    System,
}

/// A single entry in the chat log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Monotonically increasing ID (used as egui widget Id source).
    pub id: u64,
    pub author: String,
    pub content: String,
    /// Formatted as "HH:MM" local time.
    pub timestamp: String,
    pub kind: MessageKind,
}

impl ChatMessage {
    pub fn new_own(author: impl Into<String>, content: impl Into<String>, id: u64) -> Self {
        Self {
            id,
            author: author.into(),
            content: content.into(),
            timestamp: timestamp_now(),
            kind: MessageKind::Own,
        }
    }

    pub fn new_peer(author: impl Into<String>, content: impl Into<String>, id: u64) -> Self {
        Self {
            id,
            author: author.into(),
            content: content.into(),
            timestamp: timestamp_now(),
            kind: MessageKind::Peer,
        }
    }

    pub fn new_system(content: impl Into<String>, id: u64) -> Self {
        Self {
            id,
            author: "VoxLink".to_string(),
            content: content.into(),
            timestamp: timestamp_now(),
            kind: MessageKind::System,
        }
    }
}

// ── Peers ─────────────────────────────────────────────────────────────────────

/// Represents a connected remote user visible in the sidebar.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub username: String,
    /// Whether this peer has their microphone active (Phase 4).
    pub voice_active: bool,
    /// WebRTC peer ID assigned at connection (Phase 3).
    #[allow(dead_code)]
    pub peer_id: Option<String>,
}

// ── Network event bridge (Phase 2+) ──────────────────────────────────────────

/// Events sent FROM the async network task TO the egui UI thread.
#[derive(Debug, Clone)]
#[allow(dead_code)] // wired in Phase 2
pub enum NetEvent {
    /// A new peer joined the signaling channel.
    PeerJoined(String),
    /// A peer disconnected.
    PeerLeft(String),
    /// A P2P text message was received (Phase 3).
    MessageReceived { from: String, content: String },
    /// Successfully connected to the signaling server.
    Connected,
    /// Connection to the signaling server was lost.
    Disconnected,
    /// A recoverable error occurred.
    Error(String),
}

/// Commands sent FROM the egui UI thread TO the async network task.
#[derive(Debug)]
#[allow(dead_code)] // wired in Phase 2
pub enum UiCommand {
    /// Start connecting with the given username.
    Connect { username: String },
    /// Send a P2P text message to all peers (Phase 3).
    SendMessage(String),
    /// Toggle the local microphone (Phase 4).
    ToggleVoice(bool),
    /// Gracefully disconnect.
    Disconnect,
}

// ── Core Application State ────────────────────────────────────────────────────

/// All mutable state for the VoxLink application.
///
/// Owned entirely by the egui thread. The async network task communicates
/// via `mpsc` channels stored in `net_rx` and `cmd_tx`.
pub struct AppState {
    // ── Routing ──
    pub screen: Screen,

    // ── Login ──
    pub username_input: String,
    /// Set to true on the very first frame to auto-focus the username field.
    pub focus_username: bool,

    // ── Session ──
    pub username: String,

    // ── Chat ──
    pub messages: Vec<ChatMessage>,
    pub message_input: String,
    pub next_message_id: u64,
    /// Set true when a new message arrives; consumed by the scroll area.
    pub scroll_to_bottom: bool,

    // ── Voice ──
    pub voice_active: bool,

    /// Set true by commit_login; consumed by app.rs to spawn the signaling task.
    pub needs_connect: bool,

    // ── Peers (populated in Phase 2+) ──
    pub peers: Vec<PeerInfo>,

    // ── Network channels (populated in Phase 2+) ──
    /// Receives async events from the network task each frame.
    pub net_rx: Option<std::sync::mpsc::Receiver<NetEvent>>,
    /// Sends UI commands to the async network task.
    pub cmd_tx: Option<tokio::sync::mpsc::UnboundedSender<UiCommand>>,
}

impl Default for AppState {
    fn default() -> Self {
        let mut state = Self {
            screen: Screen::Login,
            username_input: String::new(),
            focus_username: true,
            username: String::new(),
            messages: Vec::new(),
            message_input: String::new(),
            next_message_id: 0,
            scroll_to_bottom: false,
            voice_active: false,
            needs_connect: false,
            peers: Vec::new(),
            net_rx: None,
            cmd_tx: None,
        };

        state.push_system(
            "Welcome to VoxLink! Enter a username and press Enter to connect.",
        );
        state
    }
}

impl AppState {
    // ── Message helpers ───────────────────────────────────────────────────────

    pub fn push_system(&mut self, msg: impl Into<String>) {
        let id = self.next_id();
        self.messages.push(ChatMessage::new_system(msg, id));
        self.scroll_to_bottom = true;
    }

    pub fn push_own(&mut self, content: impl Into<String>) {
        let id = self.next_id();
        let author = self.username.clone();
        self.messages.push(ChatMessage::new_own(author, content, id));
        self.scroll_to_bottom = true;
    }

    pub fn push_peer(&mut self, author: impl Into<String>, content: impl Into<String>) {
        let id = self.next_id();
        self.messages.push(ChatMessage::new_peer(author, content, id));
        self.scroll_to_bottom = true;
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_message_id;
        self.next_message_id += 1;
        id
    }

    // ── Network event processing ──────────────────────────────────────────────

    /// Drain all pending network events, applying them to state.
    /// Returns `true` if any events were processed (caller should repaint).
    pub fn process_net_events(&mut self) -> bool {
        let events = self.poll_network_events();
        if events.is_empty() {
            return false;
        }

        for event in events {
            self.apply_net_event(event);
        }
        true
    }

    // apply_net_event is defined in app.rs to keep UI side-effects
    // (system messages, peer list updates) co-located with the app struct.
    pub fn poll_network_events(&self) -> Vec<NetEvent> {
        match &self.net_rx {
            Some(rx) => std::iter::from_fn(|| rx.try_recv().ok()).collect(),
            None => vec![],
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns the current local time as "HH:MM" using only std.
fn timestamp_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // UTC offset is not trivial without chrono; we use UTC here.
    // Phase 2 will add chrono for proper local-time formatting.
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    format!("{h:02}:{m:02}")
}
