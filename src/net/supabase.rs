use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

const BASE_URL: &str = "https://syftqwloslmnjyvppler.supabase.co";
const ANON_KEY: &str = "sb_publishable_VK3kO0lX4tTsrHlCsH6JFQ_ebB6_lMH";

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
}

pub fn sign_up(email: &str, password: &str, username: &str) -> Result<AuthResponse> {
    let client = Client::new();
    let url = format!("{}/auth/v1/signup?apikey={}", BASE_URL, ANON_KEY);
    
    // 1. Sign up user
    let res = client.post(&url)
        .header("apikey", ANON_KEY)
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
    let profiles_url = format!("{}/rest/v1/profiles", BASE_URL);
    client.post(&profiles_url)
        .header("apikey", ANON_KEY)
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
    let url = format!("{}/auth/v1/token?grant_type=password&apikey={}", BASE_URL, ANON_KEY);
    
    let res = client.post(&url)
        .header("apikey", ANON_KEY)
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
    let url = format!("{}/rest/v1/profiles?id=eq.{}&select=*", BASE_URL, user_id);
    
    let res = client.get(&url)
        .header("apikey", ANON_KEY)
        .header("Authorization", format!("Bearer {}", access_token))
        .send()?
        .error_for_status()?;
        
    let profiles: Vec<Profile> = res.json()?;
    profiles.into_iter().next().context("Profile not found")
}

pub fn update_profile(user_id: &str, access_token: &str, username: &str, avatar_url: Option<&str>) -> Result<()> {
    let client = Client::new();
    let url = format!("{}/rest/v1/profiles?id=eq.{}&apikey={}", BASE_URL, user_id, ANON_KEY);
    
    let mut body = json!({
        "username": username,
    });
    
    if let Some(url) = avatar_url {
        body["avatar_url"] = json!(url);
    }
    
    client.patch(&url)
        .header("apikey", ANON_KEY)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", "application/json")
        .header("Prefer", "return=minimal")
        .json(&body)
        .send()?
        .error_for_status()?;
        
    Ok(())
}

pub fn upload_avatar(user_id: &str, access_token: &str, bytes: Vec<u8>, ext: &str) -> Result<String> {
    let client = Client::new();
    let filename = format!("{}_avatar.{}", user_id, ext);
    let obj_url  = format!("{}/storage/v1/object/avatars/{}", BASE_URL, filename);

    let content_type = match ext.to_lowercase().as_str() {
        "png"          => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif"          => "image/gif",
        "webp"         => "image/webp",
        _              => "application/octet-stream",
    };

    // PUT = standard upsert verb for Supabase Storage.
    let res = client.put(&obj_url)
        .header("apikey", ANON_KEY)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", content_type)
        .header("x-upsert", "true")
        .body(bytes.clone())
        .send()?;

    if !res.status().is_success() {
        // Older Supabase Storage versions: fall back to POST with x-upsert.
        let res2 = client.post(&obj_url)
            .header("apikey", ANON_KEY)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", content_type)
            .header("x-upsert", "true")
            .body(bytes)
            .send()?;

        if !res2.status().is_success() {
            let status = res2.status();
            let body   = res2.text().unwrap_or_default();
            return Err(anyhow::anyhow!("Avatar upload failed ({}): {}", status, body));
        }
    }

    // Append a timestamp so image_loader re-fetches instead of serving the cached texture.
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    Ok(format!("{}/storage/v1/object/public/avatars/{}?t={}", BASE_URL, filename, ts))
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
    let obj_url = format!("{}/storage/v1/object/avatars/{}", BASE_URL, path);

    let res = client.post(&obj_url)
        .header("apikey", ANON_KEY)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", mime_for_ext(ext))
        .body(bytes)
        .send()?;

    if !res.status().is_success() {
        let status = res.status();
        let body   = res.text().unwrap_or_default();
        return Err(anyhow::anyhow!("Media upload failed ({}): {}", status, body));
    }

    Ok(format!("{}/storage/v1/object/public/avatars/{}", BASE_URL, path))
}

fn mime_for_ext(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "png"          => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif"          => "image/gif",
        "webp"         => "image/webp",
        "mp3"          => "audio/mpeg",
        "ogg"          => "audio/ogg",
        "wav"          => "audio/wav",
        "mp4"          => "video/mp4",
        "webm"         => "video/webm",
        "mov"          => "video/quicktime",
        _              => "application/octet-stream",
    }
}
