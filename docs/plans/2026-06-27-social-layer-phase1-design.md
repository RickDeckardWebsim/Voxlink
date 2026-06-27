# VoxLink Social Layer — Phase 1: Typing Indicators & Emoji Reactions Design

> **For Claude:** REQUIRED SUB-SKILL: Use writing-plans to create the implementation plan from this design.

**Goal:** Make VoxLink chat feel responsive and expressive by adding typing indicators and emoji reactions, within the hard constraints: no server, no cost, lightweight, cross-OS (Windows/Linux/web; Mac deferred), ~50 concurrent users.

**Architecture:** Both features ride existing infrastructure — Supabase free-tier Postgres for reaction persistence, Supabase Realtime broadcasts for live typing and reaction events, the existing `messages.id` UUID as the join key. No new services, no new storage buckets, no backend code. Reactions are a side table to `messages`; typing is ephemeral (no persistence). The shared contract (`docs/plans/2026-06-27-message-contract.md`) gains two new broadcast events.

**Tech Stack:** Rust/egui (native), vanilla JS (web), Supabase PostgREST + Realtime, existing `src/net/contract.rs` + `web/contract.js` contract modules.

---

## 1. Backend: data model

### `reactions` table (new)

```sql
CREATE TABLE IF NOT EXISTS reactions (
  message_id  UUID        NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  user        TEXT        NOT NULL,            -- username (parity with messages.from_user)
  emoji       TEXT        NOT NULL,            -- single grapheme, e.g. "👍"
  created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (message_id, user, emoji)        -- one user, one emoji, per message
);
ALTER TABLE reactions ENABLE ROW LEVEL SECURITY;
CREATE POLICY rx_read   ON reactions FOR SELECT USING (true);
CREATE POLICY rx_insert ON reactions FOR INSERT TO authenticated WITH CHECK (true);
CREATE POLICY rx_delete ON reactions FOR DELETE TO authenticated USING (true);
```

- **Composite PK** makes toggling idempotent: INSERT adds, DELETE removes, no UPDATE path.
- **`ON DELETE CASCADE`** so reactions die with their message (consistent with spec §3).
- **`user` is a username string** (parity with `messages.from_user`). Delete RLS is permissive (`USING (true)`) to match the existing `messages` pattern; actual "only delete your own" enforcement happens in the REST call via the access token, not RLS. Keeps the schema consistent with the existing permissive-RLS approach.
- **Typing needs no table** — it is ephemeral.

**No changes to the `messages` table.** Reactions are a side table; typing is transient. The existing message flow is untouched.

### Storage

Reactions are pure DB rows — no file storage, no new bucket. Zero new storage cost.

---

## 2. Wire protocol (contract additions)

Two new Realtime broadcast events added to `contract.rs::event` and `contract.js::EVENTS`, and documented in spec §4.

### `typing` (ephemeral, no persistence)

```json
{ "from": "<username>", "is_typing": true }
```

- **Send:** on input-bar keystroke if ≥2 chars typed and no ping sent in the last 3s (throttle). Send `{is_typing: false}` on send/clear.
- **Recv:** add/remove `from` in `typing_users`; ignore `from == self`. Auto-expire each entry after 4s if no refresh (receiver-side timer in the UI loop).
- **Throttle is mandatory:** at 50 users, unthrottled keystroke broadcasts would flood the channel. Send rate capped at 1 ping / 3s per burst; receivers auto-expire after 4s. Bandwidth bound: ~0.3 pings/user/sec at peak — trivial on a Chromebook over mobile.
- **Render:** "Alice is typing…", "Alice and Bob are typing…", "Several people are typing…" in a thin bar above the input.

### `reaction` (mirrors a DB row mutation)

```json
{ "from": "<username>", "message_id": "<uuid>", "emoji": "👍", "active": true }
```

- **Send:** on toggle. `active:true` = added (INSERT into `reactions`, fire-and-forget); `active:false` = removed (DELETE). One event per toggle.
- **Recv:** find the message by `message_id` in the local list; add/remove `(from, emoji)` to its `reactions`. Ignore `from == self` *for the broadcast* — we already applied it optimistically locally (same pattern as `chat_message`).
- **History hydration:** on connect, after `fetch_recent_messages`, fire `fetch_reactions(message_ids)` — bulk query `SELECT message_id, user, emoji FROM reactions WHERE message_id = ANY($1)` — and merge into each `ChatMessage.reactions` before first render. One extra DB round-trip on connect; cached for the session.

---

## 3. State model & UI rendering

### State additions

`ChatMessage` gains a `reactions` field:

```rust
// src/state.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reaction {
    pub user: String,
    pub emoji: String,
}

pub struct ChatMessage {
    // ... existing fields ...
    #[serde(default)]
    pub reactions: Vec<Reaction>,   // empty for old history rows
}
```

`#[serde(default)]` keeps history rows without reactions deserializing cleanly. `Vec<Reaction>` (not `HashMap`) because render order matters and dedup is enforced by the DB composite PK; the client appends/removes by `(user, emoji)`.

One new `AppState` field for typing:

```rust
pub typing_users: Vec<String>,   // usernames currently typing (excluding self)
```

Web: `const typingUsers = {}` (object keyed by username → last-ping timestamp).

### UI rendering (both targets)

**Typing indicator** — a thin bar (~20px) above the input bar, muted text. egui: a `TopBottomPanel::bottom` slotted *above* the existing input `TopBottomPanel`. Web: a `#typing-bar` div above `#input-bar`. Text: 1 user → "Alice is typing…", 2 → "Alice and Bob are typing…", 3+ → "Several people is typing…". No avatars (keep it cheap). Auto-expiry driven by the UI loop checking timestamps each frame.

**Reactions** — rendered **below** the message bubble, left-aligned for peer messages, right-aligned for own messages (matching bubble alignment). A row of small pill chips: `👍 3` where `3` is the count of *distinct users*. egui: small `Frame` chips with emoji + count, clickable to toggle (if I've reacted, clicking removes; if not, adds). Web: `<span class="reaction-pill">` with a click handler. Distinct users, not duplicate emojis — composite PK means one emoji per user per message, so `👍 3` = three different people reacted 👍.

**Reaction picker** — on right-click (web) or right-click (egui) of a message, show a small popover with ~6 quick emojis: 👍 ❤️ 😂 😮 😢 🙏 (the Discord-default set, no custom picker to keep scope tight). Clicking one toggles it. No full emoji keyboard (YAGNI for Phase 1).

**History hydration** — on `NetEvent::Connected` (native) / `enterChat` (web), after `fetch_recent_messages`, fire `fetch_reactions(message_ids)` and merge into each `ChatMessage.reactions` before the first render.

**Optimistic apply** (same pattern as messages): on toggle, update local `ChatMessage.reactions` immediately, broadcast the `reaction` event, fire-and-forget the DB INSERT/DELETE. Sender never receives own broadcast, so no double-application.

---

## 4. Error handling, edge cases & verification

### Error handling

DB failures are non-fatal (existing pattern). Reaction INSERT/DELETE is fire-and-forget like `insert_message` — failures log a warning, never block the UI. The optimistic local state already applied; worst case the reaction vanishes on next history fetch. Typing is ephemeral with no DB path — its only failure mode is a dropped broadcast (receivers don't see the indicator — harmless).

### Edge cases

1. **Message ID mismatch** — a `reaction` event references a `message_id` not in the local list (message broadcast not yet received, or history fetch in flight). **Resolution:** drop the reaction silently; it re-hydrates on the next history+reactions fetch. No partial state, no orphan reactions. Matches how `chat_message` broadcasts already work.

2. **Race: reaction before its message** — covered by #1. Dropped, re-hydrated later. The `reaction` event is never the source of truth for a message existing; `messages` is.

3. **Duplicate reactions** — composite PK prevents server-side duplicates. Client-side, toggling checks "have I already reacted with this emoji?" — if yes, the toggle removes instead of adding. The button is always a toggle, never add-only.

4. **Typing flood / stuck typing** — three guards: (a) send-side throttle (1 ping / 3s), (b) receiver auto-expire after 4s, (c) `false` ping on send/clear. A crashed user's `true` ping is cleared by the receiver's 4s timer — no stuck "Alice is typing…" forever. The receiver-side expiry timer is mandatory, not optional.

5. **Own typing display** — never show my own indicator. Filter `from == self` on recv (native: `if from == username { return; }`; web: `self:false` + explicit `if (from === myUsername) return;`).

6. **Reactions on own messages** — allowed. No special-casing.

7. **Reactions from users who left** — reactions persist in the DB; a disconnected user's reaction still shows in history fetch (we store username, not a live presence check). Correct — a reaction is a recorded expression, not a presence signal.

### Verification

- `cargo check` native, `node --check` web — compile/parse gates.
- **Typing:** two clients (one native, one web) — type in one, confirm the indicator appears in the other within ~1s; stop typing, confirm it clears after 4s; send a message, confirm it clears immediately.
- **Reactions:** react to a message, confirm the pill appears on both clients; react again with the same emoji, confirm it toggles off; react with a different emoji, confirm both pills show; reload the receiving client, confirm reactions re-hydrate from history.
- **Cross-target:** native user reacts, web user sees it (and vice versa) — validates the contract event names match.
- **Edge #1:** a third client sends a `reaction` for a `message_id` the receiver never received — confirm no crash, no phantom pill.

**Testing constraint:** no mocks. Integration tests run against the real Supabase project (the `reactions` table must exist in the dashboard — SQL provided in §1). If live integration tests against production Supabase are undesirable, compile/parse gates + code review are the floor; live behavior verified manually.

---

## 5. Scope boundaries

**In scope (Phase 1):** typing indicators, emoji reactions, reaction picker (6 quick emojis), history hydration of reactions, two new contract events.

**Out of scope (deferred to later phases):** @mentions (Phase 1 candidate, dropped as borderline YAGNI at 50 users), message edit/delete (heavier — touches message model + history path), custom emoji picker, reaction counts by distinct-emoji aggregation UI, multi-channel, DMs, presence migration, rich status.

**Constraints honored:** no server, no cost (free-tier Supabase only), lightweight (throttled broadcasts, ~0.3 pings/user/sec peak), cross-OS (native Windows/Linux + web, Mac deferred), no-build-step web preserved.
