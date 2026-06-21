// ─────────────────────────────────────────────────────────────────────────────
// net/mod.rs — Networking module
// ─────────────────────────────────────────────────────────────────────────────

// Signaling client — native only (Wasm will substitute browser WebSocket APIs)
#[cfg(not(target_arch = "wasm32"))]
pub mod signaling;

// Phase 3: WebRTC peer management via str0m
#[cfg(not(target_arch = "wasm32"))]
pub mod webrtc;

// Supabase REST APIs (Auth, Profiles, Storage)
pub mod supabase;

pub mod updater;
