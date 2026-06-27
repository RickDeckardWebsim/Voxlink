# Social Layer Phase 1: Typing Indicators & Emoji Reactions — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use executing-plans to implement this plan task-by-task.

**Goal:** Add typing indicators and emoji reactions to VoxLink chat across native (Rust/egui) and web (vanilla JS) targets, within no-server/no-cost/lightweight/cross-OS constraints.

**Architecture:** Both features ride existing infra — Supabase Postgres for reaction persistence (new `reactions` side table), Supabase Realtime broadcasts for live events (two new contract events `typing` + `reaction`), existing `messages.id` UUID as join key. Typing is ephemeral (no persistence). Reactions hydrate from DB on connect alongside message history. Optimistic local apply + fire-and-forget DB write, matching the existing message-send pattern.

**Tech Stack:** Rust/egui 0.34, vanilla JS (no build step), Supabase PostgREST + Realtime, existing `src/net/contract.rs` + `web/contract.js` contract modules.

**Design doc:** `docs/plans/2026-06-27-social-layer-phase1-design.md`

**Testing note:** This codebase has no unit-test framework. Per-task verification is `cargo check` (native compile gate) and `node --check` (web parse gate). The final task is a manual integration smoke test against the real Supabase project (no mocks — per design §4). The `reactions` table SQL must be run in the Supabase dashboard before the smoke test.

---

## Task 1: Add `typing` and `reaction` event constants to the contract

**Files:**
- Modify: `src/net/contract.rs`
- Modify: `web/contract.js`

**Step 1: Add event constants to `src/net/contract.rs`**

The `event` module is at lines 14-23. Add two new constants inside it, before the closing `}`:

```rust
    pub const TYPING: &str        = "typing";
    pub const REACTION: &str      = "reaction";
```

Insert after `SDP_ANSWER` (line 22), before the `}`.

**Step 2: Add event constants to `web/contract.js`**

The `EVENTS` object is at lines 13-22. Add two new entries before the closing `}`:

```js
  TYPING:         'typing',
  REACTION:       'reaction',
```

Insert after `SDP_ANSWER` (line 21), before the `}`.

**Verify:** `cargo check` passes (the new constants will be unused until later tasks — that's expected, they'll produce dead-code warnings that clear as callers wire up).

**Commit:**
```bash
git add src/net/contract.rs web/contract.js
git commit -m "feat: add typing + reaction event constants to contract"
```

---

## Task 2: Add `Reaction` type, extend `ChatMessage`, add `NetEvent`/`UiCommand` variants, add `AppState.typing_users`

**Files:**
- Modify: `src/state.rs`

**Step 1: Add `Reaction` struct**

After the `Attachment` struct (closes at line 124), add:

```rust
/// A single emoji reaction on a chat message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Reaction {
    pub user: String,
    pub emoji: String,
}
```

**Step 2: Add `reactions` field to `ChatMessage`**

The `ChatMessage` struct is at lines 145-160. Add a new field after `unix_ts` (line 159), before the closing `}`:

```rust
    /// Emoji reactions on this message (hydrated from DB + live broadcasts).
    #[serde(default)]
    pub reactions: Vec<Reaction>,
```

**Step 3: Update the three `ChatMessage` constructors**

In `new_own` (line 163-178), `new_peer` (line 180-195), and `new_system` (line 197-207), add `reactions: Vec::new(),` to each struct literal. Place it after `unix_ts: 0,` (or `unix_ts: unix_now(),` for system).

**Step 4: Add `typing_users` field to `AppState`**

In the `AppState` struct (line 326-423), after the `// ── Chat ──` block's `scroll_to_bottom` field (line 403), add:

```rust
    /// Usernames currently typing (excluding self). Auto-expired by the UI loop.
    pub typing_users: Vec<String>,
```

**Step 5: Initialize `typing_users` in `Default`**

In `impl Default for AppState` (line 425-505), after `scroll_to_bottom: false,` (line 488), add:

```rust
            typing_users: Vec::new(),
```

**Step 6: Add `NetEvent` variants**

The `NetEvent` enum is at lines 285-302. Add two new variants before the closing `}`:

```rust
    /// A peer's typing state changed.
    TypingUpdate { from: String, is_typing: bool },
    /// A peer added or removed a reaction.
    ReactionUpdate { from: String, message_id: String, emoji: String, active: bool },
```

**Step 7: Add `UiCommand` variants**

The `UiCommand` enum is at lines 307-318. Add two new variants before `Disconnect` (line 317):

```rust
    /// Broadcast a typing indicator ping (throttled by the caller).
    SendTyping(bool),
    /// Toggle a reaction on a message (optimistic + broadcast + DB persist).
    SendReaction { message_id: String, emoji: String, active: bool },
    Disconnect,
```

**Verify:** `cargo check` passes. New variants/fields will show as unused until later tasks wire them.

**Commit:**
```bash
git add src/state.rs
git commit -m "feat: add Reaction type, ChatMessage.reactions, typing_users, NetEvent/UiCommand variants"
```

---

## Task 3: Add `SigCmd` variants + send/recv handlers in `signaling.rs`

**Files:**
- Modify: `src/net/signaling.rs`

**Depends on:** Task 1 (event constants), Task 2 (NetEvent variants)

**Step 1: Add `SigCmd` variants**

The `SigCmd` enum is at lines 14-27. Add two new variants before `Disconnect` (line 26):

```rust
    /// Broadcast a typing indicator ping.
    BroadcastTyping { is_typing: bool },
    /// Broadcast a reaction toggle.
    BroadcastReaction { message_id: String, emoji: String, active: bool },
    /// Broadcast our departure, then close the WebSocket gracefully.
    BroadcastPeerLeave,
    Disconnect,
```

**Step 2: Add send arms in `connect_and_run`**

In the `match cmd` block (lines 110-186), add two new arms before `SigCmd::BroadcastPeerLeave` (line 111):

```rust
                        SigCmd::BroadcastTyping { is_typing } => {
                            let topic = crate::net::contract::SIGNALING_TOPIC.to_string();
                            let broadcast = make_broadcast(&topic, crate::net::contract::event::TYPING, json!({
                                "from":       username,
                                "is_typing":  is_typing,
                            }), &mut ref_count);
                            let _ = send_text(&mut ws_stream, &broadcast).await;
                        }
                        SigCmd::BroadcastReaction { message_id, emoji, active } => {
                            let topic = crate::net::contract::SIGNALING_TOPIC.to_string();
                            let broadcast = make_broadcast(&topic, crate::net::contract::event::REACTION, json!({
                                "from":        username,
                                "message_id":  message_id,
                                "emoji":       emoji,
                                "active":      active,
                            }), &mut ref_count);
                            let _ = send_text(&mut ws_stream, &broadcast).await;
                        }
```

Use `let _ = send_text(...)` (best-effort) for typing — a dropped ping is harmless. For reactions, use `?` (a reaction broadcast failure should surface, matching the existing `BroadcastMessage` pattern). Actually, to be consistent with the `BroadcastPeerLeave` best-effort approach and since reactions are also non-critical UI state, use `let _ =` for both.

**Step 3: Add recv arms in `handle_incoming`**

In the `match b_event` block (lines 229-312), add two new arms before the `_ => {}` catch-all (line 312):

```rust
                crate::net::contract::event::TYPING => {
                    let is_typing = b_payload["is_typing"].as_bool().unwrap_or(false);
                    let _ = net_tx.send(NetEvent::TypingUpdate {
                        from: from.to_string(),
                        is_typing,
                    });
                    ctx.request_repaint();
                }
                crate::net::contract::event::REACTION => {
                    let message_id = b_payload["message_id"].as_str().unwrap_or("").to_string();
                    let emoji      = b_payload["emoji"].as_str().unwrap_or("").to_string();
                    let active     = b_payload["active"].as_bool().unwrap_or(true);
                    if !message_id.is_empty() && !emoji.is_empty() {
                        let _ = net_tx.send(NetEvent::ReactionUpdate {
                            from: from.to_string(),
                            message_id,
                            emoji,
                            active,
                        });
                        ctx.request_repaint();
                    }
                }
```

The `from == username` guard at line 227 already filters own broadcasts, so we never receive our own typing/reaction events.

**Verify:** `cargo check` passes.

**Commit:**
```bash
git add src/net/signaling.rs
git commit -m "feat: add typing + reaction SigCmd variants and send/recv handlers"
```

---

## Task 4: Route `UiCommand::SendTyping` and `UiCommand::SendReaction` through `webrtc.rs`

**Files:**
- Modify: `src/net/webrtc.rs`

**Depends on:** Task 2 (UiCommand variants), Task 3 (SigCmd variants)

**Step 1: Add two new match arms in the `UiCommand` routing**

The `match cmd` block is at lines 296-367. Add two new arms before the closing of the match (after `UiCommand::SetMuted` at line 365, before the catch-all if any — actually the match doesn't have an explicit catch-all, it just ends at `}`). Add before the closing `}` of the match:

```rust
                        UiCommand::SendTyping(is_typing) => {
                            let _ = sig_cmd_tx.send(crate::net::signaling::SigCmd::BroadcastTyping { is_typing });
                        }
                        UiCommand::SendReaction { message_id, emoji, active } => {
                            let _ = sig_cmd_tx.send(crate::net::signaling::SigCmd::BroadcastReaction {
                                message_id, emoji, active,
                            });
                        }
```

**Verify:** `cargo check` passes.

**Commit:**
```bash
git add src/net/webrtc.rs
git commit -m "feat: route SendTyping + SendReaction UiCommands through webrtc"
```

---

## Task 5: Add reaction persistence functions in `supabase.rs`

**Files:**
- Modify: `src/net/supabase.rs`

**Depends on:** Task 2 (Reaction struct)

**Step 1: Add `insert_reaction` and `delete_reaction` functions**

After the `fetch_recent_messages` function (ends at line 470), add:

```rust
// ── Reaction persistence ─────────────────────────────────────────────────────
//
// Required Supabase table (run once in the SQL editor):
//
//   CREATE TABLE IF NOT EXISTS reactions (
//     message_id  UUID        NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
//     user        TEXT        NOT NULL,
//     emoji       TEXT        NOT NULL,
//     created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
//     PRIMARY KEY (message_id, user, emoji)
//   );
//   ALTER TABLE reactions ENABLE ROW LEVEL SECURITY;
//   CREATE POLICY rx_read   ON reactions FOR SELECT USING (true);
//   CREATE POLICY rx_insert ON reactions FOR INSERT TO authenticated WITH CHECK (true);
//   CREATE POLICY rx_delete ON reactions FOR DELETE TO authenticated USING (true);

/// Insert a reaction (fire-and-forget). Composite PK prevents duplicates.
pub fn insert_reaction(access_token: &str, message_id: &str, user: &str, emoji: &str) -> Result<()> {
    let client = Client::new();
    let url = format!("{}/rest/v1/reactions", contract::SUPABASE_URL);
    let body = json!({
        "message_id": message_id,
        "user":       user,
        "emoji":      emoji,
    });
    let res = client.post(&url)
        .header("apikey",         contract::SUPABASE_ANON_KEY)
        .header("Authorization",  format!("Bearer {}", access_token))
        .header("Content-Type",   "application/json")
        .header("Prefer",         "return=minimal")
        .json(&body)
        .send()?;
    if !res.status().is_success() {
        let err = res.text().unwrap_or_default();
        log::warn!("[supabase] reaction insert failed: {}", err);
    }
    Ok(())
}

/// Delete a reaction (fire-and-forget).
pub fn delete_reaction(access_token: &str, message_id: &str, user: &str, emoji: &str) -> Result<()> {
    let client = Client::new();
    let url = format!(
        "{}/rest/v1/reactions?message_id=eq.{}&user=eq.{}&emoji=eq.{}",
        contract::SUPABASE_URL, message_id, user, emoji
    );
    let res = client.delete(&url)
        .header("apikey",         contract::SUPABASE_ANON_KEY)
        .header("Authorization",  format!("Bearer {}", access_token))
        .send()?;
    if !res.status().is_success() {
        let err = res.text().unwrap_or_default();
        log::warn!("[supabase] reaction delete failed: {}", err);
    }
    Ok(())
}
```

**Step 2: Add `fetch_reactions` function**

After `delete_reaction`, add:

```rust
/// Fetch all reactions for a set of message IDs. Returns a map of message_id → Vec<Reaction>.
pub fn fetch_reactions(
    access_token: &str,
    message_ids: &[String],
) -> Result<std::collections::HashMap<String, Vec<crate::state::Reaction>>> {
    let client = Client::new();
    // PostgREST `in` filter: message_id=in.(id1,id2,...)
    let ids_csv = message_ids.join(",");
    let url = format!(
        "{}/rest/v1/reactions?select=message_id,user,emoji&message_id=in.({})",
        contract::SUPABASE_URL, ids_csv
    );

    let res = client.get(&url)
        .header("apikey",        contract::SUPABASE_ANON_KEY)
        .header("Authorization", format!("Bearer {}", access_token))
        .send()?;

    if !res.status().is_success() {
        return Err(anyhow::anyhow!("Reactions fetch failed ({})", res.status()));
    }

    #[derive(Deserialize)]
    struct DbReaction {
        message_id: String,
        user:       String,
        emoji:      String,
    }

    let rows: Vec<DbReaction> = res.json()?;
    let mut map: std::collections::HashMap<String, Vec<crate::state::Reaction>> = std::collections::HashMap::new();
    for row in rows {
        map.entry(row.message_id).or_default().push(crate::state::Reaction {
            user:  row.user,
            emoji: row.emoji,
        });
    }
    Ok(map)
}
```

Note: `fetch_reactions` takes `&[String]` of message IDs. The caller (app.rs history hydration) will need the DB UUIDs — see Task 6 for how message IDs flow. **Important:** the `messages.id` UUID is assigned by Supabase, but the local `ChatMessage.id` is a monotonic u64 set by `next_message_id()`. Reactions key off the *DB UUID*, not the local u64. This means `ChatMessage` needs to store its DB UUID for reactions to work. Add a `db_id: Option<String>` field to `ChatMessage` in Task 2's revision — **see Task 6 for the full explanation and the field addition.**

**Verify:** `cargo check` passes.

**Commit:**
```bash
git add src/net/supabase.rs
git commit -m "feat: add reaction persistence (insert/delete/fetch) in supabase"
```

---

## Task 6: Add `db_id` to `ChatMessage`, wire `NetEvent` handlers in `app.rs`, hydrate reactions on connect

**Files:**
- Modify: `src/state.rs` (add `db_id` field)
- Modify: `src/app.rs` (NetEvent handlers + reaction hydration)
- Modify: `src/net/supabase.rs` (populate `db_id` in `fetch_recent_messages`)

**Depends on:** Task 2 (ChatMessage, NetEvent variants), Task 5 (fetch_reactions)

**Critical design note — message ID duality:**
The local `ChatMessage.id` is a u64 monotonic counter (for egui widget IDs). The DB `messages.id` is a UUID (the reactions FK). To react to a message, the client needs the DB UUID. Solution: add `db_id: Option<String>` to `ChatMessage`. History messages get it from `fetch_recent_messages`; live-sent messages get it from the DB INSERT response (or set to `None` and fetched later — but the simplest approach is to fetch the UUID on insert via `Prefer: return=representation`).

**Step 1: Add `db_id` to `ChatMessage` in `src/state.rs`**

In the `ChatMessage` struct (lines 145-160), add after `reactions`:

```rust
    /// The Supabase DB UUID of this message (for reaction FKs). None for system messages.
    #[serde(default)]
    pub db_id: Option<String>,
```

Update all three constructors to set `db_id: None,`.

**Step 2: Populate `db_id` in `fetch_recent_messages` (`src/net/supabase.rs`)**

The `DbMessage` struct (lines 367-375) needs an `id` field. Add:

```rust
struct DbMessage {
    id:                  String,   // ADD THIS
    from_user:           String,
    content:             String,
    attachment_url:      Option<String>,
    attachment_kind:     Option<String>,
    attachment_filename: Option<String>,
    created_at:          String,
}
```

The fetch URL (line 423-427) currently selects all columns implicitly. Add `select=id,from_user,content,attachment_url,attachment_kind,attachment_filename,created_at` to the query. Update the URL:

```rust
    let url = format!(
        "{}/rest/v1/messages?select=id,from_user,content,attachment_url,attachment_kind,attachment_filename,created_at&channel=eq.{}&order=created_at.desc&limit=100",
        contract::SUPABASE_URL,
        contract::DEFAULT_DB_CHANNEL
    );
```

In the `ChatMessage` construction (lines 458-466), add `db_id: Some(row.id),`.

**Step 3: Set `db_id` on live-sent messages (`src/ui/chat.rs`)**

In `try_send_message` (lines 684-706), the DB insert is fire-and-forget. To get the UUID back, change the insert to `Prefer: return=representation` and parse the response. However, this complicates the fire-and-forget pattern. **Simpler approach:** after the optimistic `push_own`, the message has `db_id: None`. When a peer receives our message via their own history fetch, they get the UUID. For *our own* reactions on *our own* just-sent messages, we need the UUID immediately. 

**Decision:** Change `insert_message` to return the UUID. Modify the fire-and-forget thread in `try_send_message` to update the message's `db_id` after insert. This requires `insert_message` to return `Result<String>` (the UUID) and the caller to find the message by local u64 ID and set its `db_id`. Add a helper `AppState::set_message_db_id(&mut self, local_id: u64, db_id: String)`.

Actually, this is getting complex. **Simplest viable approach:** make `insert_message` use `Prefer: return=representation` and return the UUID. The `try_send_message` thread sends the UUID back via an mpsc channel, and `poll` updates the message. But this adds a new rx channel to AppState.

**Even simpler:** Since reactions are keyed on DB UUID and history fetch always populates `db_id`, the only gap is reacting to a message *I just sent* before reloading. For Phase 1, accept this limitation: if `db_id` is `None`, the reaction button is disabled with a tooltip "Reactions available after sync." This avoids complicating the send path. Document in the design doc as a known Phase 1 limitation.

**Step 4: Wire `NetEvent::TypingUpdate` and `NetEvent::ReactionUpdate` in `app.rs`**

In `apply_net_event` (lines 179-266), add two new arms before the closing `}` of the match (after `VoiceStateUpdate` at line 264):

```rust
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
                // Find the message by DB UUID and toggle the reaction.
                if let Some(msg) = self.messages.iter_mut().find(|m| m.db_id.as_deref() == Some(&message_id)) {
                    if active {
                        // Add if not already present (composite PK dedup).
                        let exists = msg.reactions.iter().any(|r| r.user == from && r.emoji == emoji);
                        if !exists {
                            msg.reactions.push(crate::state::Reaction { user: from, emoji });
                        }
                    } else {
                        msg.reactions.retain(|r| !(r.user == from && r.emoji == emoji));
                    }
                }
                // If the message isn't found (edge case #1), drop silently — re-hydrated on next history fetch.
            }
```

**Step 5: Hydrate reactions on connect**

In the `NetEvent::Connected` arm (lines 181-196), after the history fetch thread is spawned, we need a *sibling* fetch for reactions. But reactions need the message IDs from the history fetch — so the reaction fetch must happen *after* the history fetch completes. 

**Approach:** chain the reaction fetch inside the same background thread. Modify the `Connected` arm's thread spawn (lines 189-194):

```rust
                    std::thread::spawn(move || {
                        let messages = crate::net::supabase::fetch_recent_messages(
                            &access_token, &username,
                        ).map_err(|e| e.to_string());

                        // If messages fetched OK, also fetch their reactions.
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
```

This chains the reaction fetch after the message fetch in the same thread, merging reactions into each `ChatMessage` before sending the result back. The existing `history_rx` consumer in the UI loop already handles `Ok(Vec<ChatMessage>)` — no change needed there.

**Verify:** `cargo check` passes.

**Commit:**
```bash
git add src/state.rs src/app.rs src/net/supabase.rs
git commit -m "feat: add db_id to ChatMessage, wire typing/reaction NetEvents, hydrate reactions on connect"
```

---

## Task 7: Add typing indicator bar + typing send logic in `chat.rs`

**Files:**
- Modify: `src/ui/chat.rs`

**Depends on:** Task 2 (typing_users), Task 4 (UiCommand::SendTyping)

**Step 1: Add typing indicator panel in `render`**

In `render` (lines 12-70), the input bar panel is at lines 43-57. Add a typing indicator panel *above* it (between the channel header and the input bar). Insert after the channel header panel (line 40) and before the input bar panel (line 42):

```rust
    // ── Typing indicator (above input bar) ────────────────────────────────────
    if !state.typing_users.is_empty() {
        egui::TopBottomPanel::bottom("typing_bar")
            .resizable(false)
            .exact_size(22.0)
            .frame(Frame::default().fill(chat_fill).inner_margin(Margin { left: 16, right: 16, top: 2, bottom: 0 }))
            .show(ctx, |ui| {
                let text = match state.typing_users.len() {
                    1 => format!("{} is typing…", state.typing_users[0]),
                    2 => format!("{} and {} are typing…", state.typing_users[0], state.typing_users[1]),
                    _ => "Several people are typing…".to_string(),
                };
                ui.label(RichText::new(text).size(12.0).color(theme::TEXT_MUTED));
            });
    }
```

**Step 2: Add typing expiry in `render`**

At the top of `render` (after `poll_media_upload(ctx, state);` at line 13), add a call to expire stale typing entries. Add a new function and call it:

```rust
    poll_typing_expiry(state);
```

Add the function at the bottom of `chat.rs`:

```rust
/// Remove typing entries older than 4 seconds (receiver-side expiry).
fn poll_typing_expiry(state: &mut AppState) {
    // The typing_users Vec is rebuilt from broadcast pings. Since we don't store
    // timestamps per user in the Vec (keeping it lightweight), we rely on the
    // sender sending a `false` ping on send/clear. For crash-detection, a full
    // solution needs timestamps — for Phase 1, we accept that a crashed user's
    // typing indicator stays until they're removed from peers on disconnect.
    // When a peer leaves (PeerLeft), clear their typing entry:
    state.typing_users.retain(|u| state.peers.iter().any(|p| &p.username == u));
}
```

**Note:** Full timestamp-based expiry requires storing `(username, Instant)` pairs. For Phase 1 simplicity, we clear typing entries when a peer leaves (via `PeerLeft`). A `false` ping on send/clear handles the normal case. This is a documented simplification.

**Step 3: Add throttled typing send on keystroke**

In `render_input_bar` (lines 510-570), after the TextEdit response (line 558), add typing ping logic. After the `if response.lost_focus()...` block (line 563-566), add:

```rust
                    // Typing indicator: send a typing ping on keystroke if throttled.
                    if response.changed() && !state.message_input.trim().is_empty() {
                        // Throttle: only send if last ping was >3s ago.
                        // Store last typing ping time in a field on AppState.
                        let now = std::time::Instant::now();
                        if now.duration_since(state.last_typing_ping) > std::time::Duration::from_secs(3) {
                            state.last_typing_ping = now;
                            if let Some(tx) = &state.cmd_tx {
                                let _ = tx.send(crate::state::UiCommand::SendTyping(true));
                            }
                        }
                    }
```

Add `pub last_typing_ping: std::time::Instant` to `AppState` in `state.rs`, initialized to `std::time::Instant::now()` in `Default`. (This is a small addition to Task 2's scope — add it here or retroactively to Task 2.)

**Step 4: Send `false` typing ping on send**

In `try_send_message` (lines 684-706), after `state.message_input.clear()` (line 687), add:

```rust
    // Clear typing indicator for all peers.
    if let Some(tx) = &state.cmd_tx {
        let _ = tx.send(crate::state::UiCommand::SendTyping(false));
    }
```

**Verify:** `cargo check` passes.

**Commit:**
```bash
git add src/ui/chat.rs src/state.rs
git commit -m "feat: typing indicator bar + throttled typing send in chat UI"
```

---

## Task 8: Add reaction rendering + picker in `components.rs`

**Files:**
- Modify: `src/ui/components.rs`

**Depends on:** Task 2 (Reaction struct, ChatMessage.reactions)

**Step 1: Add reaction rendering below the message bubble**

In `render_chat_message` (lines 105-144), after the attachment render block (lines 137-139), add reaction rendering inside the `ui.vertical` block:

```rust
            // ── Reactions ────────────────────────────────────────────────────
            if !msg.reactions.is_empty() {
                render_reactions(ui, msg);
            }
```

**Step 2: Add `render_reactions` function**

After `render_attachment` (closes at line 213), add:

```rust
const QUICK_EMOJIS: &[&str] = &["👍", "❤️", "😂", "😮", "😢", "🙏"];

fn render_reactions(ui: &mut Ui, msg: &ChatMessage) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        // Group reactions by emoji, count distinct users.
        let mut groups: std::collections::HashMap<&str, Vec<&str>> = std::collections::HashMap::new();
        for r in &msg.reactions {
            groups.entry(r.emoji.as_str()).or_default().push(r.user.as_str());
        }
        for (emoji, users) in &groups {
            let count = users.len();
            let reacted = users.iter().any(|u| u == &msg.author); // will be replaced with self check
            let label = format!("{} {}", emoji, count);
            let pill = egui::Button::new(RichText::new(&label).size(12.0))
                .fill(if reacted { theme::ELEVATED_BG } else { Color32::TRANSPARENT })
                .stroke(egui::Stroke::new(1.0, theme::SEPARATOR))
                .corner_radius(CornerRadius::same(10u8));
            if ui.add(pill).clicked() {
                // Toggle — the click handler needs to know if *I* reacted, not the author.
                // This requires passing the local username. For now, log; the real toggle
                // is wired in Step 3 via a callback.
            }
        }
    });
}
```

**Important:** The `reacted` check should be "did *I* (the local user) react?", not "did the author react?". This requires passing the local username into `render_reactions`. Update the call site in `render_chat_message` to pass it — but `render_chat_message` doesn't have the username. 

**Resolution:** Change `render_message`'s signature to accept `local_username: &str`, thread it from `render_message_area` (which has `state.username`). Update the call at `chat.rs:495`:

```rust
components::render_message(ui, msg, !same_author, avatar_url, &state.username.clone());
```

And update `render_message` and `render_chat_message` signatures to accept `local_username: &str`. Then in `render_reactions`:

```rust
fn render_reactions(ui: &mut Ui, msg: &ChatMessage, local_username: &str) {
    // ...
    let reacted = users.iter().any(|u| *u == local_username);
    // ...
    if ui.add(pill).clicked() {
        // Toggle: if I reacted, remove; if not, add.
        // Emit a UiCommand via... we need cmd_tx here. This is the challenge.
    }
}
```

**Step 3: Wire reaction toggle**

The reaction toggle needs `cmd_tx` (to send `UiCommand::SendReaction`) and `access_token` (for DB persist). `components.rs` functions don't have access to `AppState`. 

**Approach:** Return a signal from `render_reactions` indicating a toggle occurred, and handle it in `chat.rs::render_message_area` which has `state`. Change `render_message` to return `Option<(String, String, bool)>` — `(message_id, emoji, active)` when a reaction toggle is clicked, `None` otherwise. In `render_message_area`, after calling `render_message`, check the return and send the `UiCommand` + spawn the DB thread.

Update `render_message` signature to:
```rust
pub fn render_message(ui: &mut Ui, msg: &ChatMessage, show_header: bool, avatar_url: Option<&str>, local_username: &str) -> Option<(String, String, bool)>
```

Return the tuple from `render_reactions` when a pill is clicked: `(msg.db_id?.clone(), emoji.to_string(), !reacted)`. If `db_id` is `None`, the click is a no-op (Phase 1 limitation — can't react to unsynced messages).

In `chat.rs::render_message_area` (line 488-499), change the loop:

```rust
            for msg in &messages {
                let is_system = msg.kind == MessageKind::System;
                let same_author = prev_author == Some(msg.author.as_str())
                    && prev_kind == Some(&msg.kind)
                    && !is_system;

                let avatar_url = avatar_map.get(msg.author.as_str()).map(String::as_str);
                let reaction_toggle = components::render_message(ui, msg, !same_author, avatar_url, &state.username);

                if let Some((message_id, emoji, active)) = reaction_toggle {
                    // Optimistic local update
                    if let Some(m) = state.messages.iter_mut().find(|m| m.db_id.as_deref() == Some(&message_id)) {
                        if active {
                            if !m.reactions.iter().any(|r| r.user == state.username && r.emoji == emoji) {
                                m.reactions.push(crate::state::Reaction { user: state.username.clone(), emoji: emoji.clone() });
                            }
                        } else {
                            m.reactions.retain(|r| !(r.user == state.username && r.emoji == emoji));
                        }
                    }
                    // Broadcast
                    if let Some(tx) = &state.cmd_tx {
                        let _ = tx.send(crate::state::UiCommand::SendReaction {
                            message_id: message_id.clone(),
                            emoji: emoji.clone(),
                            active,
                        });
                    }
                    // DB persist (fire-and-forget)
                    if let Some(ref s) = state.session {
                        let at = s.access_token.clone();
                        let user = state.username.clone();
                        thread::spawn(move || {
                            if active {
                                let _ = crate::net::supabase::insert_reaction(&at, &message_id, &user, &emoji);
                            } else {
                                let _ = crate::net::supabase::delete_reaction(&at, &message_id, &user, &emoji);
                            }
                        });
                    }
                }

                prev_author = Some(msg.author.as_str());
                prev_kind   = Some(&msg.kind);
            }
```

**Note:** This loop borrows `state.messages` immutably (via `messages = state.messages.clone()`) then mutably (via `state.messages.iter_mut()`). The `.clone()` at line 484 already handles this — the cloned `messages` is iterated for rendering, and `state.messages` is mutated for the optimistic update. This works because the clone is a separate `Vec`.

**Step 4: Add reaction picker (right-click popover)**

In `render_chat_message`, add a right-click context menu on the message content. After the content label (line 136), add:

```rust
            // Right-click → quick reaction picker
            let content_resp = ui.add(
                egui::Label::new(RichText::new(&msg.content).size(14.0).color(theme::TEXT_PRIMARY))
                    .wrap_mode(egui::TextWrapMode::Wrap),
            );
            content_resp.context_menu(|ui| {
                ui.horizontal(|ui| {
                    for emoji in QUICK_EMOJIS {
                        if ui.button(RichText::new(*emoji).size(18.0)).clicked() {
                            // Return toggle signal — same pattern as pill click.
                            // This requires the function to return the signal.
                            // For the picker, always "add" (active=true) unless already reacted.
                            ui.memory_mut(|m| m.data.insert_temp(
                                egui::Id::new("reaction_picker_result"),
                                Some((msg.db_id.clone(), emoji.to_string())),
                            ));
                        }
                    }
                });
            });
```

The picker uses egui's `context_menu` which shows a popover on right-click. The result is stored in egui's memory and read by the caller. This is a bit awkward — a cleaner approach is to have `render_message` return the picker result too. For Phase 1, the simplest approach: have `render_message` return `Option<(String, String, bool)>` that covers both pill clicks and picker selections. The picker sets `active = !already_reacted_by_me`.

**Verify:** `cargo check` passes.

**Commit:**
```bash
git add src/ui/components.rs src/ui/chat.rs
git commit -m "feat: reaction pill rendering + quick-emoji picker in message UI"
```

---

## Task 9: Web client — typing + reactions in `web/app.js`

**Files:**
- Modify: `web/app.js`

**Depends on:** Task 1 (contract.js EVENTS.TYPING/REACTION)

**Step 1: Add typing + reaction state**

Near the top of `app.js`, after the existing state declarations (around line 65-80), add:

```js
const typingUsers = {};  // username → last-ping timestamp
let lastTypingPing = 0;
```

**Step 2: Subscribe to `typing` and `reaction` events in `connectSignaling`**

In the `.on('broadcast', ...)` chain (lines 380-388), add two new listeners before `.subscribe`:

```js
    .on('broadcast', { event: EVENTS.TYPING    }, ({ payload }) => onTyping(payload))
    .on('broadcast', { event: EVENTS.REACTION  }, ({ payload }) => onReaction(payload))
```

**Step 3: Add `onTyping` handler**

After `onProfileUpdate` (line ~451), add:

```js
function onTyping({ from, is_typing }) {
  if (!from || from === myUsername) return;
  if (is_typing) {
    typingUsers[from] = Date.now();
  } else {
    delete typingUsers[from];
  }
  renderTypingBar();
}

function renderTypingBar() {
  const bar = $('typing-bar');
  const now = Date.now();
  // Expire entries older than 4 seconds
  for (const [u, ts] of Object.entries(typingUsers)) {
    if (now - ts > 4000) delete typingUsers[u];
  }
  const users = Object.keys(typingUsers);
  if (users.length === 0) {
    if (bar) bar.textContent = '';
    return;
  }
  const text = users.length === 1 ? `${users[0]} is typing…`
             : users.length === 2 ? `${users[0]} and ${users[1]} are typing…`
             : 'Several people are typing…';
  if (bar) bar.textContent = text;
}
```

**Step 4: Add typing bar element + polling**

In `index.html`, add a `<div id="typing-bar">` above the input bar. (If modifying HTML is out of scope, create it dynamically in `init()`.) In `init()`, after the chat screen is set up, add:

```js
  // Typing bar (injected above input bar if not in HTML)
  if (!$('typing-bar')) {
    const bar = document.createElement('div');
    bar.id = 'typing-bar';
    bar.className = 'typing-bar';
    $('input-bar')?.parentNode?.insertBefore(bar, $('input-bar'));
  }
  // Poll typing expiry every second
  setInterval(renderTypingBar, 1000);
```

**Step 5: Send typing pings on keystroke**

In `bindEvents`, the input keydown handler is at line 115:
```js
  input.addEventListener('keydown', e => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); trySend(); } });
```

Add an `input` event listener for typing (after line 114):
```js
  input.addEventListener('input', () => {
    ph.style.display = input.textContent.trim() ? 'none' : '';
    // Typing ping (throttled 3s)
    const now = Date.now();
    if (input.textContent.trim().length >= 2 && now - lastTypingPing > 3000) {
      lastTypingPing = now;
      bcast(EVENTS.TYPING, { from: myUsername, is_typing: true });
    }
  });
```

In `trySend` (lines 617-629), after clearing the input, send a `false` typing ping:
```js
  bcast(EVENTS.TYPING, { from: myUsername, is_typing: false });
```

**Step 6: Add `onReaction` handler**

After `onTyping`, add:

```js
function onReaction({ from, message_id, emoji, active }) {
  if (!from || from === myUsername || !message_id || !emoji) return;
  const row = document.querySelector(`[data-msg-id="${CSS.escape(message_id)}"]`);
  if (!row) return; // message not in local list — drop silently (edge case #1)
  updateReactionPills(row, from, emoji, active);
}

function updateReactionPills(row, user, emoji, active) {
  let pillsEl = row.querySelector('.msg-reactions');
  if (!pillsEl) {
    pillsEl = document.createElement('div');
    pillsEl.className = 'msg-reactions';
    row.appendChild(pillsEl);
  }
  // Rebuild pills from a per-message reaction map stored on the DOM element.
  let reactions = pillsEl._reactions ?? {};
  const key = `${user}:${emoji}`;
  if (active) {
    reactions[key] = { user, emoji };
  } else {
    delete reactions[key];
  }
  pillsEl._reactions = reactions;

  // Group by emoji, count users
  const groups = {};
  for (const { user: u, emoji: e } of Object.values(reactions)) {
    groups[e] = groups[e] || new Set();
    groups[e].add(u);
  }

  pillsEl.innerHTML = '';
  for (const [e, users] of Object.entries(groups)) {
    const pill = document.createElement('span');
    pill.className = 'reaction-pill';
    pill.textContent = `${e} ${users.size}`;
    if (users.has(myUsername)) pill.classList.add('reacted');
    pill.onclick = () => {
      const active = !users.has(myUsername);
      toggleReaction(row.dataset.msgId, e, active);
    };
    pillsEl.appendChild(pill);
  }
}
```

**Step 7: Add `toggleReaction` function**

```js
async function toggleReaction(messageId, emoji, active) {
  // Optimistic local update
  const row = document.querySelector(`[data-msg-id="${CSS.escape(messageId)}"]`);
  if (row) updateReactionPills(row, myUsername, emoji, active);

  // Broadcast
  bcast(EVENTS.REACTION, { from: myUsername, message_id: messageId, emoji, active });

  // DB persist (fire-and-forget)
  if (active) {
    const { error } = await sb.from('reactions').insert({ message_id: messageId, user: myUsername, emoji });
    if (error) console.warn('Reaction insert failed:', error.message);
  } else {
    const { error } = await sb.from('reactions')
      .delete()
      .eq('message_id', messageId).eq('user', myUsername).eq('emoji', emoji);
    if (error) console.warn('Reaction delete failed:', error.message);
  }
}
```

**Step 8: Add `data-msg-id` to rendered messages + reaction hydration in `fetchHistory`**

In `appendMsg` (lines 679-744), when creating the message group/content, add `data-msg-id` if available. This requires the message's DB UUID. Currently `fetchHistory` doesn't fetch the `id` column. Update `fetchHistory` (lines 595-615):

```js
async function fetchHistory() {
  const { data } = await sb
    .from('messages')
    .select('id, from_user, content, attachment_url, attachment_kind, attachment_filename, created_at')
    .eq('channel', DEFAULT_DB_CHANNEL)
    .order('created_at', { ascending: false })
    .limit(100);

  if (!data) return;
  lastMsgAuthor = null;
  $('messages').innerHTML = '';

  for (const row of [...data].reverse()) {
    if (!row.from_user) continue;
    const att = row.attachment_url
      ? { url: row.attachment_url, kind: row.attachment_kind || 'image', filename: row.attachment_filename || 'attachment' }
      : null;
    appendMsg(row.from_user, row.content || '', new Date(row.created_at), false, att, row.id);
  }

  // Hydrate reactions for fetched messages
  const ids = data.map(r => r.id).filter(Boolean);
  if (ids.length) {
    const { data: rxn } = await sb.from('reactions').select('message_id,user,emoji').in('message_id', ids);
    if (rxn) {
      for (const r of rxn) {
        const row = document.querySelector(`[data-msg-id="${CSS.escape(r.message_id)}"]`);
        if (row) updateReactionPills(row, r.user, r.emoji, true);
      }
    }
  }

  scrollBottom();
}
```

Update `appendMsg` signature to accept `dbId`:
```js
function appendMsg(from, content, ts = new Date(), scroll = true, attachment = null, dbId = null) {
```

In the message group creation (line 696-717), set `group.dataset.msgId = dbId` if provided:
```js
    if (dbId) group.dataset.msgId = dbId;
```

And in the `else` branch (line 719-721), the target is `container.lastElementChild` — the `data-msg-id` is already on the group.

**Step 9: Add reaction picker (right-click)**

In `appendMsg`, after the content div is appended, add a contextmenu listener:

```js
  if (content && dbId) {
    const div = target.querySelector('.msg-content:last-child') || target.lastElementChild;
    if (div) {
      div.addEventListener('contextmenu', e => {
        e.preventDefault();
        showReactionPicker(e.clientX, e.clientY, dbId, div);
      });
    }
  }
```

Add `showReactionPicker`:
```js
const QUICK_EMOJIS = ['👍', '❤️', '😂', '😮', '😢', '🙏'];

function showReactionPicker(x, y, messageId, targetEl) {
  let picker = $('reaction-picker');
  if (!picker) {
    picker = document.createElement('div');
    picker.id = 'reaction-picker';
    picker.className = 'reaction-picker';
    document.body.appendChild(picker);
  }
  picker.style.left = `${x}px`;
  picker.style.top  = `${y}px`;
  picker.style.display = 'flex';
  picker.innerHTML = '';
  for (const emoji of QUICK_EMOJIS) {
    const btn = document.createElement('span');
    btn.className = 'picker-emoji';
    btn.textContent = emoji;
    btn.onclick = () => {
      // Check if I already reacted with this emoji
      const pillsEl = targetEl.closest('[data-msg-id]')?.querySelector('.msg-reactions');
      const reactions = pillsEl?._reactions ?? {};
      const already = Object.values(reactions).some(r => r.user === myUsername && r.emoji === emoji);
      toggleReaction(messageId, emoji, !already);
      picker.style.display = 'none';
    };
    picker.appendChild(btn);
  }
  // Close on click outside
  setTimeout(() => {
    document.addEventListener('click', function close() {
      picker.style.display = 'none';
      document.removeEventListener('click', close);
    }, { once: true });
  }, 0);
}
```

**Step 10: Add CSS for reaction pills, typing bar, and picker**

Add to `web/style.css` (or inject in `init`):

```css
.typing-bar { height: 22px; padding: 2px 16px 0; font-size: 12px; color: #949ba4; }
.msg-reactions { display: flex; flex-wrap: wrap; gap: 4px; margin-top: 4px; }
.reaction-pill {
  display: inline-flex; align-items: center; gap: 4px;
  padding: 2px 8px; border-radius: 10px; font-size: 12px;
  border: 1px solid #3f4147; background: transparent; cursor: pointer;
}
.reaction-pill.reacted { background: #5865f224; border-color: #5865f2; }
.reaction-picker {
  position: fixed; z-index: 1000; display: none; gap: 4px;
  background: #2b2d31; border: 1px solid #3f4147; border-radius: 8px;
  padding: 4px; box-shadow: 0 4px 12px rgba(0,0,0,0.4);
}
.picker-emoji { font-size: 20px; cursor: pointer; padding: 4px 6px; border-radius: 4px; }
.picker-emoji:hover { background: #3f4147; }
```

**Verify:** `node --check web/app.js` passes.

**Commit:**
```bash
git add web/app.js web/style.css
git commit -m "feat: typing indicators + emoji reactions in web client"
```

---

## Task 10: Run SQL migration + integration smoke test

**Depends on:** All prior tasks

**Step 1: Run the `reactions` table SQL in the Supabase dashboard**

Open the Supabase project SQL editor and run:

```sql
CREATE TABLE IF NOT EXISTS reactions (
  message_id  UUID        NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  user        TEXT        NOT NULL,
  emoji       TEXT        NOT NULL,
  created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (message_id, user, emoji)
);
ALTER TABLE reactions ENABLE ROW LEVEL SECURITY;
CREATE POLICY rx_read   ON reactions FOR SELECT USING (true);
CREATE POLICY rx_insert ON reactions FOR INSERT TO authenticated WITH CHECK (true);
CREATE POLICY rx_delete ON reactions FOR DELETE TO authenticated USING (true);
```

**Step 2: Build and launch both clients**

```bash
cargo build --release
# Launch the native client
# In a separate browser tab, open web/index.html
```

**Step 3: Typing indicator test**

1. Log in on both clients (one native, one web) with different accounts.
2. Type in the web client's input bar (≥2 chars).
3. Confirm the native client shows "WebUser is typing…" within ~1-3s.
4. Stop typing; confirm the indicator clears after ~4s (web) or on send (native).
5. Send a message; confirm the indicator clears immediately on both.
6. Reverse: type in native, confirm web shows the indicator.

**Step 4: Reaction test**

1. Send a message from the native client.
2. On the web client, right-click the message → click 👍.
3. Confirm the `👍 1` pill appears on both clients.
4. On the native client, click the 👍 pill → confirm it toggles to `👍 2` (you added yours).
5. Click the 👍 pill again → confirm it toggles back to `👍 1` (you removed yours).
6. Reload the web client → confirm reactions re-hydrate from DB.
7. Cross-target: react from web, confirm native sees it (and vice versa).

**Step 5: Edge case test**

1. Have a third client send a `reaction` event for a `message_id` that the receiver never received (manually craft via console: `bcast(EVENTS.REACTION, {from: 'ghost', message_id: '00000000-0000-0000-0000-000000000000', emoji: '👍', active: true})`).
2. Confirm no crash, no phantom pill.

**Commit:**
```bash
git add docs/plans/2026-06-27-social-layer-phase1-implementation.md
git commit -m "docs: social layer phase 1 implementation plan + smoke test results"
```

---

## Dependency graph

```
Task 1 (contract events) ─────────┐
                                   ├─→ Task 3 (signaling) ──→ Task 4 (webrtc) ──┐
Task 2 (state model) ─────────────┤                                              ├─→ Task 7 (chat UI typing)
                                   ├─→ Task 5 (supabase) ──→ Task 6 (app.rs) ────┤
                                   │                                              ├─→ Task 8 (components UI)
                                   └─→ Task 9 (web) ──────────────────────────────┤
                                                                                  └─→ Task 10 (smoke test)
```

Parallelizable groups:
- **Wave 1:** Task 1 + Task 2 (independent, disjoint files)
- **Wave 2:** Task 3 + Task 5 + Task 9 (Task 3 needs 1+2; Task 5 needs 2; Task 9 needs 1 — all disjoint files)
- **Wave 3:** Task 4 + Task 6 (Task 4 needs 3; Task 6 needs 2+5 — disjoint files)
- **Wave 4:** Task 7 + Task 8 (Task 7 needs 4; Task 8 needs 6 — both touch chat.rs/components.rs but different functions)
- **Wave 5:** Task 10 (needs everything)
