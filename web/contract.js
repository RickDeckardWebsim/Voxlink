// ─────────────────────────────────────────────────────────────────────────────
// VoxLink cross-target contract (web side)
// Canonical spec: ../docs/plans/2026-06-27-message-contract.md
// Mirrored by src/net/contract.rs. Keep in sync.
// ─────────────────────────────────────────────────────────────────────────────

export const SUPABASE_URL      = 'https://syftqwloslmnjyvppler.supabase.co';
export const SUPABASE_ANON_KEY = 'sb_publishable_VK3kO0lX4tTsrHlCsH6JFQ_ebB6_lMH';
export const SIGNALING_CHANNEL = 'p2p-signaling';
export const DEFAULT_DB_CHANNEL = 'general';
// Realtime broadcast event names. Canonical wire strings (spec §4).
// `peer_leave` is receive-only today (spec §8); included for completeness.
export const EVENTS = Object.freeze({
  PEER_JOIN:      'peer_join',
  PEER_LEAVE:     'peer_leave',
  CHAT_MESSAGE:   'chat_message',
  CHAT_MEDIA:     'chat_media',
  VOICE_STATE:    'voice_state',
  PROFILE_UPDATE: 'profile_update',
  SDP_OFFER:      'sdp_offer',
  SDP_ANSWER:     'sdp_answer',
});

// MIME → attachment kind. Spec §6. Anything not audio/* or video/* → 'image'.
export function mimeToKind(mime) {
  const m = (mime || '').toLowerCase();
  if (m.startsWith('audio/')) return 'audio';
  if (m.startsWith('video/')) return 'video';
  return 'image';
}

// File extension (no leading dot, case-insensitive) → attachment kind. Spec §6.
export function extToKind(ext) {
  const e = (ext || '').toLowerCase();
  if (['mp3', 'ogg', 'wav', 'flac', 'aac'].includes(e)) return 'audio';
  if (['mp4', 'webm', 'mov', 'avi', 'mkv'].includes(e)) return 'video';
  return 'image';
}
