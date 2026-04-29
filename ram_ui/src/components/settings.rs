//! Settings panel — global config, encryption toggles, multi-instance control.

use eframe::egui;
use ram_core::models::AppConfig;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Actions the settings panel can emit.
#[allow(dead_code)]
pub enum SettingsAction {
    SaveConfig,
    ChangePassword {
        new_password: String,
    },
    ClearPassword,
    EnableMultiInstance,
    DisableMultiInstance,
    MultiTabOrganize,
    MultiTabMinimizeAll,
    MultiTabRestoreAll,
    MultiTabMemoryCleanup,
    MultiTabStartAfkPreventer,
    MultiTabStopAfkPreventer,
    MultiTabTestAfk, // Test AFK immediately
    BulkImportCookies {
        cookie_file: String,
        proxy_file: Option<String>,
    },
}

/// Persistent state for the settings panel password change UI.
#[derive(Default)]
pub struct SettingsState {
    pub new_password_input: String,
    pub confirm_password_input: String,
    pub afk_preventer_active: Arc<AtomicBool>,
    pub cookie_file_path: Option<String>,
    pub proxy_file_path: Option<String>,
}

/// Draw the settings UI. Returns `Some(SettingsAction)` when an action is triggered.
pub fn show(
    ui: &mut egui::Ui,
    config: &mut AppConfig,
    has_password: bool,
    state: &mut SettingsState,
    roblox_running: bool,
    afk_preventer_active: Arc<AtomicBool>,
) -> Option<SettingsAction> {
    let mut action: Option<SettingsAction> = None;

    egui::ScrollArea::vertical().show(ui, |ui| {

    ui.heading("Settings");
    ui.separator();
    ui.add_space(8.0);

    let section_frame = egui::Frame::default()
        .inner_margin(egui::Margin::same(10.0))
        .rounding(egui::Rounding::same(6.0))
        .fill(ui.visuals().extreme_bg_color);

    // ---- Storage backend ----
    section_frame.show(ui, |ui: &mut egui::Ui| {
        ui.set_min_width(ui.available_width());
        ui.strong("Storage");
        ui.add_space(4.0);
        ui.checkbox(
            &mut config.use_credential_manager,
            "Use Windows Credential Manager (instead of encrypted file)",
        );
    });
    ui.add_space(6.0);

    // ---- Launch Behavior ----
    section_frame.show(ui, |ui: &mut egui::Ui| {
        ui.set_min_width(ui.available_width());
        ui.strong("Launch Behavior");
        ui.add_space(4.0);

        let mut wants_multi = config.multi_instance_enabled;
        let toggled = ui.checkbox(
            &mut wants_multi,
            "Enable multi-instance",
        ).changed();
        if toggled {
            if wants_multi {
                action = Some(SettingsAction::EnableMultiInstance);
            } else {
                action = Some(SettingsAction::DisableMultiInstance);
            }
        }
        if config.multi_instance_enabled {
            ui.colored_label(
                egui::Color32::from_rgb(220, 160, 40),
                "\u{26a0} This interacts with Hyperion anti-cheat and may carry ban risk.",
            );
            ui.add_space(4.0);
            ui.checkbox(
                &mut config.ignore_multi_instance_warning,
                "Ignore multi-instance close warning",
            ).on_hover_text("Don't show the warning when closing the program.");
        }
        if !config.multi_instance_enabled && roblox_running {
            ui.colored_label(
                egui::Color32::from_rgb(180, 180, 180),
                "Close all Roblox processes (including tray) before enabling.",
            );
        }

        ui.add_space(4.0);
        ui.checkbox(
            &mut config.kill_background_roblox,
            "Kill Roblox tray/background processes automatically",
        ).on_hover_text("Kills idle \"always running\" Roblox processes (--launch-to-tray).");
        if config.multi_instance_enabled && !config.kill_background_roblox {
            ui.colored_label(
                egui::Color32::from_rgb(220, 160, 40),
                "⚠ Recommended when multi-instance is enabled — tray processes stack up.",
            );
        }

        ui.add_space(4.0);
        ui.checkbox(
            &mut config.auto_arrange_windows,
            "Auto-arrange Roblox windows after launch",
        ).on_hover_text("Tiles Roblox windows in a grid (2 = side-by-side, 4 = 2×2, etc.).");
    });
    ui.add_space(6.0);

    // ---- Privacy ----
    section_frame.show(ui, |ui: &mut egui::Ui| {
        ui.set_min_width(ui.available_width());
        ui.strong("Privacy");
        ui.add_space(4.0);
        ui.checkbox(
            &mut config.privacy_mode,
            "Clear RobloxCookies.dat before each launch",
        ).on_hover_text("Prevents Roblox from associating your accounts via stored cookies.");
        ui.checkbox(
            &mut config.anonymize_names,
            "Anonymize account names",
        ).on_hover_text("Replaces usernames and display names with generic \"Account 1\", \"Account 2\", etc.");
        ui.add_space(4.0);
        ui.checkbox(
            &mut config.check_for_updates,
            "Check for updates on startup",
        ).on_hover_text("Queries GitLab for newer releases (requires restart to take effect).");
    });
    ui.add_space(6.0);

    // ---- Roblox path override ----
    section_frame.show(ui, |ui: &mut egui::Ui| {
        ui.set_min_width(ui.available_width());
        ui.strong("Roblox Player Path");
        ui.add_space(4.0);
        ui.label("Leave empty for auto-detect:");
        let mut path_str = config
            .roblox_player_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        if ui.text_edit_singleline(&mut path_str).changed() {
            config.roblox_player_path = if path_str.trim().is_empty() {
                None
            } else {
                Some(std::path::PathBuf::from(path_str))
            };
        }
    });

    ui.add_space(12.0);

    if ui.button("💾  Save Settings").clicked() {
        action = Some(SettingsAction::SaveConfig);
    }

    ui.add_space(12.0);
    ui.separator();
    ui.add_space(8.0);

    // ---- Master password management ----
    section_frame.show(ui, |ui: &mut egui::Ui| {
        ui.set_min_width(ui.available_width());
        ui.strong("Master Password");
        ui.add_space(4.0);
        if has_password {
            ui.label("A master password is currently set.");
        } else {
            ui.colored_label(
                egui::Color32::from_rgb(220, 160, 40),
                "⚠ No master password set. Add an account to set one.",
            );
        }
        ui.add_space(4.0);

        ui.label("New password:");
        ui.add(
            egui::TextEdit::singleline(&mut state.new_password_input)
                .password(true)
                .hint_text("Enter new password"),
        );
        ui.label("Confirm password:");
        ui.add(
            egui::TextEdit::singleline(&mut state.confirm_password_input)
                .password(true)
                .hint_text("Confirm new password"),
        );
        ui.add_space(4.0);

        let passwords_match = !state.new_password_input.is_empty()
            && state.new_password_input == state.confirm_password_input;

        if !state.new_password_input.is_empty()
            && !state.confirm_password_input.is_empty()
            && !passwords_match
        {
            ui.colored_label(
                egui::Color32::from_rgb(200, 60, 60),
                "Passwords do not match.",
            );
        }

        if ui
            .add_enabled(passwords_match, egui::Button::new("🔑  Change Password"))
            .clicked()
        {
            let new_pw = state.new_password_input.clone();
            state.new_password_input.clear();
            state.confirm_password_input.clear();
            action = Some(SettingsAction::ChangePassword {
                new_password: new_pw,
            });
        }
    }); // section_frame

    // ---- Multi Tab Management (Experimental) ----
    ui.add_space(16.0);
    ui.separator();
    ui.add_space(8.0);
    ui.heading("🧪 Multi Tab Management");
    ui.colored_label(
        egui::Color32::from_rgb(220, 160, 40),
        "⚠ EXPERIMENTAL - Manage multiple Roblox windows",
    );
    ui.add_space(4.0);

    let section_frame = egui::Frame::default()
        .inner_margin(egui::Margin::same(10.0))
        .rounding(egui::Rounding::same(6.0))
        .fill(ui.visuals().extreme_bg_color);

    section_frame.show(ui, |ui: &mut egui::Ui| {
        ui.set_min_width(ui.available_width());
        ui.strong("Window Management");
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("🔲 Organize Windows").clicked() {
                action = Some(SettingsAction::MultiTabOrganize);
            }
            ui.label("Arrange all Roblox windows in a grid");
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("⬇ Minimize All").clicked() {
                action = Some(SettingsAction::MultiTabMinimizeAll);
            }
            if ui.button("⬆ Restore All").clicked() {
                action = Some(SettingsAction::MultiTabRestoreAll);
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("🧹 Memory Cleanup").clicked() {
                action = Some(SettingsAction::MultiTabMemoryCleanup);
            }
            ui.label("Clear memory from all Roblox processes");
        });
    });

    ui.add_space(8.0);

    section_frame.show(ui, |ui: &mut egui::Ui| {
        ui.set_min_width(ui.available_width());
        ui.strong("AFK Prevention");
        ui.add_space(4.0);
        ui.label("Send ESC with 4 different methods: SendInput, PostMessageW, keybd_event, SendInput again");
        ui.add_space(4.0);
        if afk_preventer_active.load(Ordering::Relaxed) {
            ui.colored_label(
                egui::Color32::from_rgb(80, 200, 80),
                "🟢 AFK Prevention Active",
            );
            ui.horizontal(|ui| {
                if ui.button("⏹ Stop").clicked() {
                    action = Some(SettingsAction::MultiTabStopAfkPreventer);
                }
                ui.label("Running every 10 min...");
            });
        } else {
            ui.colored_label(
                egui::Color32::from_rgb(150, 150, 150),
                "⚪ AFK Prevention Inactive",
            );
            ui.horizontal(|ui| {
                if ui.button("▶ Start").clicked() {
                    action = Some(SettingsAction::MultiTabStartAfkPreventer);
                }
                ui.label("Auto (10 min)");
            });
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("🔴 Test Now").clicked() {
                action = Some(SettingsAction::MultiTabTestAfk);
            }
            ui.label("Test ESC send immediately");
        });
    });

    // ---- Data Location ----
    ui.add_space(16.0);
    ui.separator();
    ui.add_space(8.0);
    ui.strong("Data Location");
    ui.add_space(4.0);

    let data_dir = crate::data_dir();
    ui.label("Accounts and settings are saved to:");
    ui.monospace(data_dir.display().to_string());
    ui.add_space(4.0);

    if ui.button("📂 Open Folder").clicked() {
        let _ = std::process::Command::new("explorer")
            .arg(data_dir.as_os_str())
            .spawn();
    }

    // ---- Bulk Cookie Import ----
    ui.add_space(16.0);
    ui.separator();
    ui.add_space(8.0);
    ui.heading("📥 Bulk Cookie Import");
    ui.colored_label(
        egui::Color32::from_rgb(220, 160, 40),
        "⚠ Import multiple accounts from a text file",
    );
    ui.add_space(4.0);
    ui.label("File format: One cookie per line, starting with");
    ui.monospace("_|WARNING:-DO-NOT-SHARE-THIS.");
    ui.add_space(4.0);
    
    section_frame.show(ui, |ui: &mut egui::Ui| {
        ui.set_min_width(ui.available_width());
        ui.strong("Import from File");
        ui.add_space(4.0);
        ui.label("Cookie file (.txt):");
        ui.add_space(4.0);
        
        if ui.button("📄 Select Cookie File").clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Text Files", &["txt"])
                .add_filter("All Files", &["*"])
                .pick_file()
            {
                state.cookie_file_path = Some(path.display().to_string());
            }
        }
        
        if let Some(ref path) = state.cookie_file_path {
            ui.label(format!("Selected: {}", path));
        }
        
        ui.add_space(4.0);
        ui.label("Proxy file (.txt) [optional]:");
        ui.add_space(4.0);
        if ui.button("🌐 Select Proxy File").clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Text Files", &["txt"])
                .add_filter("All Files", &["*"])
                .pick_file()
            {
                state.proxy_file_path = Some(path.display().to_string());
            }
        }
        
        if let Some(ref path) = state.proxy_file_path {
            ui.label(format!("Selected: {}", path));
        }
        
        ui.add_space(4.0);
        ui.colored_label(
            egui::Color32::from_rgb(180, 180, 180),
            "Proxy format: http://proxy:port or user:pass@proxy:port",
        );
        ui.add_space(8.0);
        
        if state.cookie_file_path.is_some() {
            if ui.button("🚀 Start Import").clicked() {
                action = Some(SettingsAction::BulkImportCookies {
                    cookie_file: state.cookie_file_path.clone().unwrap_or_default(),
                    proxy_file: state.proxy_file_path.clone(),
                });
            }
        }
        
        ui.add_space(4.0);
        ui.label("Each cookie will be validated and added automatically.");
        ui.add_space(2.0);
        ui.colored_label(
            egui::Color32::from_rgb(180, 180, 180),
            "Note: Validation may take a few seconds per account.",
        );
    });

    }); // ScrollArea

    action
}
