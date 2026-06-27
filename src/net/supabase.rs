use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::net::contract;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub user: User,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub username: String,
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub description: String,
}

/// Exchange a refresh token for a fresh access + refresh token pair.
/// Call this on startup when a saved session is found; Supabase access tokens
/// expire after 1 hour by default.
pub fn refresh_session(refresh_token: &str) -> Result<(String, String)> {
    let client = Client::new();
    let url = format!("{}/auth/v1/token?grant_type=refresh_token", contract::SUPABASE_URL);

    let res = client.post(&url)
        .header("apikey", contract::SUPABASE_ANON_KEY)
        .header("Content-Type", "application/json")
        .json(&json!({ "refresh_token": refresh_token }))
        .send()?;

    if !res.status().is_success() {
        let body = res.text().unwrap_or_default();
        return Err(anyhow::anyhow!("Token refresh failed: {}", body));
    }

    let parsed: serde_json::Value = res.json()?;
    let access  = parsed["access_token"].as_str()
        .ok_or_else(|| anyhow::anyhow!("No access_token in refresh response"))?
        .to_string();
    let refresh = parsed["refresh_token"].as_str()
        .unwrap_or(refresh_token)
        .to_string();
    Ok((access, refresh))
}

pub fn sign_up(email: &str, password: &str, username: &str) -> Result<AuthResponse> {
    let client = Client::new();
    let url = format!("{}/auth/v1/signup?apikey={}", contract::SUPABASE_URL, contract::SUPABASE_ANON_KEY);
    
    // 1. Sign up user
    let res = client.post(&url)
        .header("apikey", contract::SUPABASE_ANON_KEY)
        .header("Content-Type", "application/json")
        .json(&json!({
            "email": email,
            "password": password
        }))
        .send()?
        .error_for_status()?;
        
    let text = res.text()?;
    
    let parsed: serde_json::Value = serde_json::from_str(&text)?;
    
    let mut access_token = String::new();
    let mut refresh_token = String::new();
    let mut user_id = String::new();
    let mut user_email = String::new();
    
    if let Some(session_token) = parsed.get("access_token").and_then(|v| v.as_str()) {
        access_token = session_token.to_string();
        refresh_token = parsed.get("refresh_token").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if let Some(user_obj) = parsed.get("user") {
            user_id = user_obj.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            user_email = user_obj.get("email").and_then(|v| v.as_str()).unwrap_or("").to_string();
        }
    } else {
        // It's just a User object (Email confirmation required)
        user_id = parsed.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        user_email = parsed.get("email").and_then(|v| v.as_str()).unwrap_or("").to_string();
    }
    
    if user_id.is_empty() {
        return Err(anyhow::anyhow!("Unexpected response from Supabase Auth: {}", text));
    }
    
    let auth_res = AuthResponse {
        access_token: access_token.clone(),
        refresh_token,
        user: User {
            id: user_id.clone(),
            email: user_email,
        }
    };
    
    // If we didn't get an access token, we can't insert into profiles yet because RLS requires auth.
    // BUT wait, if email confirmations are required, they can't log in immediately!
    // We should return an error asking them to confirm their email, OR if we have a service key we could bypass it.
    // If access_token is empty, let's just return an error to the UI telling them to check their email.
    if access_token.is_empty() {
        return Err(anyhow::anyhow!("Please check your email to confirm your account before logging in."));
    }
    
    // 2. Insert into profiles table
    let profiles_url = format!("{}/rest/v1/profiles", contract::SUPABASE_URL);
    client.post(&profiles_url)
        .header("apikey", contract::SUPABASE_ANON_KEY)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", "application/json")
        .header("Prefer", "return=minimal")
        .json(&json!({
            "id": user_id,
            "username": username,
        }))
        .send()?
        .error_for_status()?;

    Ok(auth_res)
}

pub fn sign_in(email: &str, password: &str) -> Result<AuthResponse> {
    let client = Client::new();
    let url = format!("{}/auth/v1/token?grant_type=password&apikey={}", contract::SUPABASE_URL, contract::SUPABASE_ANON_KEY);
    
    let res = client.post(&url)
        .header("apikey", contract::SUPABASE_ANON_KEY)
        .header("Content-Type", "application/json")
        .json(&json!({
            "email": email,
            "password": password
        }))
        .send()?;
        
    if !res.status().is_success() {
        let err_text = res.text().unwrap_or_default();
        return Err(anyhow::anyhow!("Login failed: {}", err_text));
    }
        
    let auth_res: AuthResponse = res.json()?;
    Ok(auth_res)
}

pub fn get_profile(user_id: &str, access_token: &str) -> Result<Profile> {
    let client = Client::new();
    let url = format!("{}/rest/v1/profiles?id=eq.{}&select=*", contract::SUPABASE_URL, user_id);
    
    let res = client.get(&url)
        .header("apikey", contract::SUPABASE_ANON_KEY)
        .header("Authorization", format!("Bearer {}", access_token))
        .send()?
        .error_for_status()?;
        
    let profiles: Vec<Profile> = res.json()?;
    profiles.into_iter().next().context("Profile not found")
}

pub fn update_profile(user_id: &str, access_token: &str, username: &str, avatar_url: Option<&str>, description: &str) -> Result<()> {
    let client = Client::new();
    let url = format!("{}/rest/v1/profiles?id=eq.{}&apikey={}", contract::SUPABASE_URL, user_id, contract::SUPABASE_ANON_KEY);

    let mut body = json!({
        "username": username,
        "description": description,
    });

    if let Some(url) = avatar_url {
        body["avatar_url"] = json!(url);
    }

    client.patch(&url)
        .header("apikey", contract::SUPABASE_ANON_KEY)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", "application/json")
        .header("Prefer", "return=minimal")
        .json(&body)
        .send()?
        .error_for_status()?;

    Ok(())
}

// ── Error helpers ─────────────────────────────────────────────────────────────

/// Extract a human-readable message from a Supabase JSON error body.
fn parse_supabase_error(body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        for key in &["message", "error_description", "error", "msg"] {
            if let Some(s) = v[key].as_str() {
                // Translate known technical messages into user-friendly ones
                return if s.contains("exp") && s.contains("claim") {
                    "Session expired — the app will refresh your credentials automatically.".to_string()
                } else if s.contains("JWT") || s.contains("token") {
                    "Authentication token invalid — please sign out and back in.".to_string()
                } else {
                    s.to_string()
                };
            }
        }
    }
    if body.len() > 200 { format!("{}…", &body[..200]) } else { body.to_string() }
}

/// Returns true when an error string indicates a stale / invalid JWT.
fn is_auth_error(msg: &str) -> bool {
    msg.contains("401")
        || msg.contains("403")
        || msg.contains("Unauthorized")
        || msg.contains("exp")
        || msg.contains("JWT")
        || msg.contains("expired")
}

// ── Storage uploads ───────────────────────────────────────────────────────────

pub fn upload_avatar(user_id: &str, access_token: &str, bytes: Vec<u8>, ext: &str) -> Result<String> {
    let client = Client::new();
    let filename = format!("{}_avatar.{}", user_id, ext);
    let obj_url  = format!("{}/storage/v1/object/avatars/{}", contract::SUPABASE_URL, filename);

    let content_type = contract::mime_for_ext(ext);

    // PUT = standard upsert verb for Supabase Storage.
    let res = client.put(&obj_url)
        .header("apikey", contract::SUPABASE_ANON_KEY)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", content_type)
        .header("x-upsert", "true")
        .body(bytes.clone())
        .send()?;

    if !res.status().is_success() {
        // Older Supabase Storage versions: fall back to POST with x-upsert.
        let res2 = client.post(&obj_url)
        .header("apikey", contract::SUPABASE_ANON_KEY)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", content_type)
            .header("x-upsert", "true")
            .body(bytes)
            .send()?;

        if !res2.status().is_success() {
            let body = res2.text().unwrap_or_default();
            return Err(anyhow::anyhow!("{}", parse_supabase_error(&body)));
        }
    }

    // Append a timestamp so image_loader re-fetches instead of serving the cached texture.
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    Ok(format!("{}/storage/v1/object/public/avatars/{}?t={}", contract::SUPABASE_URL, filename, ts))
}

/// Upload a chat media attachment. Files are stored under `avatars/chat/{user_id}/` so only
/// one bucket ("avatars") needs to be configured in Supabase.
pub fn upload_media(
    user_id: &str,
    access_token: &str,
    bytes: Vec<u8>,
    ext: &str,
    original_name: &str,
) -> Result<String> {
    let client = Client::new();
    let ts: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let safe_name: String = original_name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '.')
        .take(40)
        .collect();
    let path    = format!("chat/{}/{}-{}", user_id, ts, safe_name);
    let obj_url = format!("{}/storage/v1/object/avatars/{}", contract::SUPABASE_URL, path);

    let res = client.post(&obj_url)
        .header("apikey", contract::SUPABASE_ANON_KEY)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", contract::mime_for_ext(ext))
        .body(bytes)
        .send()?;

    if !res.status().is_success() {
        let body = res.text().unwrap_or_default();
        return Err(anyhow::anyhow!("{}", parse_supabase_error(&body)));
    }

    Ok(format!("{}/storage/v1/object/public/avatars/{}", contract::SUPABASE_URL, path))
}

/// Upload avatar with automatic JWT refresh on auth failure.
/// Returns `(public_url, new_tokens)` — if `new_tokens` is `Some`, the caller must
/// persist the new `(access_token, refresh_token)` pair back to `Session` and disk.
pub fn upload_avatar_auto_refresh(
    user_id: &str,
    access_token: &str,
    refresh_token: &str,
    bytes: Vec<u8>,
    ext: &str,
) -> Result<(String, Option<(String, String)>)> {
    match upload_avatar(user_id, access_token, bytes.clone(), ext) {
        Ok(url) => Ok((url, None)),
        Err(e) if is_auth_error(&e.to_string()) => {
            let (new_at, new_rt) = refresh_session(refresh_token)
                .map_err(|_| anyhow::anyhow!("Session expired. Please sign out and back in."))?;
            let url = upload_avatar(user_id, &new_at, bytes, ext)?;
            Ok((url, Some((new_at, new_rt))))
        }
        Err(e) => Err(e),
    }
}

/// Upload media with automatic JWT refresh on auth failure.
/// Returns `(public_url, new_tokens)` — same contract as `upload_avatar_auto_refresh`.
pub fn upload_media_auto_refresh(
    user_id: &str,
    access_token: &str,
    refresh_token: &str,
    bytes: Vec<u8>,
    ext: &str,
    original_name: &str,
) -> Result<(String, Option<(String, String)>)> {
    match upload_media(user_id, access_token, bytes.clone(), ext, original_name) {
        Ok(url) => Ok((url, None)),
        Err(e) if is_auth_error(&e.to_string()) => {
            let (new_at, new_rt) = refresh_session(refresh_token)
                .map_err(|_| anyhow::anyhow!("Session expired. Please sign out and back in."))?;
            let url = upload_media(user_id, &new_at, bytes, ext, original_name)?;
            Ok((url, Some((new_at, new_rt))))
        }
        Err(e) => Err(e),
    }
}

// ── Chat message persistence ──────────────────────────────────────────────────
//
// Required Supabase table (run once in the SQL editor):
//
//   CREATE TABLE IF NOT EXISTS messages (
//     id               UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
//     channel          TEXT        NOT NULL DEFAULT 'general',
//     from_user        TEXT        NOT NULL,
//     content          TEXT        NOT NULL DEFAULT '',
//     attachment_url   TEXT,
//     attachment_kind  TEXT,
//     attachment_filename TEXT,
//     created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
//   );
//   ALTER TABLE messages ENABLE ROW LEVEL SECURITY;
//   CREATE POLICY "msg_read"   ON messages FOR SELECT USING (true);
//   CREATE POLICY "msg_insert" ON messages FOR INSERT TO authenticated WITH CHECK (true);
//   CREATE INDEX messages_channel_time ON messages (channel, created_at DESC);

#[derive(Deserialize)]
struct DbMessage {
    id:                  String,
    from_user:           String,
    content:             String,
    attachment_url:      Option<String>,
    attachment_kind:     Option<String>,
    attachment_filename: Option<String>,
    created_at:          String,
}

/// Insert a sent message into the `messages` table (fire-and-forget).
/// Failures are logged as warnings; they don't block the local send.
pub fn insert_message(
    access_token: &str,
    from_user: &str,
    content: &str,
    attachment: Option<&crate::state::Attachment>,
) -> Result<()> {
    let client  = Client::new();
    let url     = format!("{}/rest/v1/messages", contract::SUPABASE_URL);

    let mut body = json!({
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

    let res = client.post(&url)
        .header("apikey",         contract::SUPABASE_ANON_KEY)
        .header("Authorization",  format!("Bearer {}", access_token))
        .header("Content-Type",   "application/json")
        .header("Prefer",         "return=minimal")
        .json(&body)
        .send()?;

    if !res.status().is_success() {
        let err = res.text().unwrap_or_default();
        log::warn!("[supabase] message insert failed: {}", err);
    }
    Ok(())
}

/// Fetch the 100 most-recent messages from the `messages` table.
/// Returns them in chronological order (oldest first).
pub fn fetch_recent_messages(
    access_token: &str,
    local_username: &str,
) -> Result<Vec<crate::state::ChatMessage>> {
    let client = Client::new();
    let url = format!(
        "{}/rest/v1/messages?select=id,from_user,content,attachment_url,attachment_kind,attachment_filename,created_at&channel=eq.{}&order=created_at.desc&limit=100",
        contract::SUPABASE_URL,
        contract::DEFAULT_DB_CHANNEL
    );

    let res = client.get(&url)
        .header("apikey",        contract::SUPABASE_ANON_KEY)
        .header("Authorization", format!("Bearer {}", access_token))
        .send()?;

    if !res.status().is_success() {
        return Err(anyhow::anyhow!(
            "History fetch failed ({})", res.status()
        ));
    }

    let rows: Vec<DbMessage> = res.json()?;

    // API returns newest-first; reverse for chronological order
    let messages: Vec<crate::state::ChatMessage> = rows.into_iter().rev().map(|row| {
        let kind = if row.from_user == local_username {
            crate::state::MessageKind::Own
        } else {
            crate::state::MessageKind::Peer
        };

        let attachment = match (row.attachment_url, row.attachment_kind, row.attachment_filename) {
            (Some(url), Some(kind_str), Some(filename)) => {
                let att_kind = crate::state::AttachmentKind::from_str(&kind_str);
                Some(crate::state::Attachment { url, kind: att_kind, filename })
            }
            _ => None,
        };

        crate::state::ChatMessage {
            id:         0, // Reassigned by the caller using next_message_id
            author:     row.from_user,
            content:    row.content,
            timestamp:  iso_to_hhmm(&row.created_at),
            kind,
            attachment,
            unix_ts:    0, // History messages never expire
            reactions:  Vec::new(),       // hydrated by fetch_reactions on connect
            db_id:      Some(row.id),     // DB UUID for reaction FKs
        }
    }).collect();

    Ok(messages)
}
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

/// Fetch all reactions for a set of message IDs. Returns a map of message_id → Vec<Reaction>.
pub fn fetch_reactions(
    access_token: &str,
    message_ids: &[String],
) -> Result<std::collections::HashMap<String, Vec<crate::state::Reaction>>> {
    let client = Client::new();
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

/// Convert an ISO 8601 timestamp to "HH:MM" for display.
fn iso_to_hhmm(iso: &str) -> String {
    if let Some(time_part) = iso.split('T').nth(1) {
        let parts: Vec<&str> = time_part.split(':').collect();
        if parts.len() >= 2 {
            return format!("{}:{}", parts[0], parts[1]);
        }
    }
    "??:??".to_string()
}

