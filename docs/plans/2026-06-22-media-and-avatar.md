# Media Attachments & Avatar Upload Fix Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use executing-plans to implement this plan task-by-task.

**Goal:** Fix the broken profile picture upload and add image/audio/video attachment support to the chat.

**Architecture:** Attachments are uploaded to Supabase Storage (existing "avatars" bucket, `chat/` prefix for media). After upload, a public URL is broadcast through the existing Supabase Realtime signaling channel as a new `chat_media` broadcast event. Receivers reconstruct a `ChatMessage` with an `Attachment` struct and render it inline. All storage I/O runs on dedicated threads; the egui UI thread only polls results.

**Tech Stack:** Rust, egui 0.34, eframe, reqwest (blocking), Supabase Storage REST API, existing Supabase Realtime WebSocket signaling, rfd (file picker), std::sync::mpsc

---

## Task 1: Add `Attachment` types and update `ChatMessage` + network enums in `state.rs`

**Files:**
- Modify: `src/state.rs`

**What to change — exact edits:**

After the `MessageKind` enum (around line 36), insert:

```rust
/// Kind of media attached to a message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AttachmentKind {
    Image,
    Audio,
    Video,
}

/// A file attached to a chat message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub url: String,
    pub kind: AttachmentKind,
    pub filename: String,
}

/// Returned by the background media-upload thread.
pub struct MediaUploadResult {
    pub url: String,
    pub kind: AttachmentKind,
    pub filename: String,
    pub caption: String,
}
```

Update `ChatMessage` struct to add:
```rust
pub attachment: Option<Attachment>,
```

Update the three `ChatMessage` constructors:
- `new_own(author, content, id, attachment: Option<Attachment>)` — set `attachment`
- `new_peer(author, content, id, attachment: Option<Attachment>)` — set `attachment`
- `new_system(content, id)` — hardcode `attachment: None` (no parameter change)

Update `NetEvent::MessageReceived` to carry:
```rust
MessageReceived { from: String, content: String, attachment: Option<Attachment> },
```

Add to `UiCommand`:
```rust
SendMedia { caption: String, url: String, kind: AttachmentKind, filename: String },
```

Add to `AppState` struct:
```rust
pub media_in_progress: bool,
pub media_rx: Option<std::sync::mpsc::Receiver<Result<MediaUploadResult, String>>>,
```

Initialize both to `false` / `None` in `AppState::default()`.

Update the three `push_*` helpers to pass `None` as attachment to the updated constructors. Add two new helpers:
```rust
pub fn push_own_media(&mut self, content: impl Into<String>, attachment: Option<Attachment>) {
    let id = self.next_id();
    let author = self.username.clone();
    self.messages.push(ChatMessage::new_own(author, content, id, attachment));
    self.scroll_to_bottom = true;
}

pub fn push_peer_media(&mut self, author: impl Into<String>, content: impl Into<String>, attachment: Option<Attachment>) {
    let id = self.next_id();
    self.messages.push(ChatMessage::new_peer(author, content, id, attachment));
    self.scroll_to_bottom = true;
}
```

**Verify:** `cargo check` passes with no new errors.

---

## Task 2: Add `invalidate` to `image_loader.rs`

**Files:**
- Modify: `src/ui/image_loader.rs`

**What to change:**

After the `get_avatar_texture` function, add:

```rust
/// Remove a URL from the texture cache, forcing a re-fetch on next access.
/// Call this after uploading a new avatar so the old texture is not reused.
pub fn invalidate(url: &str) {
    if let Some(cache) = CACHE.get() {
        if let Ok(mut map) = cache.lock() {
            map.remove(url);
        }
    }
}
```

**Verify:** `cargo check` passes.

---

## Task 3: Fix `upload_avatar` and add `upload_media` in `supabase.rs`

**Files:**
- Modify: `src/net/supabase.rs`

**What to change:**

Replace the entire `upload_avatar` function body with a PUT-first approach (PUT is the correct upsert verb for Supabase Storage) and append a `?t=<unix_timestamp>` cache-buster to the returned public URL:

```rust
pub fn upload_avatar(user_id: &str, access_token: &str, bytes: Vec<u8>, ext: &str) -> Result<String> {
    let client = Client::new();
    let filename = format!("{}_avatar.{}", user_id, ext);
    let obj_url = format!("{}/storage/v1/object/avatars/{}", BASE_URL, filename);

    let content_type = match ext.to_lowercase().as_str() {
        "png"         => "image/png",
        "jpg" | "jpeg"=> "image/jpeg",
        "gif"         => "image/gif",
        "webp"        => "image/webp",
        _             => "application/octet-stream",
    };

    // PUT = upsert; if the object doesn't exist yet Supabase still creates it.
    let res = client.put(&obj_url)
        .header("apikey", ANON_KEY)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", content_type)
        .header("x-upsert", "true")
        .body(bytes.clone())
        .send()?;

    if !res.status().is_success() {
        // Fall back to POST (for Supabase versions that don't support PUT upsert)
        let res2 = client.post(&obj_url)
            .header("apikey", ANON_KEY)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", content_type)
            .header("x-upsert", "true")
            .body(bytes)
            .send()?;

        if !res2.status().is_success() {
            let status = res2.status();
            let body = res2.text().unwrap_or_default();
            return Err(anyhow::anyhow!("Avatar upload failed ({}): {}", status, body));
        }
    }

    // Cache-buster so image_loader fetches the fresh image rather than serving
    // the old cached texture that lives at the same URL.
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    Ok(format!("{}/storage/v1/object/public/avatars/{}?t={}", BASE_URL, filename, ts))
}
```

Add the new `upload_media` function and helpers below `upload_avatar`:

```rust
/// Upload a chat media attachment. Stores under avatars/chat/{user_id}/ so only
/// one Supabase Storage bucket ("avatars") needs to exist.
pub fn upload_media(
    user_id: &str,
    access_token: &str,
    bytes: Vec<u8>,
    ext: &str,
    original_name: &str,
) -> Result<String> {
    let client = Client::new();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let safe_name: String = original_name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '.')
        .take(40)
        .collect();
    let path = format!("chat/{}/{}-{}", user_id, ts, safe_name);
    let obj_url = format!("{}/storage/v1/object/avatars/{}", BASE_URL, path);

    let content_type = mime_for_ext(ext);

    let res = client.post(&obj_url)
        .header("apikey", ANON_KEY)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", content_type)
        .body(bytes)
        .send()?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().unwrap_or_default();
        return Err(anyhow::anyhow!("Media upload failed ({}): {}", status, body));
    }

    Ok(format!("{}/storage/v1/object/public/avatars/{}", BASE_URL, path))
}

fn mime_for_ext(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "png"           => "image/png",
        "jpg" | "jpeg"  => "image/jpeg",
        "gif"           => "image/gif",
        "webp"          => "image/webp",
        "mp3"           => "audio/mpeg",
        "ogg"           => "audio/ogg",
        "wav"           => "audio/wav",
        "mp4"           => "video/mp4",
        "webm"          => "video/webm",
        "mov"           => "video/quicktime",
        _               => "application/octet-stream",
    }
}
```

**Verify:** `cargo check` passes.

---

## Task 4: Fix avatar cache invalidation in `profile.rs`

**Files:**
- Modify: `src/ui/profile.rs`

**What to change:**

In the `poll for profile picture upload result` block inside `render_modal`, find the `Ok(Some(url))` arm and add a cache invalidation call before saving:

```rust
Ok(Some(url)) => {
    if let Some(mut session) = state.session.take() {
        // Invalidate the old avatar texture so image_loader fetches the fresh one.
        if let Some(old_url) = &session.avatar_url {
            super::image_loader::invalidate(old_url);
        }
        session.avatar_url = Some(url);
        session.save();
        state.session = Some(session);
    }
}
```

**Verify:** `cargo check` passes.

---

## Task 5: Add `BroadcastMedia` to signaling in `signaling.rs`

**Files:**
- Modify: `src/net/signaling.rs`

**What to change:**

Add a new variant to `SigCmd`:
```rust
BroadcastMedia {
    caption: String,
    url: String,
    kind: String,    // "image" | "audio" | "video"
    filename: String,
},
```

In `connect_and_run`, inside the `cmd_opt = sig_cmd_rx.recv()` branch, add a new match arm after `BroadcastMessage`:

```rust
SigCmd::BroadcastMedia { caption, url, kind, filename } => {
    let topic = format!("realtime:{}", CHANNEL);
    let broadcast = make_broadcast(&topic, "chat_media", json!({
        "from": username,
        "content": caption,
        "url": url,
        "kind": kind,
        "filename": filename,
    }), &mut ref_count);
    send_text(&mut ws_stream, &broadcast).await?;
}
```

In `handle_incoming`, add a new match arm inside the `"broadcast"` handler after `"chat_message"`:

```rust
"chat_media" => {
    let content  = b_payload["content"].as_str().unwrap_or("").to_string();
    let url      = b_payload["url"].as_str().unwrap_or("").to_string();
    let kind_str = b_payload["kind"].as_str().unwrap_or("image");
    let filename = b_payload["filename"].as_str().unwrap_or("attachment").to_string();

    let kind = match kind_str {
        "audio" => crate::state::AttachmentKind::Audio,
        "video" => crate::state::AttachmentKind::Video,
        _       => crate::state::AttachmentKind::Image,
    };
    let attachment = if url.is_empty() { None } else {
        Some(crate::state::Attachment { url, kind, filename })
    };
    let _ = net_tx.send(NetEvent::MessageReceived {
        from: from.to_string(),
        content,
        attachment,
    });
    ctx.request_repaint();
}
```

Also update the existing `"chat_message"` arm to match the updated `MessageReceived` shape:
```rust
"chat_message" => {
    if let Some(content) = b_payload["content"].as_str() {
        let _ = net_tx.send(NetEvent::MessageReceived {
            from: from.to_string(),
            content: content.to_string(),
            attachment: None,
        });
        ctx.request_repaint();
    }
}
```

**Verify:** `cargo check` passes.

---

## Task 6: Wire `UiCommand::SendMedia` through `webrtc.rs`

**Files:**
- Modify: `src/net/webrtc.rs`

**What to change:**

In the `cmd = cmd_rx.recv()` branch, inside the `match cmd` block, add a new arm before the `_ => {}` catch-all:

```rust
UiCommand::SendMedia { caption, url, kind, filename } => {
    let kind_str = match kind {
        crate::state::AttachmentKind::Image => "image",
        crate::state::AttachmentKind::Audio => "audio",
        crate::state::AttachmentKind::Video => "video",
    };
    let _ = sig_cmd_tx.send(crate::net::signaling::SigCmd::BroadcastMedia {
        caption,
        url,
        kind: kind_str.to_string(),
        filename,
    });
}
```

**Verify:** `cargo check` passes.

---

## Task 7: Update `apply_net_event` in `app.rs`

**Files:**
- Modify: `src/app.rs`

**What to change:**

The `NetEvent::MessageReceived` arm now carries `attachment`. Update it:

```rust
NetEvent::MessageReceived { from, content, attachment } => {
    self.push_peer_media(from, content, attachment);
}
```

**Verify:** `cargo check` passes.

---

## Task 8: Add attachment rendering to `components.rs`

**Files:**
- Modify: `src/ui/components.rs`

**What to change:**

At the top of the file, add to the existing `use egui::{...}` import: ensure `Stroke` is imported (it may already be via theme). Also import `egui::load::SizedTexture`.

After the closing brace of `render_chat_message`, add:

```rust
fn render_attachment(ui: &mut Ui, attachment: &crate::state::Attachment) {
    ui.add_space(4.0);
    match attachment.kind {
        crate::state::AttachmentKind::Image => {
            let tex = super::image_loader::get_avatar_texture(ui.ctx(), &attachment.url);
            if let Some(tex) = tex {
                let nat = tex.size_vec2();
                let max_w = ui.available_width().min(420.0);
                let scale = if nat.x > 0.0 { (max_w / nat.x).min(1.0) } else { 1.0 };
                let mut display = nat * scale;
                if display.y > 320.0 {
                    display = display * (320.0 / display.y);
                }
                let sized = egui::load::SizedTexture::new(tex.id(), display);
                ui.add(egui::Image::new(sized));
            } else {
                ui.label(
                    RichText::new("⏳ Loading image…")
                        .size(13.0)
                        .color(theme::TEXT_MUTED),
                );
            }
        }
        crate::state::AttachmentKind::Audio => {
            egui::Frame::default()
                .fill(theme::ELEVATED_BG)
                .corner_radius(CornerRadius::same(8u8))
                .inner_margin(egui::Margin::symmetric(12i8, 8i8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("🎵").size(16.0));
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(&attachment.filename)
                                .size(13.0)
                                .color(theme::TEXT_PRIMARY),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Open").clicked() {
                                open_externally(&attachment.url);
                            }
                        });
                    });
                });
        }
        crate::state::AttachmentKind::Video => {
            egui::Frame::default()
                .fill(theme::ELEVATED_BG)
                .corner_radius(CornerRadius::same(8u8))
                .inner_margin(egui::Margin::symmetric(12i8, 8i8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("🎬").size(16.0));
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(&attachment.filename)
                                .size(13.0)
                                .color(theme::TEXT_PRIMARY),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Open").clicked() {
                                open_externally(&attachment.url);
                            }
                        });
                    });
                });
        }
    }
}

fn open_externally(url: &str) {
    #[cfg(target_os = "windows")]
    { let _ = std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn(); }
    #[cfg(target_os = "macos")]
    { let _ = std::process::Command::new("open").arg(url).spawn(); }
    #[cfg(target_os = "linux")]
    { let _ = std::process::Command::new("xdg-open").arg(url).spawn(); }
}
```

In `render_chat_message`, inside `ui.vertical(|ui| { ... })`, after the `ui.add(egui::Label::new(...))` for the message text, add:

```rust
if let Some(ref att) = msg.attachment {
    render_attachment(ui, att);
}
```

**Verify:** `cargo check` passes.

---

## Task 9: Add attachment button and upload polling to `chat.rs`

**Files:**
- Modify: `src/ui/chat.rs`

**What to change:**

**Step A — Poll media upload at top of `render()`:**

At the very top of `pub fn render(ctx, state)`, before the panel declarations, add a call:

```rust
poll_media_upload(ctx, state);
```

Add the `poll_media_upload` free function (at the bottom of `chat.rs`):

```rust
fn poll_media_upload(ctx: &egui::Context, state: &mut AppState) {
    let result = state.media_rx.as_ref().and_then(|rx| rx.try_recv().ok());
    if let Some(result) = result {
        state.media_in_progress = false;
        state.media_rx = None;
        match result {
            Ok(r) => {
                let att = crate::state::Attachment {
                    url: r.url.clone(),
                    kind: r.kind.clone(),
                    filename: r.filename.clone(),
                };
                // Optimistic local display
                state.push_own_media(r.caption.clone(), Some(att.clone()));
                // Broadcast to peers
                if let Some(tx) = &state.cmd_tx {
                    let _ = tx.send(crate::state::UiCommand::SendMedia {
                        caption: r.caption,
                        url: att.url,
                        kind: att.kind,
                        filename: att.filename,
                    });
                }
            }
            Err(e) => {
                state.push_system(format!("⚠ Media upload failed: {}", e));
            }
        }
        ctx.request_repaint();
    }
}
```

**Step B — Rewrite `render_input_bar`** to include the 📎 button inside the input frame:

```rust
fn render_input_bar(ctx: &egui::Context, ui: &mut egui::Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        Frame::default()
            .fill(theme::INPUT_BG)
            .corner_radius(CornerRadius::same(8u8))
            .inner_margin(Margin { left: 10, right: 14, top: 10, bottom: 10 })
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    // ── Attachment button ────────────────────────────────────
                    if state.media_in_progress {
                        ui.spinner();
                    } else {
                        let attach = ui.add(
                            egui::Button::new(
                                RichText::new("📎").size(16.0).color(theme::TEXT_MUTED)
                            )
                            .fill(Color32::TRANSPARENT)
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(CornerRadius::same(4u8)),
                        );
                        if attach.hovered() {
                            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if attach.clicked() {
                            pick_and_upload_media(state, ctx);
                        }
                    }
                    ui.add_space(6.0);

                    // ── Text input ───────────────────────────────────────────
                    let input_id = egui::Id::new("message_input_field");
                    let avail_w  = ui.available_width();
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut state.message_input)
                            .id(input_id)
                            .hint_text("Message #general…")
                            .desired_width(avail_w)
                            .font(egui::FontId::proportional(15.0))
                            .frame(egui::Frame::NONE),
                    );
                    if response.lost_focus() && ctx.input(|i| i.key_pressed(Key::Enter)) {
                        try_send_message(state);
                        ctx.memory_mut(|m| m.request_focus(input_id));
                    }
                });
            });
    });
}
```

**Step C — Add `pick_and_upload_media` free function:**

```rust
fn pick_and_upload_media(state: &mut AppState, ctx: &egui::Context) {
    let session = match &state.session {
        Some(s) => s.clone(),
        None => return,
    };

    let path = rfd::FileDialog::new()
        .add_filter("Images",    &["png", "jpg", "jpeg", "gif", "webp"])
        .add_filter("Audio",     &["mp3", "ogg", "wav"])
        .add_filter("Video",     &["mp4", "webm", "mov"])
        .add_filter("All Media", &["png", "jpg", "jpeg", "gif", "webp",
                                    "mp3", "ogg", "wav", "mp4", "webm", "mov"])
        .pick_file();

    let path = match path {
        Some(p) => p,
        None => return,
    };

    state.media_in_progress = true;

    let caption = state.message_input.trim().to_string();
    state.message_input.clear();

    let (tx, rx) = std::sync::mpsc::channel();
    state.media_rx = Some(rx);
    let ctx_clone = ctx.clone();

    std::thread::spawn(move || {
        let ext      = path.extension().and_then(|e| e.to_str()).unwrap_or("bin").to_string();
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("attachment").to_string();
        let kind     = kind_for_ext(&ext);

        let result = std::fs::read(&path)
            .map_err(|e| format!("Failed to read file: {}", e))
            .and_then(|bytes| {
                crate::net::supabase::upload_media(
                    &session.user_id,
                    &session.access_token,
                    bytes,
                    &ext,
                    &filename,
                )
                .map_err(|e| e.to_string())
            })
            .map(|url| crate::state::MediaUploadResult { url, kind, filename, caption });

        let _ = tx.send(result);
        ctx_clone.request_repaint();
    });
}

fn kind_for_ext(ext: &str) -> crate::state::AttachmentKind {
    match ext.to_lowercase().as_str() {
        "mp3" | "ogg" | "wav" | "flac" | "aac" => crate::state::AttachmentKind::Audio,
        "mp4" | "webm" | "mov" | "avi" | "mkv"  => crate::state::AttachmentKind::Video,
        _                                        => crate::state::AttachmentKind::Image,
    }
}
```

**Important — update imports at top of `chat.rs`:**

The existing import line is:
```rust
use egui::{Color32, CornerRadius, Frame, Key, Margin, RichText, ScrollArea, Vec2};
```

It already has what we need. Ensure `Color32`, `CornerRadius`, `Frame`, `Key`, `Margin`, `RichText` are all present (they are).

**Verify:** `cargo check` and then `cargo build` both pass.

---

## Task 10: Full build + smoke test

**Steps:**

1. Run `cargo build --release` and confirm zero errors, only pre-existing warnings.
2. Launch the app (`cargo run`), log in.
3. **Avatar fix:** Open profile modal → click avatar → pick an image → confirm the new avatar appears (not the old cached one) and no error label shows.
4. **Image attachment:** Click 📎 in the chat input → pick a PNG/JPG → confirm a loading spinner appears → confirm the image renders inline in the message list for both sender and receiver.
5. **Audio attachment:** Click 📎 → pick an MP3 → confirm the audio chip with "Open" button appears.
6. **Video attachment:** Click 📎 → pick an MP4 → confirm the video chip with "Open" button appears.
7. Confirm the "Open" button on audio/video chips launches the system player.

**Commit:**

```
git add src/state.rs src/ui/image_loader.rs src/net/supabase.rs \
        src/ui/profile.rs src/net/signaling.rs src/net/webrtc.rs \
        src/app.rs src/ui/components.rs src/ui/chat.rs
git commit -m "feat: media attachments in chat + fix avatar upload upsert and cache invalidation"
```

---

## Supabase Storage Setup Note

The "avatars" bucket is shared for both avatar images and chat media (under the `chat/` path prefix). If uploads fail with "Bucket not found", create the bucket in the Supabase dashboard:

1. Open your project → Storage → New bucket → Name: `avatars` → Public: ✓
2. Add an RLS policy: allow authenticated users to INSERT into `avatars`.

No second bucket is required.
