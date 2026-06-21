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

fn build_app_icon() -> egui::IconData {
    let bytes = include_bytes!("../voxlink.png");
    let image = image::load_from_memory(bytes).expect("Failed to load icon").into_rgba8();
    let (width, height) = image.dimensions();
    let rgba = image.into_raw();
    egui::IconData {
        rgba,
        width,
        height,
    }
}
