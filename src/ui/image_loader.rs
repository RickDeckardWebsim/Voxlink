use egui::{ColorImage, Context, TextureHandle, TextureOptions};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

static CACHE: OnceLock<Arc<Mutex<HashMap<String, CacheEntry>>>> = OnceLock::new();

enum CacheEntry {
    Loading,
    Loaded(TextureHandle),
    Error,
}

pub fn get_avatar_texture(ctx: &Context, url: &str) -> Option<TextureHandle> {
    if url.is_empty() {
        return None;
    }

    let cache = CACHE.get_or_init(|| Arc::new(Mutex::new(HashMap::new())));
    
    let mut map = cache.lock().unwrap();
    if let Some(entry) = map.get(url) {
        match entry {
            CacheEntry::Loaded(tex) => return Some(tex.clone()),
            _ => return None,
        }
    }

    // Not in cache, start loading
    map.insert(url.to_string(), CacheEntry::Loading);
    
    let url_clone = url.to_string();
    let cache_clone = cache.clone();
    let ctx_clone = ctx.clone();
    
    thread::spawn(move || {
        let result = fetch_and_decode(&url_clone);
        
        let mut map = cache_clone.lock().unwrap();
        match result {
            Ok(img) => {
                let tex = ctx_clone.load_texture(&url_clone, img, TextureOptions::default());
                map.insert(url_clone, CacheEntry::Loaded(tex));
            }
            Err(_) => {
                map.insert(url_clone, CacheEntry::Error);
            }
        }
        ctx_clone.request_repaint();
    });

    None
}

fn fetch_and_decode(url: &str) -> anyhow::Result<ColorImage> {
    let bytes = reqwest::blocking::get(url)?.bytes()?;
    let image = image::load_from_memory(&bytes)?;
    let size = [image.width() as _, image.height() as _];
    let image_buffer = image.to_rgba8();
    let pixels = image_buffer.as_flat_samples();
    Ok(ColorImage::from_rgba_unmultiplied(size, pixels.as_slice()))
}
