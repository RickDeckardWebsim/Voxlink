// Cross-target shared constants and mappings.
// Canonical spec: docs/plans/2026-06-27-message-contract.md
// Mirrored by web/contract.js.

pub const SUPABASE_URL: &str = "https://syftqwloslmnjyvppler.supabase.co";
pub const SUPABASE_ANON_KEY: &str = "sb_publishable_VK3kO0lX4tTsrHlCsH6JFQ_ebB6_lMH";
/// Supabase Realtime channel name (without the `realtime:` prefix).
pub const SIGNALING_CHANNEL: &str = "p2p-signaling";
/// Realtime topic string used on the wire.
pub const SIGNALING_TOPIC: &str = "realtime:p2p-signaling";

/// Realtime broadcast event names. Canonical wire strings (spec §4).
/// `chat_message` and `chat_media` payloads now carry `message_id` and
/// `reply_to` / `reply_to_author` / `reply_to_content` (spec §4.2/§4.3/§4.9).
/// `peer_leave` is receive-only today (spec §8); included for completeness.
pub mod event {
    pub const PEER_JOIN: &str       = "peer_join";
    pub const PEER_LEAVE: &str      = "peer_leave";
    pub const CHAT_MESSAGE: &str    = "chat_message";
    pub const CHAT_MEDIA: &str      = "chat_media";
    pub const VOICE_STATE: &str     = "voice_state";
    pub const PROFILE_UPDATE: &str  = "profile_update";
    pub const SDP_OFFER: &str       = "sdp_offer";
    pub const SDP_ANSWER: &str      = "sdp_answer";
    pub const TYPING: &str        = "typing";
    pub const REACTION: &str      = "reaction";
}
/// PostgREST `messages.channel` value.
pub const DEFAULT_DB_CHANNEL: &str = "general";

/// MIME → attachment kind. Canonical table: contract spec §6.
pub fn mime_to_kind(mime: &str) -> crate::state::AttachmentKind {
    let lower = mime.to_ascii_lowercase();
    if lower.starts_with("audio/") { crate::state::AttachmentKind::Audio }
    else if lower.starts_with("video/") { crate::state::AttachmentKind::Video }
    else { crate::state::AttachmentKind::Image }
}

/// File extension (no leading dot, case-insensitive) → attachment kind.
pub fn ext_to_kind(ext: &str) -> crate::state::AttachmentKind {
    match ext.to_ascii_lowercase().as_str() {
        "mp3" | "ogg" | "wav" | "flac" | "aac" => crate::state::AttachmentKind::Audio,
        "mp4" | "webm" | "mov" | "avi" | "mkv" => crate::state::AttachmentKind::Video,
        _ => crate::state::AttachmentKind::Image,
    }
}

/// MIME content-type for a Storage upload of the given extension.
pub fn mime_for_ext(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        _ => "application/octet-stream",
    }
}
