#![windows_subsystem = "windows"]

mod app;
mod bridge;
mod components;
mod toast;

use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

/// Canonical data directory: `%APPDATA%\VHRobloxManager`.
fn data_dir() -> PathBuf {
    let base = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    base.join("VHRobloxManager")
}

/// Check for legacy data files next to the exe and offer to migrate them.
fn maybe_migrate_legacy_data(data_dir: &std::path::Path) {
    let legacy_config = PathBuf::from("config.json");
    let legacy_accounts = PathBuf::from("accounts.dat");

    let has_legacy = legacy_config.is_file() || legacy_accounts.is_file();
    let has_new = data_dir.join("config.json").is_file();

    if !has_legacy || has_new {
        return;
    }

    let result = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Info)
        .set_title("VHRobloxManager — Migrate Data")
        .set_description(
            "VHRobloxManager now stores data in a standard location.\n\n\
             Found existing data next to the exe. Move it to the new location?\n\n\
             • Yes — move files (recommended)\n\
             • No — keep using files next to the exe",
        )
        .set_buttons(rfd::MessageButtons::YesNo)
        .show();

    if result == rfd::MessageDialogResult::Yes {
        if let Err(e) = std::fs::create_dir_all(data_dir) {
            tracing::error!("Failed to create data dir: {e}");
            return;
        }
        for name in &["config.json", "accounts.dat"] {
            let src = PathBuf::from(name);
            if src.is_file() {
                let dst = data_dir.join(name);
                if let Err(e) = std::fs::rename(&src, &dst) {
                    if let Err(e2) = std::fs::copy(&src, &dst) {
                        tracing::error!("Failed to migrate {name}: rename={e}, copy={e2}");
                    } else {
                        let _ = std::fs::remove_file(&src);
                    }
                }
            }
        }
    }
}

fn main() {
    let log_dir = data_dir();
    let _ = std::fs::create_dir_all(&log_dir);
    
    // Log to file in %APPDATA%/VHRobloxManager/
    let file_appender = tracing_appender::rolling::daily(&log_dir, "vhrm_.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    
    // Only INFO+ logs (no debug) - user-friendly
    let log_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));
    
    tracing_subscriber::fmt()
        .with_env_filter(log_filter)
        .with_writer(non_blocking)
        .init();

    tracing::info!("VHRobloxManager started, log dir: {:?}", log_dir);
    
    let data_dir = data_dir();

    maybe_migrate_legacy_data(&data_dir);

    let _ = std::fs::create_dir_all(&data_dir);

    let (config_path, config) = if PathBuf::from("config.json").is_file()
        && !data_dir.join("config.json").is_file()
    {
        let p = PathBuf::from("config.json");
        let c = ram_core::AppConfig::load(&p);
        (p, c)
    } else {
        let p = data_dir.join("config.json");
        let mut c = ram_core::AppConfig::load(&p);
        if c.accounts_path == std::path::Path::new("accounts.dat") {
            c.accounts_path = data_dir.join("accounts.dat");
        }
        (p, c)
    };

    // Load ICO logo for window icon
    let raw_ico = include_bytes!("../../assets/Logo.ico");
    let (icon_rgba, icon_width, icon_height) = {
        let img = image::load_from_memory(raw_ico).expect("failed to decode Logo.ico");
        let rgba_img = img.to_rgba8();
        let (w, h) = rgba_img.dimensions();
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for p in rgba_img.pixels() {
            rgba.extend_from_slice(&[p[0], p[1], p[2], p[3]]);
        }
        (rgba, w, h)
    };

    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([config.window_width, config.window_height])
            .with_min_inner_size([640.0, 400.0])
            .with_title(format!("VH Roblox Manager v{}", env!("CARGO_PKG_VERSION")))
            .with_icon(eframe::egui::IconData {
                rgba: icon_rgba,
                width: icon_width,
                height: icon_height,
            }),
        ..Default::default()
    };

    let _ = eframe::run_native(
        "VH Roblox Manager",
        native_options,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(app::AppState::new(config, config_path)))
        }),
    );
}
