// ─────────────────────────────────────────────────────────────────────────────
// main.rs — VoxLink entry point
//
// Responsibilities:
//   1. Initialize the logger (native only)
//   2. Create the eframe NativeOptions (window size, title, wgpu backend)
//   3. Launch the eframe event loop
//
// The Tokio async runtime will be spawned here in Phase 2.
// ─────────────────────────────────────────────────────────────────────────────

// On Windows release builds, suppress the console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod net;
mod state;
mod ui;

use app::VoxLinkApp;

fn main() {
    // ── Logger ────────────────────────────────────────────────────────────────
    #[cfg(not(target_arch = "wasm32"))]
    {
        env_logger::Builder::from_env(
            env_logger::Env::default().default_filter_or("voxlink=debug,warn"),
        )
        .init();
    }

    log::info!("Starting VoxLink");

    // ── Window options ────────────────────────────────────────────────────────
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("VoxLink")
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([800.0, 500.0])
            .with_icon(build_app_icon()),
        // wgpu is selected by default via the Cargo feature flag.
        // The renderer preference below confirms it explicitly.
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    // ── Launch ────────────────────────────────────────────────────────────────
    eframe::run_native(
        "VoxLink",
        native_options,
        Box::new(|cc| Ok(Box::new(VoxLinkApp::new(cc)) as Box<dyn eframe::App>)),
    )
    .expect("Fatal: eframe failed to start");
}

/// Generates a simple 64×64 blurple circle icon at runtime.
/// Replace with `egui::IconData::try_from_png_bytes(include_bytes!("../assets/icon.png"))` later.
fn build_app_icon() -> egui::IconData {
    let size = 64u32;
    let center = (size / 2) as f32;
    let radius_sq = (center * 0.92) * (center * 0.92);

    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist_sq = dx * dx + dy * dy;

            if dist_sq < radius_sq {
                // Blurple fill
                rgba.push(0x58); // R
                rgba.push(0x65); // G
                rgba.push(0xf2); // B
                rgba.push(0xff); // A
            } else {
                // Transparent outside the circle
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }

    egui::IconData { rgba, width: size, height: size }
}
