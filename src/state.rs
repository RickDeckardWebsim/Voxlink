// ─────────────────────────────────────────────────────────────────────────────
// state.rs — Core application state
//
// This module is intentionally free of any async-runtime types (no Tokio
// imports) so that it can compile cleanly on both native and wasm32 targets.
// The channel types for network communication are added in Phase 2 using
// std::sync::mpsc, which works on all platforms.
// ─────────────────────────────────────────────────────────────────────────────

use serde::{Deserialize, Serialize};
use directories::ProjectDirs;
use std::fs;
use std::path::PathBuf;

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

/// Kind of media file attached to a message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AttachmentKind {
    Image,
    Audio,
    Video,
}

/// A file attached to a chat message, stored in Supabase Storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub url: String,
    pub kind: AttachmentKind,
    pub filename: String,
}

/// Value produced by the background avatar-upload thread.
pub struct ProfileUploadResult {
    /// New avatar public URL, or `None` if only a username was saved.
    pub avatar_url: Option<String>,
    /// Fresh token pair if the upload had to refresh the JWT; caller must persist.
    pub new_tokens: Option<(String, String)>,
}

/// Value produced by the background media-upload thread.
pub struct MediaUploadResult {
    pub url: String,
    pub kind: AttachmentKind,
    pub filename: String,
    pub caption: String,
    /// Fresh token pair if the upload had to refresh the JWT; caller must persist.
    pub new_tokens: Option<(String, String)>,
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
    pub attachment: Option<Attachment>,
    /// Unix epoch seconds of creation.
    /// System messages (join/leave) use this to drive the 2-hour auto-prune.
    /// Persistent messages (Own/Peer) set this to 0 — they never expire.
    #[serde(default)]
    pub unix_ts: u64,
}

impl ChatMessage {
    pub fn new_own(
        author: impl Into<String>,
        content: impl Into<String>,
        id: u64,
        attachment: Option<Attachment>,
    ) -> Self {
        Self {
            id,
            author: author.into(),
            content: content.into(),
            timestamp: timestamp_now(),
            kind: MessageKind::Own,
            attachment,
            unix_ts: 0,
        }
    }

    pub fn new_peer(
        author: impl Into<String>,
        content: impl Into<String>,
        id: u64,
        attachment: Option<Attachment>,
    ) -> Self {
        Self {
            id,
            author: author.into(),
            content: content.into(),
            timestamp: timestamp_now(),
            kind: MessageKind::Peer,
            attachment,
            unix_ts: 0,
        }
    }

    pub fn new_system(content: impl Into<String>, id: u64) -> Self {
        Self {
            id,
            author: "VoxLink".to_string(),
            content: content.into(),
            timestamp: timestamp_now(),
            kind: MessageKind::System,
            attachment: None,
            unix_ts: unix_now(),
        }
    }
}

/// Unix epoch seconds — used for system-message age checks.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ── Peers ─────────────────────────────────────────────────────────────────────

/// Represents a connected remote user visible in the sidebar.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub username: String,
    pub avatar_url: Option<String>,
    /// Whether this peer has joined the voice channel.
    pub in_voice: bool,
    /// Whether this peer is currently producing audio above the speaking threshold.
    pub is_speaking: bool,
    /// Whether this peer has muted their microphone.
    pub is_muted: bool,
    #[allow(dead_code)]
    pub peer_id: Option<String>,
}

// ── Session ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    pub user_id: String,
    pub email: String,
    pub username: String,
    pub avatar_url: Option<String>,
}

impl Session {
    pub fn load() -> Option<Self> {
        if let Some(proj_dirs) = ProjectDirs::from("com", "VoxLink", "VoxLinkApp") {
            let path = proj_dirs.config_dir().join("session.json");
            if let Ok(content) = fs::read_to_string(&path) {
                return serde_json::from_str(&content).ok();
            }
        }
        None
    }

    pub fn save(&self) {
        if let Some(proj_dirs) = ProjectDirs::from("com", "VoxLink", "VoxLinkApp") {
            let dir = proj_dirs.config_dir();
            let _ = fs::create_dir_all(dir);
            let path = dir.join("session.json");
            if let Ok(content) = serde_json::to_string_pretty(self) {
                let _ = fs::write(path, content);
            }
        }
    }

    pub fn clear() {
        if let Some(proj_dirs) = ProjectDirs::from("com", "VoxLink", "VoxLinkApp") {
            let path = proj_dirs.config_dir().join("session.json");
            let _ = fs::remove_file(path);
        }
    }
}

// ── Network event bridge (Phase 2+) ──────────────────────────────────────────

/// Events sent FROM the async network task TO the egui UI thread.
#[derive(Debug, Clone)]
#[allow(dead_code)] // wired in Phase 2
pub enum NetEvent {
    /// A new peer joined the signaling channel (avatar_url may be None for legacy clients).
    PeerJoined { from: String, avatar_url: Option<String> },
    /// A peer disconnected.
    PeerLeft(String),
    /// A text or media message was received from a peer.
    MessageReceived { from: String, content: String, attachment: Option<Attachment> },
    /// Successfully connected to the signaling server.
    Connected,
    /// Connection to the signaling server was lost.
    Disconnected,
    /// A recoverable error occurred.
    Error(String),
    /// A peer's voice state changed (speaking, muted, joined/left voice).
    VoiceStateUpdate { from: String, speaking: bool, muted: bool, in_voice: bool },
}

/// Commands sent FROM the egui UI thread TO the async network task.
#[derive(Debug)]
#[allow(dead_code)]
pub enum UiCommand {
    Connect { username: String },
    SendMessage(String),
    SendMedia { caption: String, url: String, kind: AttachmentKind, filename: String },
    /// Join (true) or leave (false) the voice channel.
    ToggleVoice(bool),
    /// Mute (true) or unmute (false) the local microphone while remaining in voice.
    SetMuted(bool),
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
    pub is_registering: bool,
    pub email_input: String,
    pub password_input: String,
    pub username_input: String,
    pub auth_error: Option<String>,
    pub auth_in_progress: bool,
    pub auth_rx: Option<std::sync::mpsc::Receiver<Result<Session, String>>>,

    /// Set to true on the very first frame to auto-focus.
    pub focus_input: bool,

    // ── Session ──
    pub session: Option<Session>,
    pub username: String, // Kept for quick access, mirrors session.username when logged in

    // ── Profile Modal ──
    pub show_profile_modal: bool,
    pub profile_in_progress: bool,
    pub profile_error: Option<String>,

    // ── Media Upload ──
    pub media_in_progress: bool,
    pub media_rx: Option<std::sync::mpsc::Receiver<Result<MediaUploadResult, String>>>,

    // ── Token refresh ──
    // Background refresh of the Supabase JWT on startup; result is (access_token, refresh_token).
    pub session_refresh_rx: Option<std::sync::mpsc::Receiver<Result<(String, String), String>>>,

    // ── Chat history ──
    // One-shot receiver for the history fetch that fires on connect.
    pub history_rx: Option<std::sync::mpsc::Receiver<Result<Vec<ChatMessage>, String>>>,

    // ── Updater ──
    pub updater_tx: std::sync::mpsc::Sender<crate::net::updater::UpdaterEvent>,
    pub updater_rx: std::sync::mpsc::Receiver<crate::net::updater::UpdaterEvent>,
    pub update_available_version: Option<String>,
    pub update_in_progress: bool,
    pub update_error: Option<String>,
    pub profile_rx: Option<std::sync::mpsc::Receiver<Result<ProfileUploadResult, String>>>,

    // ── Chat ──
    pub messages: Vec<ChatMessage>,
    pub message_input: String,
    pub next_message_id: u64,
    /// Set true when a new message arrives; consumed by the scroll area.
    pub scroll_to_bottom: bool,

    // ── Voice ──
    pub voice_active: bool,
    /// Whether the local mic is muted (audio still captured, but not transmitted).
    pub is_muted: bool,
    /// Whether the local user is currently speaking (from audio RMS detection).
    pub is_speaking: bool,

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
        let session = Session::load();
        let (updater_tx, updater_rx) = std::sync::mpsc::channel();

        // Kick off update check
        crate::net::updater::check_for_updates(updater_tx.clone());

        // If a saved session exists, immediately refresh the access token in the background.
        // Supabase JWTs expire after ~1 hour; the refresh token lasts much longer.
        let session_refresh_rx = session.as_ref().map(|s| {
            let rt = s.refresh_token.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let result = crate::net::supabase::refresh_session(&rt)
                    .map_err(|e| e.to_string());
                let _ = tx.send(result);
            });
            rx
        });

        let mut state = Self {
            screen: if session.is_some() { Screen::Chat } else { Screen::Login },
            is_registering: false,
            email_input: String::new(),
            password_input: String::new(),
            username_input: String::new(),
            auth_error: None,
            auth_in_progress: false,
            auth_rx: None,
            focus_input: true,
            username: session.as_ref().map(|s| s.username.clone()).unwrap_or_default(),
            session: session.clone(),
            show_profile_modal: false,
            profile_in_progress: false,
            profile_error: None,
            media_in_progress: false,
            media_rx: None,
            session_refresh_rx,
            history_rx: None,
            updater_tx,
            updater_rx,
            update_available_version: None,
            update_in_progress: false,
            update_error: None,
            profile_rx: None,
            messages: Vec::new(),
            message_input: String::new(),
            next_message_id: 0,
            scroll_to_bottom: false,
            voice_active: false,
            is_muted: false,
            is_speaking: false,
            needs_connect: false,
            peers: Vec::new(),
            net_rx: None,
            cmd_tx: None,
        };

        if state.session.is_some() {
            state.push_system(format!("Welcome back, {}! Connecting to signaling...", state.username));
            state.needs_connect = true;
        } else {
            state.push_system("Welcome to VoxLink! Please log in to connect.");
        }
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
        self.push_own_media(content, None);
    }

    pub fn push_own_media(&mut self, content: impl Into<String>, attachment: Option<Attachment>) {
        let id = self.next_id();
        let author = self.username.clone();
        self.messages.push(ChatMessage::new_own(author, content, id, attachment));
        self.scroll_to_bottom = true;
    }

    pub fn push_peer(&mut self, author: impl Into<String>, content: impl Into<String>) {
        self.push_peer_media(author, content, None);
    }

    pub fn push_peer_media(
        &mut self,
        author: impl Into<String>,
        content: impl Into<String>,
        attachment: Option<Attachment>,
    ) {
        let id = self.next_id();
        self.messages.push(ChatMessage::new_peer(author, content, id, attachment));
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
        let mut did_work = false;
        
        let events = self.poll_network_events();
        if !events.is_empty() {
            did_work = true;
            for event in events {
                self.apply_net_event(event);
            }
        }
        
        // Poll chat history (one-shot, fires once after connect)
        if let Some(result) = self.history_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.history_rx = None;
            did_work = true;
            match result {
                Ok(mut history) if !history.is_empty() => {
                    let n = history.len();
                    // Assign fresh widget IDs that don't collide with already-used ones
                    for msg in &mut history {
                        msg.id = self.next_message_id;
                        self.next_message_id += 1;
                    }
                    // Prepend history so it appears before any live/system messages
                    let tail: Vec<ChatMessage> = self.messages.drain(..).collect();
                    self.messages = history;
                    self.messages.extend(tail);
                    self.scroll_to_bottom = true;
                    log::info!("[history] Prepended {} messages from history", n);
                }
                Ok(_)  => log::info!("[history] No prior messages in this channel"),
                Err(e) => log::warn!("[history] Load failed: {}", e),
            }
        }

        // Prune system messages (join / leave notifications) older than 2 hours
        let now = unix_now();
        self.messages.retain(|m| {
            m.kind != MessageKind::System
                || m.unix_ts == 0
                || now.saturating_sub(m.unix_ts) < 7200
        });

        // Also poll updater events
        while let Ok(event) = self.updater_rx.try_recv() {
            did_work = true;
            match event {
                crate::net::updater::UpdaterEvent::UpdateAvailable(version) => {
                    self.update_available_version = Some(version);
                }
                crate::net::updater::UpdaterEvent::UpdateFinished => {
                    self.update_in_progress = false;
                    // App should restart momentarily
                }
                crate::net::updater::UpdaterEvent::UpdateFailed(err) => {
                    self.update_in_progress = false;
                    self.update_error = Some(err);
                }
            }
        }

        did_work
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
