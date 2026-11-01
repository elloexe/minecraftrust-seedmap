//! Minecraft Rust Seedmap: a seed map and structure finder for Minecraft
//! Rust (Java Edition 26.3 in Rust), built on the game's own world generator.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod finder;
mod jobs;
mod map;
mod theme;

fn main() -> eframe::Result<()> {
    // Decode the worldgen bundle while the window opens.
    std::thread::spawn(|| {
        let _ = seedmap_core::data();
        let _ = seedmap_core::structures::catalog();
    });
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icon-64.png"))
        .unwrap_or_default();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Minecraft Rust Seedmap — 26.3")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([900.0, 560.0])
            .with_icon(std::sync::Arc::new(icon)),
        ..Default::default()
    };
    eframe::run_native(
        "minecraftrust-seedmap",
        options,
        Box::new(|cc| Ok(Box::new(app::SeedmapApp::new(cc)))),
    )
}
