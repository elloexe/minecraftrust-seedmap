//! Minecraft Rust Seedmap: a seed map and structure finder for Minecraft
//! Rust (Java Edition 26.3 in Rust), built on the game's own world generator.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod finder;
mod jobs;
mod map;
mod theme;

/// Where a crash report goes: next to the saved settings.
pub fn crash_log_path() -> Option<std::path::PathBuf> {
    eframe::storage_dir("minecraftrust-seedmap").map(|d| d.join("crash.log"))
}

/// Every panic, on any thread, is appended to crash.log with a backtrace.
/// Worker threads catch theirs and carry on; this keeps a trace of them.
fn install_crash_log() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(path) = crash_log_path() {
            let _ = std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")));
            let when = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let thread = std::thread::current()
                .name()
                .unwrap_or("unnamed")
                .to_owned();
            let report = format!(
                "--- unix time {when}, version {}, thread '{thread}'\n{info}\n{}\n",
                env!("CARGO_PKG_VERSION"),
                std::backtrace::Backtrace::force_capture()
            );
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                let _ = file.write_all(report.as_bytes());
            }
        }
        default(info);
    }));
}

fn main() -> eframe::Result<()> {
    install_crash_log();
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
