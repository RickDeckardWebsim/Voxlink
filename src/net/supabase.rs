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
    let url = format!("{}/auth/v1/signup", BASE_URL);
    
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
        
    let auth_res: AuthResponse = res.json()?;
    
    // 2. Insert into profiles table
    let profiles_url = format!("{}/rest/v1/profiles", BASE_URL);
    client.post(&profiles_url)
        .header("apikey", ANON_KEY)
        .header("Authorization", format!("Bearer {}", auth_res.access_token))
        .header("Content-Type", "application/json")
        .header("Prefer", "return=minimal")
        .json(&json!({
            "id": auth_res.user.id,
            "username": username,
        }))
        .send()?
        .error_for_status()?;

    Ok(auth_res)
}

pub fn sign_in(email: &str, password: &str) -> Result<AuthResponse> {
    let client = Client::new();
    let url = format!("{}/auth/v1/token?grant_type=password", BASE_URL);
    
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
    let url = format!("{}/rest/v1/profiles?id=eq.{}", BASE_URL, user_id);
    
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
    let url = format!("{}/storage/v1/object/avatars/{}", BASE_URL, filename);
    
    let content_type = match ext {
        "png" => "image/png",
        "jpeg" | "jpg" => "image/jpeg",
        _ => "application/octet-stream",
    };
    
    // We use POST to allow upserting, or actually Supabase storage uses POST for new, PUT for update.
    // If it already exists, POST might fail. Let's use PUT to upsert.
    // To upsert, we need a header: "x-upsert": "true" (some versions use this)
    // Actually, just using PUT /object/avatars/path works for upsert.
    // Let's use PUT just in case.
    // Wait, the standard Supabase storage API is POST to upload, but PUT with upsert works.
    
    let res = client.post(&url)
        .header("apikey", ANON_KEY)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", content_type)
        .header("x-upsert", "true")
        .body(bytes.clone())
        .send()?;
        
    if !res.status().is_success() {
        // If POST fails, try PUT
        let res2 = client.put(&url)
            .header("apikey", ANON_KEY)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", content_type)
            .header("x-upsert", "true")
            .body(bytes.clone())
            .send()?;
        res2.error_for_status()?;
    }
        
    let public_url = format!("{}/storage/v1/object/public/avatars/{}", BASE_URL, filename);
    Ok(public_url)
}
