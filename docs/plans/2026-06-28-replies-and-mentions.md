# VoxLink Replies + Mentions Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:executing-plans to implement this plan task-by-task.

**Goal:** Add Discord-style per-message replies, @mention pings (highlight + sound), and make reactions work on live-arrived messages — across the native Rust/egui client and the vanilla-JS web client, over one Supabase backend governed by a canonical contract.

**Architecture:** A dual-client app (native Rust/egui + web JS) over one Supabase backend. A canonical contract doc (`docs/plans/2026-06-27-message-contract.md`) is edited first, then mirrored to `src/net/contract.rs` and `web/contract.js`. Replies add three nullable columns to `messages` and three optional fields to the `chat_message`/`chat_media` payloads. The foundational prerequisite (Task 0) gives every message a stable client-generated UUID the moment it appears — fixing the latent live-reaction bug and making live replies referenceable. Mentions are pure client-side `@username` parsing with a notification sound; no contract/DB change.

**Tech Stack:** Rust + egui 0.34 + eframe; reqwest (Supabase REST); tokio-tungstenite (Realtime); `uuid` v1 (already a dep); cpal (audio). Web: vanilla JS, Supabase-JS, `crypto.randomUUID()`, Web Audio.

**Design doc:** `docs/plans/2026-06-28-replies-and-mentions-design.md` (committed at `69b5823`).

**Verification model:** This codebase has no unit-test harness — it is an immediate-mode GUI app against a live Supabase project. Each task's verification is `cargo build` (compile clean) plus a manual E2E step run against two real clients (native ↔ native, and native ↔ web) per §6 of the design. Do not invent mocks. Run `cargo build` after every task that touches Rust; open `web/index.html` (served however the repo currently serves it) for web checks.

---

## Task 0: Canonical contract — add `message_id` + reply fields

**Files:**
- Modify: `docs/plans/2026-06-27-message-contract.md` (§3 table, §4.2, §4.3)

**Step 1: Edit §3 `messages` table**

Add three columns to the `CREATE TABLE` block in §3 so the canonical schema matches the design's §3.1:

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
```

Note under the table: `id` accepts an explicit client-generated UUID on insert (no schema change — the `DEFAULT` only fires when `id` is omitted). `reply_to_*` are nullable; existing `msg_read`/`msg_insert` RLS covers them.

**Step 2: Edit §4.2 `chat_message` payload**

Change the payload from `{ "from": String, "content": String }` to:

```
{ "from": String, "content": String, "message_id": String, "reply_to": String|null, "reply_to_author": String|null, "reply_to_content": String|null }
```

Add: "`message_id` is the client-generated UUID (native `Uuid::new_v4()`, web `crypto.randomUUID()`); the same value is sent in the DB insert's `id` field. `reply_to` is the parent message's UUID, or `null` for a non-reply. `reply_to_author`/`reply_to_content` are a denormalized snippet (content truncated to 100 chars by the sender) so receivers can render the reply reference without resolving the parent. Receivers treat `null` and missing-key identically."

**Step 3: Edit §4.3 `chat_media` payload**

Add the same three reply fields + `message_id` to the `chat_media` payload:

```
{ "from": String, "content": String, "url": String, "kind": "image"|"audio"|"video", "filename": String, "message_id": String, "reply_to": String|null, "reply_to_author": String|null, "reply_to_content": String|null }
```

**Step 4: Add a §4.9 "Client-generated message id" note**

```
### 4.9 Client-generated message id
Both clients generate the message UUID locally (native `Uuid::new_v4()`, web `crypto.randomUUID()`) before broadcast and use it for: (a) the `message_id` field in `chat_message`/`chat_media`, (b) the optimistic message's `db_id` / `dataset.msgId`, (c) the explicit `id` in the `messages` insert. This is required because the send order is broadcast → DB insert, so a DB-generated UUID is not yet known at broadcast time. The `messages.id DEFAULT gen_random_uuid()` only fires when `id` is omitted; both clients now always send it.
```

**Step 5: Commit**

```bash
git add docs/plans/2026-06-27-message-contract.md
git commit -m "contract: add message_id + reply_to_* to chat_message/chat_media payloads"
```

---

## Task 1: Mirror contract to Rust + JS constants

**Files:**
- Modify: `src/net/contract.rs` (no new event constants needed, but add a doc comment pointing to the new payload fields)
- Modify: `web/contract.js` (same)

**Step 1: Rust — annotate the event module**

In `src/net/contract.rs` `pub mod event`, the `CHAT_MESSAGE` and `CHAT_MEDIA` constants stay the same (no new events). Add a doc comment above the `event` module:

```rust
/// Realtime broadcast event names. Canonical wire strings (spec §4).
/// `chat_message` and `chat_media` payloads now carry `message_id` and
/// `reply_to` / `reply_to_author` / `reply_to_content` (spec §4.2/§4.3/§4.9).
/// `peer_leave` is receive-only today (spec §8); included for completeness.
pub mod event {
```

**Step 2: JS — mirror the same note in `web/contract.js`**

Above the `export const EVENTS = ...` block, update the comment:

```js
// Realtime broadcast event names. Canonical wire strings (spec §4).
// chat_message and chat_media payloads now carry message_id and
// reply_to / reply_to_author / reply_to_content (spec §4.2/§4.3/§4.9).
// `peer_leave` is receive-only today (spec §8); included for completeness.
```

No constant values change.

**Step 3: Verify + commit**

```bash
cargo build
git add src/net/contract.rs web/contract.js
git commit -m "contract: mirror message_id + reply payload note to rust and js"
```

Expected: `cargo build` compiles clean (comment-only change).

---

## Task 2: Rust state — `ChatMessage` reply fields + `ReplyTarget` + `AppState.reply_target`

**Files:**
- Modify: `src/state.rs:150-173` (`ChatMessage`), `:299-345` (`NetEvent`, `UiCommand`), `:425-454` (`AppState`), `:541-580` (`push_*` helpers)

**Step 1: Add reply fields to `ChatMessage`**

At `src/state.rs:172`, after the `db_id` field, add (all `#[serde(default)]`):

```rust
    /// DB UUID of the message this is replying to (None = not a reply).
    #[serde(default)]
    pub reply_to: Option<String>,
    /// Denormalized author of the parent message (for rendering without a join).
    #[serde(default)]
    pub reply_to_author: Option<String>,
    /// Denormalized snippet of the parent message's content (≤100 chars).
    #[serde(default)]
    pub reply_to_content: Option<String>,
```

Update the three constructors (`new_own`, `new_peer`, `new_system` at `:176`, `:195`, `:214`) to initialize these three fields to `None`. For `new_own`/`new_peer`, add optional parameters so callers can pass reply data — see Step 4 below for the call-site updates.

**Step 2: Add `message_id` + reply fields to `NetEvent::MessageReceived`**

At `src/state.rs:310`, change:

```rust
    MessageReceived { from: String, content: String, attachment: Option<Attachment> },
```

to:

```rust
    MessageReceived {
        from: String,
        content: String,
        attachment: Option<Attachment>,
        message_id: String,
        reply_to: Option<String>,
        reply_to_author: Option<String>,
        reply_to_content: Option<String>,
    },
```

**Step 3: Add `reply` to `UiCommand::SendMessage` / `SendMedia` + define `ReplyTarget`**

At `src/state.rs:332-333`, change:

```rust
    SendMessage(String),
    SendMedia { caption: String, url: String, kind: AttachmentKind, filename: String },
```

to:

```rust
    SendMessage { content: String, reply: Option<ReplyTarget> },
    SendMedia { caption: String, url: String, kind: AttachmentKind, filename: String, reply: Option<ReplyTarget> },
```

Add the `ReplyTarget` struct near `ChatMessage` (e.g. after line 173):

```rust
/// In-progress reply being composed (set when the user clicks "Reply" on a message).
/// `content` is truncated to 100 chars at capture time for the denormalized snippet.
#[derive(Debug, Clone)]
pub struct ReplyTarget {
    pub db_id: String,
    pub author: String,
    pub content: String,
}
```

**Step 4: Add `reply_target` to `AppState` + update `push_*` helpers**

In `AppState` (around `src/state.rs:426-434`, the Chat section), add:

```rust
    /// The message currently being replied to (None = normal compose). Set by the
    /// "Reply" context-menu action; cleared on send or ESC.
    pub reply_target: Option<ReplyTarget>,
```

In `Default for AppState` (`:516-528`), initialize `reply_target: None,`.

Update `push_own_media` (`:554`) to accept reply fields and thread them into `ChatMessage::new_own`:

```rust
    pub fn push_own_media(
        &mut self,
        content: impl Into<String>,
        attachment: Option<Attachment>,
        reply: Option<&ReplyTarget>,
    ) {
        let id = self.next_id();
        let author = self.username.clone();
        let mut msg = ChatMessage::new_own(author, content, id, attachment);
        if let Some(r) = reply {
            msg.reply_to = Some(r.db_id.clone());
            msg.reply_to_author = Some(r.author.clone());
            msg.reply_to_content = Some(r.content.clone());
        }
        self.messages.push(msg);
        self.scroll_to_bottom = true;
    }
```

Update `push_own` (`:550`) to pass `None`. Update `push_peer_media` (`:565`) similarly — it needs to accept `message_id` + reply fields coming from `NetEvent::MessageReceived`:

```rust
    pub fn push_peer_media(
        &mut self,
        author: impl Into<String>,
        content: impl Into<String>,
        attachment: Option<Attachment>,
        message_id: String,
        reply: Option<(String, String, String)>, // (reply_to, reply_to_author, reply_to_content)
    ) {
        let id = self.next_id();
        let mut msg = ChatMessage::new_peer(author, content, id, attachment);
        msg.db_id = Some(message_id);
        if let Some((to, author, content)) = reply {
            msg.reply_to = Some(to);
            msg.reply_to_author = Some(author);
            msg.reply_to_content = Some(content);
        }
        self.messages.push(msg);
        self.scroll_to_bottom = true;
    }
```

Update `push_peer` (`:561`) to pass `String::new()` and `None` (it's only used for legacy system-echo paths if any; grep and fix all call sites).

**Step 5: Grep for all `push_own`/`push_peer`/`push_own_media`/`push_peer_media` call sites and fix signatures**

Run: `grep -n "push_own\|push_peer" src/` — fix every caller (notably `src/ui/chat.rs:655` `push_own_media`, `:759` `push_own`, and any in `app.rs`). Pass `None` / `String::new()` where reply data isn't available yet; the real reply wiring lands in Task 6.

**Step 6: Verify + commit**

```bash
cargo build
```

Expected: compile clean (all call sites updated). Then:

```bash
git add src/state.rs
git commit -m "state: add ChatMessage reply fields, ReplyTarget, reply_target state, NetEvent/UiCommand reply + message_id"
```

---

## Task 3: Rust supabase — explicit `id` + reply columns on insert/fetch

**Files:**
- Modify: `src/net/supabase.rs:367-415` (`DbMessage`, `insert_message`), `:419-473` (`fetch_recent_messages`)

**Step 1: Add reply fields to `DbMessage`**

At `src/net/supabase.rs:367-376`, add:

```rust
#[derive(Deserialize)]
struct DbMessage {
    id:                  String,
    from_user:           String,
    content:             String,
    attachment_url:      Option<String>,
    attachment_kind:     Option<String>,
    attachment_filename: Option<String>,
    reply_to_id:         Option<String>,
    reply_to_author:     Option<String>,
    reply_to_content:    Option<String>,
    created_at:          String,
}
```

**Step 2: `insert_message` — accept explicit `id` + reply fields**

Change the signature at `:380` to:

```rust
pub fn insert_message(
    access_token: &str,
    from_user: &str,
    content: &str,
    attachment: Option<&crate::state::Attachment>,
    id: &str,
    reply: Option<(&str, &str, &str)>, // (reply_to_id, reply_to_author, reply_to_content)
) -> Result<()> {
```

In the body (`:389-400`), add `id` and reply fields:

```rust
    let mut body = json!({
        "id":        id,
        "channel":   contract::DEFAULT_DB_CHANNEL,
        "from_user": from_user,
        "content":   content,
    });

    if let Some(att) = attachment {
        let kind_str = att.kind.as_str();
        body["attachment_url"]      = json!(att.url);
        body["attachment_kind"]     = json!(kind_str);
        body["attachment_filename"] = json!(att.filename);
    }

    if let Some((to_id, to_author, to_content)) = reply {
        body["reply_to_id"]      = json!(to_id);
        body["reply_to_author"]  = json!(to_author);
        body["reply_to_content"] = json!(to_content);
    }
```

Keep `Prefer: return=minimal` — the client already knows the id.

**Step 3: `fetch_recent_messages` — select + populate reply fields**

Update the `select=` query at `:425` to include the new columns:

```rust
    let url = format!(
        "{}/rest/v1/messages?select=id,from_user,content,attachment_url,attachment_kind,attachment_filename,reply_to_id,reply_to_author,reply_to_content,created_at&channel=eq.{}&order=created_at.desc&limit=100",
        contract::SUPABASE_URL,
        contract::DEFAULT_DB_CHANNEL
    );
```

In the row→`ChatMessage` mapping (`:444-470`), set the reply fields:

```rust
        crate::state::ChatMessage {
            id:         0,
            author:     row.from_user,
            content:    row.content,
            timestamp:  iso_to_hhmm(&row.created_at),
            kind,
            attachment,
            unix_ts:    0,
            reactions:  Vec::new(),
            db_id:      Some(row.id),
            reply_to:         row.reply_to_id,
            reply_to_author:  row.reply_to_author,
            reply_to_content: row.reply_to_content,
        }
```

**Step 4: Fix `insert_message` call sites**

Run: `grep -n "insert_message" src/` — two callers: `src/ui/chat.rs:766` (text) and `:672` (media). Pass the client-generated `id` (generated in Task 6's `try_send_message`/`pick_and_upload_media` rewrite) and `None` for reply for now (reply wiring in Task 6). For this task, pass placeholder `Uuid::new_v4().to_string()` and `None` so it compiles; Task 6 replaces the placeholder with the real shared UUID.

Add `use uuid::Uuid;` at the top of `chat.rs` if not present.

**Step 5: Verify + commit**

```bash
cargo build
```

Expected: clean. Then:

```bash
git add src/net/supabase.rs src/ui/chat.rs
git commit -m "supabase: explicit id + reply_to_* on insert and fetch"
```

---

## Task 4: Rust signaling — `message_id` + reply on `BroadcastMessage`/`BroadcastMedia` + `handle_incoming`

**Files:**
- Modify: `src/net/signaling.rs:14-31` (`SigCmd`), `:169-187` (broadcast arms), `:282-333` (`handle_incoming` chat arms)

**Step 1: Extend `SigCmd::BroadcastMessage` / `BroadcastMedia`**

At `src/net/signaling.rs:18-19`, change:

```rust
    BroadcastMessage(String),
    BroadcastMedia { caption: String, url: String, kind: String, filename: String },
```

to:

```rust
    BroadcastMessage { content: String, message_id: String, reply: Option<(String, String, String)> },
    BroadcastMedia {
        caption: String, url: String, kind: String, filename: String,
        message_id: String, reply: Option<(String, String, String)>,
    },
```

**Step 2: Update the broadcast send arms (`:169-187`)**

```rust
                        SigCmd::BroadcastMessage { content, message_id, reply } => {
                            let topic = crate::net::contract::SIGNALING_TOPIC.to_string();
                            let mut payload = json!({
                                "from":        username,
                                "content":     content,
                                "message_id":  message_id,
                            });
                            if let Some((to, author, snippet)) = reply {
                                payload["reply_to"]         = json!(to);
                                payload["reply_to_author"]  = json!(author);
                                payload["reply_to_content"] = json!(snippet);
                            }
                            let broadcast = make_broadcast(&topic, crate::net::contract::event::CHAT_MESSAGE, payload, &mut ref_count);
                            send_text(&mut ws_stream, &broadcast).await?;
                        }
                        SigCmd::BroadcastMedia { caption, url, kind, filename, message_id, reply } => {
                            let topic = crate::net::contract::SIGNALING_TOPIC.to_string();
                            let mut payload = json!({
                                "from":        username,
                                "content":     caption,
                                "url":         url,
                                "kind":        kind,
                                "filename":    filename,
                                "message_id":  message_id,
                            });
                            if let Some((to, author, snippet)) = reply {
                                payload["reply_to"]         = json!(to);
                                payload["reply_to_author"]  = json!(author);
                                payload["reply_to_content"] = json!(snippet);
                            }
                            let broadcast = make_broadcast(&topic, crate::net::contract::event::CHAT_MEDIA, payload, &mut ref_count);
                            send_text(&mut ws_stream, &broadcast).await?;
                        }
```

**Step 3: Update `handle_incoming` chat arms (`:282-333`)**

```rust
                crate::net::contract::event::CHAT_MESSAGE => {
                    if let Some(content) = b_payload["content"].as_str() {
                        let message_id = b_payload["message_id"].as_str().unwrap_or("").to_string();
                        let reply_to         = b_payload["reply_to"].as_str().map(str::to_owned);
                        let reply_to_author  = b_payload["reply_to_author"].as_str().map(str::to_owned);
                        let reply_to_content = b_payload["reply_to_content"].as_str().map(str::to_owned);
                        let reply = match (reply_to, reply_to_author, reply_to_content) {
                            (Some(to), Some(a), Some(c)) => Some((to, a, c)),
                            _ => None,
                        };
                        let _ = net_tx.send(NetEvent::MessageReceived {
                            from: from.to_string(),
                            content: content.to_string(),
                            attachment: None,
                            message_id,
                            reply_to:         reply.as_ref().map(|r| r.0.clone()),
                            reply_to_author:  reply.as_ref().map(|r| r.1.clone()),
                            reply_to_content: reply.as_ref().map(|r| r.2.clone()),
                        });
                        ctx.request_repaint();
                    }
                }
```

Do the equivalent for `CHAT_MEDIA` (`:317-333`): read `message_id` + reply fields, pass into `NetEvent::MessageReceived` with the constructed `attachment`.

**Step 4: Verify + commit**

```bash
cargo build
```

Expected: clean (call sites in `webrtc.rs` still pass old shapes — fix in Task 5). If `webrtc.rs` dispatch breaks the build, fix the two dispatch arms now to pass `message_id: String::new()` and `reply: None` placeholders; Task 5 wires the real values.

```bash
git add src/net/signaling.rs src/net/webrtc.rs
git commit -m "signaling: carry message_id + reply on chat_message/chat_media"
```

---

## Task 5: Rust webrtc — `UiCommand` → `SigCmd` dispatch for new shapes

**Files:**
- Modify: `src/net/webrtc.rs` (the `UiCommand::SendMessage` / `SendMedia` dispatch arms inside the `loop`)

**Step 1: Locate the dispatch arms**

Run: `grep -n "SendMessage\|SendMedia\|UiCommand" src/net/webrtc.rs` — find the `match cmd { ... }` that maps `UiCommand` to `SigCmd`.

**Step 2: Update the dispatch**

```rust
            UiCommand::SendMessage { content, reply } => {
                let message_id = uuid::Uuid::new_v4().to_string();
                let reply_tuple = reply.as_ref().map(|r| (r.db_id.clone(), r.author.clone(), r.content.clone()));
                let _ = sig_cmd_tx.send(crate::net::signaling::SigCmd::BroadcastMessage {
                    content,
                    message_id: message_id.clone(),
                    reply: reply_tuple,
                });
                // The message_id is also needed by the UI thread for the DB insert + db_id stamping.
                // Return it via a side channel: see Task 6 — the UI generates the id itself and
                // passes it in the command, so this dispatch reads it from the command instead.
            }
            UiCommand::SendMedia { caption, url, kind, filename, reply } => {
                let message_id = uuid::Uuid::new_v4().to_string();
                let reply_tuple = reply.as_ref().map(|r| (r.db_id.clone(), r.author.clone(), r.content.clone()));
                let _ = sig_cmd_tx.send(crate::net::signaling::SigCmd::BroadcastMedia {
                    caption, url, kind, filename,
                    message_id: message_id.clone(),
                    reply: reply_tuple,
                });
            }
```

**IMPORTANT correction — the UUID must be generated in the UI thread, not here.** The UI thread needs the same UUID for the optimistic `db_id` and the DB insert. So `UiCommand::SendMessage`/`SendMedia` must carry `message_id: String` as a field (generated by `try_send_message` before sending the command). Revise `UiCommand` in Task 2 Step 3 accordingly:

```rust
    SendMessage { content: String, message_id: String, reply: Option<ReplyTarget> },
    SendMedia { caption: String, url: String, kind: AttachmentKind, filename: String, message_id: String, reply: Option<ReplyTarget> },
```

Then the dispatch just forwards:

```rust
            UiCommand::SendMessage { content, message_id, reply } => {
                let reply_tuple = reply.as_ref().map(|r| (r.db_id.clone(), r.author.clone(), r.content.clone()));
                let _ = sig_cmd_tx.send(crate::net::signaling::SigCmd::BroadcastMessage {
                    content, message_id, reply: reply_tuple,
                });
            }
            UiCommand::SendMedia { caption, url, kind, filename, message_id, reply } => {
                let reply_tuple = reply.as_ref().map(|r| (r.db_id.clone(), r.author.clone(), r.content.clone()));
                let _ = sig_cmd_tx.send(crate::net::signaling::SigCmd::BroadcastMedia {
                    caption, url, kind, filename, message_id, reply: reply_tuple,
                });
            }
```

Apply this correction to Task 2's `UiCommand` definition before continuing (edit `src/state.rs` if you already wrote the narrower version).

**Step 3: Verify + commit**

```bash
cargo build
```

Expected: clean.

```bash
git add src/net/webrtc.rs src/state.rs
git commit -m "webrtc: forward message_id + reply in SendMessage/SendMedia dispatch"
```

---

## Task 6: Rust UI — generate UUID, send with `message_id`+`reply`, stamp `db_id`, reply affordance + render

**Files:**
- Modify: `src/ui/chat.rs` (`try_send_message` `:745-771`, `poll_media_upload` `:635-686`, `pick_and_upload_media` `:688-740`, `render_input_bar` `:561-631`, `render` `:12-88`)
- Modify: `src/ui/components.rs` (`render_chat_message` `:111-180`, context menu `:162-173`)
- Modify: `src/app.rs:272-274` (`MessageReceived` application)

**Step 1: `try_send_message` — generate UUID, thread through broadcast + optimistic + DB**

Rewrite `src/ui/chat.rs:745-771`:

```rust
fn try_send_message(state: &mut AppState) {
    let content = state.message_input.trim().to_string();
    if content.is_empty() { return; }
    state.message_input.clear();
    if let Some(tx) = &state.cmd_tx {
        let _ = tx.send(crate::state::UiCommand::SendTyping(false));
    }

    let message_id = uuid::Uuid::new_v4().to_string();
    let reply = state.reply_target.take(); // cleared on send
    let reply_ref = reply.as_ref();
    let reply_db = reply_ref.map(|r| r.db_id.clone());

    // Broadcast (carries message_id + reply)
    if let Some(tx) = &state.cmd_tx {
        let _ = tx.send(crate::state::UiCommand::SendMessage {
            content: content.clone(),
            message_id: message_id.clone(),
            reply: reply.clone(),
        });
    }

    // Optimistic local display — born with its db_id
    state.push_own_media(content.clone(), None, reply_ref);
    // Stamp the just-pushed message's db_id
    if let Some(m) = state.messages.last_mut() {
        m.db_id = Some(message_id.clone());
    }

    // DB persist (fire-and-forget) — explicit id + reply fields
    if let Some(ref s) = state.session {
        let at   = s.access_token.clone();
        let from = state.username.clone();
        let id   = message_id.clone();
        let reply_tuple = reply_ref.map(|r| (r.db_id.clone(), r.author.clone(), r.content.clone()));
        thread::spawn(move || {
            let reply_args = reply_tuple.as_ref().map(|(a,b,c)| (a.as_str(), b.as_str(), c.as_str()));
            if let Err(e) = crate::net::supabase::insert_message(&at, &from, &content, None, &id, reply_args) {
                log::warn!("[chat] DB insert failed: {}", e);
            }
        });
    }
}
```

Add `use uuid::Uuid;` (or `use uuid;`) at the top of `chat.rs`.

**Step 2: `poll_media_upload` — same UUID threading for media**

In `src/ui/chat.rs:635-686`, after the upload succeeds, generate `message_id`, pass into `SendMedia` + `push_own_media` + `insert_message`. Mirror the text path. The media `insert_message` call (`:672`) already passes an `Attachment`; add `id` + `reply` args.

**Step 3: `apply_net_event` — stamp live-arrived messages**

In `src/app.rs:272-274`, change:

```rust
            NetEvent::MessageReceived { from, content, attachment } => {
                self.push_peer_media(from, content, attachment);
            }
```

to:

```rust
            NetEvent::MessageReceived { from, content, attachment, message_id, reply_to, reply_to_author, reply_to_content } => {
                let reply = match (reply_to, reply_to_author, reply_to_content) {
                    (Some(to), Some(a), Some(c)) => Some((to, a, c)),
                    _ => None,
                };
                self.push_peer_media(from, content, attachment, message_id, reply);
            }
```

**Step 4: Reply affordance — context menu "Reply"**

In `src/ui/components.rs:162-173`, extend the context menu to add a "Reply" button after the emoji row:

```rust
            content_resp.context_menu(|ui| {
                ui.horizontal(|ui| {
                    for emoji in QUICK_EMOJIS {
                        if ui.button(RichText::new(*emoji).size(18.0)).clicked() {
                            if let Some(ref db_id) = msg.db_id {
                                let already = msg.reactions.iter().any(|r| r.user == local_username && r.emoji == *emoji);
                                toggle = Some((db_id.clone(), emoji.to_string(), !already));
                            }
                        }
                    }
                });
                ui.separator();
                if ui.button("Reply").clicked() {
                    if let Some(ref db_id) = msg.db_id {
                        // Return a reply-trigger signal up to the caller via a second channel:
                        // simplest is to store on a shared mutable. Instead, we re-render the
                        // input bar to read a global. Cleanest: return an enum.
                    }
                }
            });
```

The cleanest way to set `state.reply_target` from inside `render_message` (which only has `&Ui`, not `&mut AppState`) is to change `render_message`'s return type to also signal a reply intent. Extend the return tuple:

Change `render_message` signature (`:77-88`) to return `Option<MessageAction>` where:

```rust
pub enum MessageAction {
    ReactionToggle { message_id: String, emoji: String, active: bool },
    Reply { db_id: String, author: String, content: String },
}
```

Update `render_chat_message` to populate `MessageAction::Reply` when the "Reply" button is clicked (truncate `content` to 100 chars). Update `render_message_area` in `chat.rs:506-550` to handle `MessageAction::Reply` by setting `state.reply_target = Some(ReplyTarget { ... })` and requesting focus on the input.

**Step 5: Reply preview bar above the input**

In `src/ui/chat.rs` `render` (`:12-88`), add a `TopBottomPanel::bottom("reply_bar")` above `input_bar` that renders when `state.reply_target.is_some()`: shows `↪ Replying to {author}: {truncated content}` + a ✕ button that sets `state.reply_target = None`. Also handle ESC in `render_input_bar` (`:614`) to clear `reply_target`.

**Step 6: Render the reply reference above the message body**

In `src/ui/components.rs` `render_chat_message` (`:133-143`), before the author header, if `msg.reply_to.is_some()`, render a compact line:

```rust
            if let (Some(_to), Some(author), Some(snippet)) = (&msg.reply_to, &msg.reply_to_author, &msg.reply_to_content) {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("↪").size(11.0).color(theme::TEXT_MUTED));
                    ui.label(RichText::new(format!("@{}: {}", author, snippet)).size(11.0).color(theme::TEXT_MUTED));
                });
                ui.add_space(2.0);
            }
```

(Optional: make it clickable to scroll to the parent if `msg.reply_to` matches a local `db_id`. Keep simple for v1 — non-clickable is acceptable.)

**Step 7: Verify + commit**

```bash
cargo build
```

Expected: clean. Manual E2E (two native clients): send a message → reply to it from the other client → quoted reference renders on both; react to a just-arrived live message → pill appears (previously dropped).

```bash
git add src/ui/chat.rs src/ui/components.rs src/app.rs
git commit -m "ui: reply affordance + render, client-generated message_id, live db_id stamping"
```

---

## Task 7: Rust mentions — `@username` highlight + notification sound

**Files:**
- Modify: `src/ui/components.rs` (`render_chat_message` content rendering `:144-149`)
- Modify: `src/app.rs` (`MessageReceived` arm — trigger sound)
- Create: `src/audio/notification.rs` + bundle a WAV asset
- Modify: `src/audio/mod.rs`

**Step 1: Bundle a notification sound asset**

Create `src/audio/assets/notification.wav` — a short (<1s) public-domain ping. If you cannot source one, generate a 440Hz sine burst via a tiny script and commit the WAV. Reference it with `include_bytes!("assets/notification.wav")` in `src/audio/notification.rs`.

**Step 2: `src/audio/notification.rs`**

```rust
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;

const NOTIFICATION_WAV: &[u8] = include_bytes!("assets/notification.wav");

/// Play a short notification ping on the default output device.
/// Fire-and-forget; errors are logged and never propagated (must not block the UI).
pub fn play_notification() {
    std::thread::spawn(|| {
        if let Err(e) = play_notification_blocking() {
            log::warn!("[audio] notification playback failed: {}", e);
        }
    });
}

fn play_notification_blocking() -> anyhow::Result<()> {
    // Decode the WAV to i16 samples + sample rate using the `image`-style approach:
    // simplest is the `hound` crate, but to avoid a new dep, parse the minimal WAV header
    // manually (44-byte header → 16-bit PCM samples). If that's too much, add `hound` to
    // Cargo.toml [target.'cfg(not(target_arch = "wasm32"))'.dependencies].
    // ...decode to Vec<i16> + sample_rate...
    // Then output via cpal the same way playback.rs does, one-shot.
    todo!("decode WAV + one-shot cpal playback")
}
```

**Decision point:** if manual WAV parsing is more than ~40 lines, add `hound = "0.11"` to the native-only deps in `Cargo.toml` (`[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`). Prefer the crate — it's tiny and correct.

Add `pub mod notification;` to `src/audio/mod.rs`.

**Step 3: Trigger the sound on mentioned message receipt**

In `src/app.rs` `MessageReceived` arm (the one updated in Task 6 Step 3), after `push_peer_media`, check for a mention:

```rust
                // Mention ping: if the message content contains @<local username>, play a sound.
                let local = self.username.clone();
                let mention = format!("@{}", local);
                if content.contains(&mention) {
                    crate::audio::notification::play_notification();
                }
```

(Place this before the `push_peer_media` call captures `content`, or clone `content` first.)

**Step 4: Highlight `@username` in rendered content**

In `src/ui/components.rs:144-149`, replace the single `Label::new` with mention-aware rendering. Split `content` on `@<known-username>` tokens (known = `local_username` + all peers — but `components.rs` doesn't have peer access; pass a `&[String]` of known usernames into `render_message`). For each matched `@username`, render it as a styled `RichText` segment in `theme::ACCENT` with a subtle background; unmatched text renders normally.

Because `render_message` doesn't currently receive the peer list, add a `known_users: &[String]` parameter (or pass `&AppState` — but `components.rs` is kept free of `AppState`). Thread it from `render_message_area` (`chat.rs:506-550`), which builds `avatar_map` from `state.peers` already — also build a `known_users: Vec<String>` there and pass it through.

Keep it simple: build the set once per frame, do a linear scan for `@name` per message.

**Step 5: Verify + commit**

```bash
cargo build
```

Expected: clean. Manual E2E: from client B, send `@alice hello` → client A (alice) hears the ping + sees `@alice` highlighted; send `@nobody` → no highlight, no sound.

```bash
git add src/audio/ src/ui/components.rs src/app.rs src/ui/chat.rs Cargo.toml Cargo.lock
git commit -m "mentions: @username highlight + notification sound on ping"
```

---

## Task 8: Web — `message_id` + reply on send/receive/render

**Files:**
- Modify: `web/app.js` (`trySend` `:811-824`, `uploadMedia` `:826-854`, `onChatMessage` `:480-483`, `onChatMedia` `:485-488`, `appendMsg` `:859-922`, `onReaction` already uses `dataset.msgId`)
- Modify: `web/style.css` (`.msg-reply-ref`, `.mention`)
- Create: `web/notification.mp3`

**Step 1: `trySend` — generate UUID, include `message_id` + reply**

Rewrite `web/app.js:811-824`:

```js
async function trySend() {
  const input   = $('message-input');
  const content = input.textContent.trim();
  if (!content) return;
  input.textContent = '';
  $('input-placeholder').style.display = '';
  bcast(EVENTS.TYPING, { from: myUsername, is_typing: false });

  const messageId = crypto.randomUUID();
  const reply = pendingReply ? {
    reply_to: pendingReply.dbId, reply_to_author: pendingReply.author, reply_to_content: pendingReply.content,
  } : {};

  bcast(EVENTS.CHAT_MESSAGE, { from: myUsername, content, message_id: messageId, ...reply });
  appendMsg(myUsername, content, new Date(), true, null, messageId, reply.reply_to ? reply : null);

  const { error } = await sb.from('messages').insert({
    id: messageId, from_user: myUsername, content, channel: DEFAULT_DB_CHANNEL,
    ...(reply.reply_to ? { reply_to_id: reply.reply_to, reply_to_author: reply.reply_to_author, reply_to_content: reply.reply_to_content } : {}),
  });
  if (error) sysMsg(`⚠ Message not saved to history: ${error.message}`);
  pendingReply = null;
  hideReplyPreview();
}
```

Add a module-level `let pendingReply = null;` near the top of the chat section.

**Step 2: `uploadMedia` — same UUID threading**

In `web/app.js:826-854`, generate `messageId`, include in `bcast` + `insert` + `appendMsg`.

**Step 3: `onChatMessage` / `onChatMedia` — read `message_id` + reply**

```js
function onChatMessage({ from, content, message_id, reply_to, reply_to_author, reply_to_content }) {
  if (!from || !content) return;
  const reply = reply_to ? { reply_to, reply_to_author, reply_to_content } : null;
  appendMsg(from, content, new Date(), true, null, message_id || null, reply);
  if (content.includes(`@${myUsername}`)) playNotification();
}
function onChatMedia({ from, content, url, kind, filename, message_id, reply_to, reply_to_author, reply_to_content }) {
  if (!from || !url) return;
  const reply = reply_to ? { reply_to, reply_to_author, reply_to_content } : null;
  appendMsg(from, content || '', new Date(), true, { url, kind: kind || 'image', filename: filename || 'attachment' }, message_id || null, reply);
  if ((content || '').includes(`@${myUsername}`)) playNotification();
}
```

**Step 4: `appendMsg` — accept `dbId` (already does) + `reply`, render `.msg-reply-ref`**

In `web/app.js:859`, add a `reply = null` parameter. After the header (`:893-896`), before `.msg-content`, if `reply`, render:

```js
  if (reply) {
    const refEl = document.createElement('div');
    refEl.className = 'msg-reply-ref';
    refEl.innerHTML = `<span class="reply-arrow">↪</span> @${esc(reply.reply_to_author)}: ${esc(reply.reply_to_content)}`;
    group.appendChild(refEl);
  }
```

**Step 5: Reply affordance — hover "Reply" button + preview bar**

Add a hover "Reply" button to each `.msg-group` (in `appendMsg`, after building the group). On click, set `pendingReply = { dbId: group.dataset.msgId, author: from, content: content.slice(0,100) }` and show `#reply-preview` (a bar above `#message-input` showing `↪ Replying to @author: snippet` + a ✕). `hideReplyPreview()` clears it. ESC clears too.

**Step 6: Mention highlight in content**

In `appendMsg` content rendering (`:905-908`), wrap matched `@<knownUsername>` in `<span class="mention">@name</span>`. Build `knownUsers` once per render (or maintain a `Set` updated on peer join/leave). Unmatched `@foo` stays plain text.

**Step 7: Notification sound**

Create `web/notification.mp3` (short ping). Add:

```js
let notificationAudio = null;
function playNotification() {
  if (!notificationAudio) notificationAudio = new Audio('notification.mp3');
  notificationAudio.currentTime = 0;
  notificationAudio.play().catch(() => {}); // autoplay blocks until gesture
}
// Unlock on first user gesture:
document.addEventListener('pointerdown', () => { playNotification(); }, { once: true });
```

(The first-gesture call is silent if no real mention; it just unlocks the AudioContext for future programmatic plays.)

**Step 8: CSS**

In `web/style.css`:

```css
.msg-reply-ref { font-size: 11px; color: var(--text-muted); margin: 2px 0 4px; border-left: 2px solid var(--separator); padding-left: 6px; }
.reply-arrow { color: var(--text-muted); margin-right: 2px; }
.mention { color: var(--accent, #5865f2); background: rgba(88,101,242,0.15); border-radius: 3px; padding: 0 2px; }
#reply-preview { /* bar above input: muted bg, reply snippet, ✕ button */ }
```

**Step 9: Verify + commit**

Open `web/index.html` in two browsers (or native + browser). Manual E2E: reply to a live message → quoted ref renders on both; `@mention` highlights + pings; react to a live message works.

```bash
git add web/app.js web/style.css web/notification.mp3
git commit -m "web: message_id + replies + @mention highlight + notification sound"
```

---

## Task 9: SQL migration + final cross-target E2E

**Step 1: Run the SQL migration in the Supabase dashboard SQL editor**

```sql
ALTER TABLE messages ADD COLUMN IF NOT EXISTS reply_to_id      UUID REFERENCES messages(id) ON DELETE SET NULL;
ALTER TABLE messages ADD COLUMN IF NOT EXISTS reply_to_author  TEXT;
ALTER TABLE messages ADD COLUMN IF NOT EXISTS reply_to_content TEXT;
```

(The `messages.id` column already accepts explicit UUIDs — no change needed there.)

**Step 2: Cross-target E2E matrix**

Run two clients and verify each cell:

| Scenario | Native→Native | Native→Web | Web→Native |
|---|---|---|---|
| Reply to live message renders quoted ref | ☐ | ☐ | ☐ |
| Reply to out-of-window message renders snippet | ☐ | ☐ | ☐ |
| React to live message (previously dropped) | ☐ | ☐ | ☐ |
| `@mention` highlights + sounds | ☐ | ☐ | ☐ |
| Unmatched `@foo` is plain text | ☐ | ☐ | ☐ |
| Reply ESC cancels pending reply | ☐ | ☐ | ☐ |

**Step 3: Final commit (if any drift)**

```bash
git add -A
git commit -m "chore: replies + mentions cross-target E2E verified"
```

---

## Notes for the executor

- **No mocks.** All verification is `cargo build` + real two-client E2E against the live Supabase project. The repo has no test harness; do not invent one.
- **Edit the canonical contract first** (Task 0), then mirror — every contract change flows from `docs/plans/2026-06-27-message-contract.md`.
- **Client-generated UUIDs, not `return=representation`.** The send order is broadcast → insert; the UUID must be known before broadcast. Generate it in the UI thread (`Uuid::new_v4()` / `crypto.randomUUID()`), use it for `message_id` + `db_id` + insert `id`.
- **Task 5 contains a correction to Task 2's `UiCommand` shape** — `SendMessage`/`SendMedia` must carry `message_id: String`. Apply that correction when you reach Task 5 (or preemptively in Task 2).
- **`hound` crate decision** in Task 7: if manual WAV parsing is too much, add `hound` to native-only deps. Prefer the crate.
