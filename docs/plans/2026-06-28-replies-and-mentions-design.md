# VoxLink Replies + Mentions Design

> **Status:** Validated 2026-06-28. Canonical for these features.
> Builds on `2026-06-27-message-contract.md` (edited by this design — see §6).

## 1. Scope

Three Discord-parity per-message features:

- **Reactions** — already fully implemented on both clients (native: `render_reactions` + context menu in `components.rs`, `insert_reaction`/`delete_reaction`/`fetch_reactions` in `supabase.rs`, `REACTION` broadcast + `ReactionUpdate`/`SendReaction`; web: `toggleReaction`/`showReactionPicker`/`onReaction` + DB hydration in `fetchHistory`). **Not redesigned here.** This design does, however, fix a latent bug that prevented reacting to *live-arrived* messages (see §2).
- **Replies** — Discord-style reply reference. Contract + DB + both clients.
- **Mentions / pings** — `@username` highlight + notification sound. **Client-side only; no contract/DB change.**

## 2. Foundational prerequisite — universal message UUID plumbing

The current code discards the DB UUID of a just-sent message and never propagates a live-arrived message's UUID to the receiver. This blocks replies (and silently drops reactions) on any message that arrived via live broadcast rather than history fetch — i.e. the common case. Verified gaps:

1. **Sender discards the UUID.** `insert_message` (`src/net/supabase.rs:406`) uses `Prefer: return=minimal` and returns `Result<()>`; the generated UUID is thrown away. Web `trySend` (`web/app.js:822`) calls `sb.from('messages').insert({...})` without `.select()`. Result: just-sent own messages keep `db_id: None` (`src/state.rs:191`, `web/app.js:877` `if (dbId)` guard) until a history refresh.
2. **The wire payload carries no message id.** `chat_message` is `{from, content}` (contract §4.2; `src/net/signaling.rs:171-174`); `chat_media` is `{from, content, url, kind, filename}` (§4.3; `src/net/signaling.rs:179-185`). `NetEvent::MessageReceived` (`src/state.rs:310`) has no id field. So a *received* live message also lands with `db_id: None`.
3. **Latent reaction bug (both clients).** `ReactionUpdate` resolves by `m.db_id == Some(message_id)` (`src/app.rs:305`); web `onReaction` queries `[data-msg-id="${message_id}"]` (`web/app.js:540`). A just-arrived live message matches neither → the reaction is **silently dropped**. Same root cause as #2.

### Fix (Task 0 — both clients) — generate the UUID client-side

**Key sequencing constraint.** The current send order is *broadcast → optimistic display → DB insert* (native `src/ui/chat.rs:754-759`, web `web/app.js:819-822`). The broadcast fires **before** the insert, so a DB-generated UUID is not yet known when the payload is built. We therefore **generate the message UUID client-side** and use the same value for the broadcast `message_id`, the optimistic message's `db_id`, and the DB insert. This makes `db_id` known synchronously before `push_own`/`appendMsg` — no "thread the UUID back" plumbing, no extra round-trip, and peers see the message as fast as today.

- **UUID source.** Native: the `uuid` crate (`uuid = { version = "1", features = ["v4"] }`, already a dep at `Cargo.toml:48`) → `Uuid::new_v4().to_string()`. Web: `crypto.randomUUID()`.
- **DB.** The `messages.id` column is `DEFAULT gen_random_uuid()` but accepts an explicit UUID on insert — **no schema change**. `insert_message` (`src/net/supabase.rs:380`) now sends an explicit `"id"` field in the insert body and may keep `Prefer: return=minimal` (the client already knows the id). Web `trySend`/`uploadMedia` include `id` in the `sb.from('messages').insert({...})` body.
- **Optimistic display.** The UUID is generated first, then threaded into the broadcast `message_id`, the optimistic own message's `db_id` (native: pass into `push_own_media`; web: pass into `appendMsg(..., dbId)`), and the DB insert — all using one value.

**Wire.** Add `message_id: String` to the `chat_message` (§4.2) and `chat_media` (§4.3) broadcast payloads. The sender includes the client-generated UUID.

**Receiver.** `handle_incoming` reads `message_id`; `NetEvent::MessageReceived` carries it; `apply_net_event` sets the live peer message's `db_id = Some(message_id)` (native); web `onChatMessage`/`onChatMedia` pass `dbId` to `appendMsg` so `dataset.msgId` is set on the live row.

**Effect.** Every message — own or peer, live or history — has a stable DB UUID in the client the moment it appears, with no async round-trip on the send path. Replies can reference any visible message; live reactions resolve on both clients (bug #3 fixed as a side effect).

## 3. Replies

### 3.1 Database

Three new nullable columns on `messages`:

```sql
ALTER TABLE messages ADD COLUMN reply_to_id      UUID REFERENCES messages(id) ON DELETE SET NULL;
ALTER TABLE messages ADD COLUMN reply_to_author  TEXT;
ALTER TABLE messages ADD COLUMN reply_to_content TEXT;   -- sender truncates to 100 chars
```

Storing the snippet on the row (not just the FK) means history fetch stays a flat `SELECT` — no self-join, no PostgREST relationship naming. VoxLink has no edit/delete today, so snippet staleness is moot. Existing `msg_read`/`msg_insert` RLS policies cover the new columns.

### 3.2 Wire contract

Edit `docs/plans/2026-06-27-message-contract.md` §3 (table) and §4.2/§4.3 (payloads), then mirror to `src/net/contract.rs` and `web/contract.js`:

- `chat_message` payload becomes `{ from, content, message_id, reply_to, reply_to_author, reply_to_content }`.
- `chat_media` payload becomes `{ from, content, url, kind, filename, message_id, reply_to, reply_to_author, reply_to_content }`.
- `reply_to`/`reply_to_author`/`reply_to_content` are `String|null`; `Option<String>` serializes to JSON `null`, receivers treat null and missing-key identically (existing §4 convention).
- No new event constants — same `chat_message`/`chat_media` events, extended payloads.

### 3.3 Rust state (`src/state.rs`)

- `ChatMessage` gains `reply_to`, `reply_to_author`, `reply_to_content` (all `Option<String>`, `#[serde(default)]`).
- New `ReplyTarget { db_id: String, author: String, content: String }` (content truncated to 100 chars at capture time).
- `AppState.reply_target: Option<ReplyTarget>` — the in-progress reply being composed. Cleared on send or ESC.
- `UiCommand::SendMessage(String)` → `SendMessage { content: String, reply: Option<ReplyTarget> }`; `SendMedia` gains `reply: Option<ReplyTarget>`.

### 3.4 Rust net

- `supabase.rs`: `DbMessage` + `insert_message` (now sends an explicit client-generated `id` in the body; still `Result<()>` fire-and-forget) + `fetch_recent_messages` carry the three reply fields (direct select, no join).
- `signaling.rs`: `SigCmd::BroadcastMessage` / `BroadcastMedia` carry reply fields + `message_id`; `handle_incoming` reads them into `NetEvent::MessageReceived`.
- `NetEvent::MessageReceived` gains `message_id: String` + the three reply fields.
- `webrtc.rs`: `UiCommand` → `SigCmd` dispatch passes reply fields + `message_id` through.

### 3.5 Native UI

- `ui/components.rs`: add "Reply" to the existing right-click context menu (`content_resp.context_menu(...)` already present). Click → sets `state.reply_target` and returns focus to the input.
- `ui/chat.rs`: when `state.reply_target` is `Some`, render a reply preview bar above the input bar (author + truncated content + ✕ cancel). ESC clears. `try_send_message` includes reply fields in the broadcast + DB insert, then clears `reply_target`.
- `ui/components.rs` `render_chat_message`: if `msg.reply_to` is `Some`, render a compact `↪ author: snippet` line above the body in muted text; clickable to scroll to the parent if present locally (match by `db_id`).
- `app.rs` `apply_net_event`: stamp `db_id` + reply fields onto live-arrived messages.

### 3.6 Web UI

- `web/app.js`: hover "Reply" button on `.msg-group` → sets `pendingReply = { dbId, author, content }` + shows a reply preview bar above `#message-input`.
- `appendMsg` (app.js:859) accepts `replyTo`/`replyToAuthor`/`replyToContent` and renders a `.msg-reply-ref` element above `.msg-content`.
- `trySend` (app.js:811) / `uploadMedia` include reply fields + `message_id` in `bcast` and `sb.from('messages').insert(...)`.
- `onChatMessage`/`onChatMedia` (app.js:480-488) read `message_id` + reply fields, pass to `appendMsg`.
- `web/style.css`: `.msg-reply-ref` styling (muted, smaller, left border accent).

## 4. Mentions / pings (client-side only)

**No contract/DB change.** Parse `@username` from content at render time.

- **Validation:** `@<token>` is a mention iff `token` matches `self.username` or any peer in `peers`. Unmatched `@foo` renders as plain text.
- **Highlight:** matched `@username` rendered in accent/blurple + subtle background, both clients. Native: split content into segments and render mentions as styled `RichText` spans in `render_chat_message`. Web: wrap matched mentions in `<span class="mention">` during `appendMsg` content rendering.
- **Sound:** on receive of `chat_message`/`chat_media` whose content contains `@<local_username>`, play a notification sound.
  - Native: bundled short public-domain WAV (~1-2KB) played via a new `audio::play_notification()` helper on the existing cpal stack. Asset bundled via `include_bytes!`.
  - Web: `new Audio('notification.mp3').play()` with a user-gesture unlock fallback (browsers block autoplay until first interaction; unlock on first `pointerdown`/`keydown`).
- **No unread badge, no push, no new broadcast event** — per user decision (highlight + sound only).

## 5. Files touched

**Canonical (edit first):** `docs/plans/2026-06-27-message-contract.md` (§3 table, §4.2/§4.3 payloads).

**Mirrored contracts:** `src/net/contract.rs`, `web/contract.js`.

**Native:** `src/state.rs`, `src/app.rs`, `src/net/{supabase.rs,signaling.rs,webrtc.rs}`, `src/ui/{chat.rs,components.rs}`, `src/audio/` (new `play_notification` + bundled WAV asset).

**Web:** `web/app.js`, `web/style.css`, `web/notification.mp3` (new asset).

**SQL (run once in Supabase editor):** the three `ALTER TABLE messages ADD COLUMN` statements in §3.1.

## 6. Verification

- `cargo build` native compile clean.
- Two-client E2E (native ↔ native and native ↔ web):
  - Reply to a *live-arrived* message renders the quoted reference on both clients; "jump to parent" works when the parent is in the local window.
  - Reply to a message that fell out of the 100-msg history window still renders the snippet (denormalized), with `reply_to_id` set.
  - React to a *live-arrived* message works on both clients (previously silently dropped — bug #3 fixed).
  - `@mention` highlights in accent and plays the notification sound on the receiver; unmatched `@foo` renders as plain text.
  - Cross-target interop: a native-sent reply + mention renders correctly on web, and vice-versa.

## 7. Out of scope

- Reply threading / nested replies (flat replies only; `reply_to_id` is always the immediate parent).
- Message edit/delete (does not exist; snippet staleness is moot).
- Unread/mentions badge, push notifications, offline mention delivery.
- Migrating presence to the Realtime Presence API (separate spec, §9 of the message contract).
