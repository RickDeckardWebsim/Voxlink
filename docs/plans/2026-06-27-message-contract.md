# VoxLink Web↔Standalone Message & Event Contract

> **Status:** Canonical. Both the native Rust client (`src/`) and the web client (`web/app.js`) MUST conform to every definition here. A behavior change to any shared concept MUST be edited here first, then mirrored in `src/net/contract.rs` and `web/contract.js`.

## 1. Purpose

VoxLink ships as two clients over one Supabase backend: a native Rust/egui app (Windows now, Linux in flight) and a no-build-step vanilla-JS web client (Chromebook target). Both render the same chat and run the same Realtime signaling. To keep a 1-1 featureset across targets, every shared concept — constants, the `messages` table, every Realtime broadcast event, attachment kind values, and the MIME/extension → kind table — has exactly one definition here.

## 2. Shared constants

| Constant | Value | Defined in |
|---|---|---|
| `SUPABASE_URL` | `https://syftqwloslmnjyvppler.supabase.co` | `contract.rs`, `contract.js` |
| `SUPABASE_ANON_KEY` | `sb_publishable_VK3kO0lX4tTsrHlCsH6JFQ_ebB6_lMH` | `contract.rs`, `contract.js` |
| `SIGNALING_CHANNEL` | `p2p-signaling` | `contract.rs`, `contract.js` |
| `DEFAULT_DB_CHANNEL` | `general` | `contract.rs`, `contract.js` |

The Supabase Realtime topic string is `realtime:${SIGNALING_CHANNEL}`. The Phoenix channel name (web `sb.channel(CHANNEL_NAME, ...)`) equals `SIGNALING_CHANNEL` exactly — Supabase's JS SDK prepends `realtime:` internally, matching the native client's explicit `realtime:` prefix.

## 3. `messages` table (PostgREST persistence)

```sql
CREATE TABLE IF NOT EXISTS messages (
  id                  UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
  channel             TEXT        NOT NULL DEFAULT 'general',
  from_user           TEXT        NOT NULL,
  content             TEXT        NOT NULL DEFAULT '',
  attachment_url      TEXT,
  attachment_kind     TEXT,
  attachment_filename TEXT,
  reply_to_id         UUID        REFERENCES messages(id) ON DELETE SET NULL,
  reply_to_author     TEXT,
  reply_to_content    TEXT,
  created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
ALTER TABLE messages ENABLE ROW LEVEL SECURITY;
CREATE POLICY msg_read   ON messages FOR SELECT USING (true);
CREATE POLICY msg_insert ON messages FOR INSERT TO authenticated WITH CHECK (true);
CREATE INDEX messages_channel_time ON messages (channel, created_at DESC);
```
- **Note:** `id` accepts an explicit client-generated UUID on insert (no schema change — the DEFAULT only fires when `id` is omitted). `reply_to_*` are nullable; existing `msg_read`/`msg_insert` RLS covers them.

- Insert path: `POST {SUPABASE_URL}/rest/v1/messages` with `apikey` + `Authorization: Bearer <access_token>` + `Prefer: return=minimal`. Body always includes `channel` (= `DEFAULT_DB_CHANNEL`), `from_user`, `content`. Attachment columns set only when an attachment is present.
- Fetch path: `GET {SUPABASE_URL}/rest/v1/messages?channel=eq.general&order=created_at.desc&limit=100`. Reverse client-side to chronological. History messages are non-expiring (use `unix_ts = 0`); system messages expire after 2 h.

## 4. Realtime broadcast events

All events flow over the `realtime:${SIGNALING_CHANNEL}` topic. The wire envelope (native) is:
```json
{ "topic": "realtime:p2p-signaling", "event": "broadcast",
  "payload": { "type": "broadcast", "event": "<EVENT>", "payload": <PAYLOAD> },
  "ref": "<N>", "join_ref": "1" }
```
Web clients use `sigChannel.send({ type: 'broadcast', event: '<EVENT>', payload: <PAYLOAD> })`; Supabase's SDK wraps it in the same envelope. **`self` is `false` on web, `true` on native** — the native client filters its own broadcasts in `handle_incoming` (`if from == username { return; }`).

`from` is always the sender's display username (string). `Option<String>` fields serialize as JSON `null` when absent; receivers MUST treat `null` and missing-key identically (use `.unwrap_or_default()` / `?? null`).

### 4.1 `peer_join`
**Payload:** `{ "from": String, "avatar_url": String|null, "description": String|null }`
- Send: on subscribe success, and in response to another peer's join (mutual-announce). Web also uses it to re-announce on reconnect.
- Recv: add/update peer in sidebar; system msg "<from> joined the room."
- **Known gap:** no `peer_leave` is ever *sent* by either client (see §6). Leave is inferred from Realtime disconnect.

### 4.2 `chat_message`
**Payload:** `{ "from": String, "content": String, "message_id": String, "reply_to": String|null, "reply_to_author": String|null, "reply_to_content": String|null }`

`message_id` is the client-generated UUID (native `Uuid::new_v4()`, web `crypto.randomUUID()`); the same value is sent in the DB insert's `id` field. `reply_to` is the parent message's UUID, or `null` for a non-reply. `reply_to_author`/`reply_to_content` are a denormalized snippet (content truncated to 100 chars by the sender) so receivers can render the reply reference without resolving the parent. Receivers treat `null` and missing-key identically.
- Send: optimistic local display, then broadcast, then fire-and-forget DB insert (`attachment_*` columns omitted).
- Recv: render as peer message with `attachment: None`.

### 4.3 `chat_media`
**Payload:** `{ "from": String, "content": String, "url": String, "kind": "image"|"audio"|"video", "filename": String, "message_id": String, "reply_to": String|null, "reply_to_author": String|null, "reply_to_content": String|null }`
- Send: after a successful Storage upload to `avatars/chat/{user_id}/{ts}-{safe_name}`, broadcast with the public URL, then DB insert including `attachment_url`/`attachment_kind`/`attachment_filename`.
- Recv: if `url` is empty, treat as a plain text message; else construct an `Attachment { url, kind, filename }` and render inline (image) or as a chip with "Open" (audio/video).
- `content` is the user-typed caption, not the filename. May be `""`.

### 4.4 `voice_state`
**Payload:** `{ "from": String, "speaking": bool, "muted": bool, "in_voice": bool }`
- Send: on join voice (`in_voice:true`), on leave voice (`in_voice:false`), on mute toggle, and on speaking→silent transitions (driven by RMS detection).
- Recv: update `PeerInfo.{is_speaking,is_muted,in_voice}`. Ignore if `from == self` on web; native applies own-state echoes to `AppState`.

### 4.5 `profile_update`
**Payload:** `{ "from": String /* old username */, "new_username": String, "avatar_url": String|null, "description": String|null }`
- Send: after a successful `profiles` row update (username and/or description) or avatar upload.
- Recv: update peer's username (re-key the peer map if changed), avatar, description. Ignore if `from == self`.

### 4.6 `sdp_offer`
**Payload:** `{ "from": String, "to": String, "sdp": String }`
- Send: the lower-username peer (lexicographic `<`) initiates the call after both are in voice. `sdp` is the SDP body string.
- Recv: only act if `to == self`; set remote description, create+send `sdp_answer`.

### 4.7 `sdp_answer`
**Payload:** `{ "from": String, "to": String, "sdp": String }`
- Send: in response to an `sdp_offer` addressed to self.
- Recv: only act if `to == self` and local state is `have-local-offer`; set remote description.

### 4.8 `peer_leave` (receive-only — see §6)
**Payload:** `{ "from": String }` — defined for forward compatibility. Neither client currently sends it.

### 4.9 Client-generated message id
Both clients generate the message UUID locally (native `Uuid::new_v4()`, web `crypto.randomUUID()`) before broadcast and use it for: (a) the `message_id` field in `chat_message`/`chat_media`, (b) the optimistic message's `db_id` / `dataset.msgId`, (c) the explicit `id` in the `messages` insert. This is required because the send order is broadcast → DB insert, so a DB-generated UUID is not yet known at broadcast time. The `messages.id DEFAULT gen_random_uuid()` only fires when `id` is omitted; both clients now always send it.

## 5. Attachment kind

Three values, lowercase strings on the wire and in the DB:

| Wire/DB string | Rust enum variant | Web string |
|---|---|---|
| `"image"` | `AttachmentKind::Image` | `'image'` |
| `"audio"` | `AttachmentKind::Audio` | `'audio'` |
| `"video"` | `AttachmentKind::Video` | `'video'` |

Rust `AttachmentKind` exposes `as_str() -> &'static str` and `from_str(&str) -> Self` (in `state.rs`); all serialization sites call these instead of open-coded `match`. Web uses the raw string.

**Name-collision warning:** Rust `ChatMessage.kind` is `MessageKind { Own, Peer, System }` — a *render* classification, unrelated to attachment kind. Do not confuse the two. The spec reserves "kind" for attachment kind on the wire; `ChatMessage.kind` is a UI-only field never sent.

## 6. MIME / extension → kind mapping (single source)

One table, mirrored in `contract.rs::ext_to_kind` / `mime_to_kind` and `contract.js::mimeToKind` / `extToKind`. Anything not listed defaults to `image`.

| Extension (lowercase) | MIME | Kind |
|---|---|---|
| png, jpg, jpeg, gif, webp, bmp | image/png, image/jpeg, image/gif, image/webp, image/bmp | image |
| mp3, ogg, wav, flac, aac | audio/mpeg, audio/ogg, audio/wav, application/octet-stream, application/octet-stream | audio |
| mp4, webm, mov, avi, mkv | video/mp4, video/webm, video/quicktime, application/octet-stream, application/octet-stream | video |

Web `kindForMime` keys off the MIME prefix (`audio/` → audio, `video/` → video, else image); it MUST produce the same result as `mime_to_kind` for any MIME the file picker accepts.

## 7. Storage

- Bucket: `avatars` (public). Shared by profile avatars and chat media.
- Avatar object path: `{user_id}_avatar.{ext}` (upserted; returned URL gets `?t=<unix>` cache-buster).
- Media object path: `chat/{user_id}/{ts}-{safe_name}` where `safe_name` filters to `[A-Za-z0-9._-]` and truncates to 40 chars.
- Public URL format: `{SUPABASE_URL}/storage/v1/object/public/avatars/{path}`.

## 8. Known cross-target gaps

1. **`peer_leave` is sent on graceful exit only.** Both targets now broadcast `peer_leave` before intentional departure (native: `on_exit` → `UiCommand::Disconnect` → `SigCmd::BroadcastPeerLeave` then `Disconnect`; web: `cleanup()` before `unsubscribe`, plus a `beforeunload` listener for tab/window close). **Crash / kill / network loss is NOT covered** — those still rely on Supabase Realtime's server-side presence timeout (~10-45s). A full fix would migrate presence to the Realtime Presence API (see §9).
2. **`self` flag differs.** Native subscribes with `self:true` and filters own messages in `handle_incoming`; web subscribes with `self:false`. Both end up ignoring own broadcasts; the difference is intentional and benign.
3. **WebRTC implementation diverges by necessity.** Web uses the browser `RTCPeerConnection`; native uses `str0m`. The signaling payloads (§4.6/4.7) are identical, so the two interoperate. Not a contract concern.

## 9. Future: migrate presence to Realtime Presence API

The current `peer_join`/`peer_leave` broadcast model handles graceful joins/leaves but cannot detect crashes or network loss promptly. Supabase Realtime's built-in Presence API (`channel.on('presence', { event: 'join'|'leave' }, ...)` on web; equivalent Phoenix presence tracking on native) handles departure authoritatively via server heartbeat timeout. Migrating would replace the custom presence broadcasts with track/untrack calls and presence-event handlers on both targets, and would let the `peer_leave` recv arms consume presence-leave events instead of broadcast events. This is a signaling-layer refactor scoped separately from the message contract.
