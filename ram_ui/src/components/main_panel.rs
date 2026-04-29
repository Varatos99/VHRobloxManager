//! Main content panel — selected account details, avatar, launch controls.

use eframe::egui;
use ram_core::api::GroupRole;
use ram_core::models::{Account, FavoritePlace};

/// Actions the main panel can request.
pub enum MainPanelAction {
    RemoveAccount(u64),
    UpdateAlias {
        user_id: u64,
        alias: String,
    },
    SaveFavorite {
        name: String,
        place_id: u64,
    },
    RemoveFavorite(usize),
    KillAll,
    RefreshRobux,
    RefreshGroups,
    FetchGroupRobux {
        group_id: u64,
        group_name: String,
    },
    LaunchGame {
        place_id: u64,
        job_id: Option<String>,
    },
    LaunchRoblox,
    LaunchStudio,
}

/// Persistent input state for the main panel.
#[derive(Default)]
pub struct MainPanelState {
    pub place_id_input: String,
    pub job_id_input: String,
    pub alias_input: String,
    /// Track which account the alias input belongs to.
    alias_for_user: Option<u64>,
    pub favorite_name_input: String,
}

/// Result returned by the main panel.
pub struct MainPanelResult {
    pub action: Option<MainPanelAction>,
    /// Screen rect of the Launch button (for tutorial highlighting).
    pub launch_btn_rect: egui::Rect,
}

/// Draw the main panel for a selected account.
pub fn show(
    ui: &mut egui::Ui,
    account: &Account,
    state: &mut MainPanelState,
    roblox_running: bool,
    avatar_bytes: Option<&Vec<u8>>,
    favorite_places: &[FavoritePlace],
    anonymize: bool,
    groups: &[(GroupRole, Option<u64>)], // (group, robux_balance)
) -> MainPanelResult {
    let mut action: Option<MainPanelAction> = None;
    let mut launch_btn_rect = egui::Rect::NOTHING;

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.vertical(|ui| {
            let section_frame = egui::Frame::default()
                .inner_margin(egui::Margin::same(10.0))
                .rounding(egui::Rounding::same(6.0))
                .fill(ui.visuals().extreme_bg_color);

            // ---- Header row: avatar + name ---
            section_frame.show(ui, |ui: &mut egui::Ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    // Avatar image (loaded from backend-downloaded bytes)
                    if let Some(bytes) = avatar_bytes {
                        let uri = format!("bytes://avatar/{}.png", account.user_id);
                        ui.add(
                            egui::Image::from_bytes(uri, bytes.clone())
                                .fit_to_exact_size(egui::vec2(64.0, 64.0))
                                .rounding(egui::Rounding::same(8.0)),
                        );
                    } else {
                        // Placeholder
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(64.0, 64.0), egui::Sense::hover());
                        ui.painter()
                            .rect_filled(rect, 8.0, egui::Color32::from_rgb(60, 60, 70));
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "?",
                            egui::FontId::proportional(28.0),
                            egui::Color32::WHITE,
                        );
                    }

                    ui.vertical(|ui| {
                        if anonymize {
                            ui.heading("Account");
                        } else {
                            ui.heading(&account.display_name);
                            ui.label(format!("@{}", account.username));
                            ui.label(format!("ID: {}", account.user_id));
                        }

                        // Presence badge
                        let status = account.last_presence.status_text();
                        let color = match account.last_presence.user_presence_type {
                            1 => egui::Color32::from_rgb(60, 180, 75),
                            2 => egui::Color32::from_rgb(30, 144, 255),
                            3 => egui::Color32::from_rgb(255, 165, 0),
                            _ => egui::Color32::GRAY,
                        };
                        ui.colored_label(color, status);

                        // Robux balance
                        if let Some(robux) = account.robux_balance {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.label("\u{1f4b0}");
                                ui.label(format!("{}", robux));
                                if account.is_premium {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(255, 215, 0),
                                        "Premium",
                                    );
                                }
                                if ui.small_button("↻").clicked() {
                                    action = Some(MainPanelAction::RefreshRobux);
                                }
                            });
                        } else {
                            ui.add_space(4.0);
                            if ui.button("Check Robux \u{1f4b0}").clicked() {
                                action = Some(MainPanelAction::RefreshRobux);
                            }
                        }
                    });
                });
            }); // header frame
            ui.add_space(6.0);

            // ---- Launch controls ---
            section_frame.show(ui, |ui: &mut egui::Ui| {
                ui.set_min_width(ui.available_width());
                ui.heading("Launch Game");
                ui.add_space(4.0);

                // Favorite places quick-select
                if !favorite_places.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Favorites:");
                        for (i, fav) in favorite_places.iter().enumerate() {
                            let btn = ui.small_button(&fav.name);
                            if btn.clicked() {
                                state.place_id_input = fav.place_id.to_string();
                            }
                            btn.context_menu(|ui| {
                                if ui.button("🗑 Remove").clicked() {
                                    action = Some(MainPanelAction::RemoveFavorite(i));
                                    ui.close_menu();
                                }
                            });
                        }
                    });
                    ui.add_space(4.0);
                }

                egui::Grid::new("launch_grid")
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Place ID:");
                        ui.text_edit_singleline(&mut state.place_id_input);
                        ui.end_row();

                        ui.label("Job ID (optional):");
                        ui.text_edit_singleline(&mut state.job_id_input);
                        ui.end_row();
                    });

                ui.add_space(4.0);

                // Save current Place ID as a favorite
                let place_valid = state.place_id_input.parse::<u64>().is_ok();
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(place_valid, |ui| {
                        ui.text_edit_singleline(&mut state.favorite_name_input)
                            .on_hover_text("Name for this favorite");
                        let can_save = !state.favorite_name_input.trim().is_empty();
                        if ui
                            .add_enabled(can_save, egui::Button::new("⭐ Save Favorite"))
                            .clicked()
                        {
                            if let Ok(pid) = state.place_id_input.parse::<u64>() {
                                action = Some(MainPanelAction::SaveFavorite {
                                    name: state.favorite_name_input.trim().to_string(),
                                    place_id: pid,
                                });
                                state.favorite_name_input.clear();
                            }
                        }
                    });
                });

                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    let launch_btn = ui.add_enabled(place_valid, egui::Button::new("🚀  Launch"));
                    launch_btn_rect = launch_btn.rect;
                    if launch_btn.clicked() {
                        if let Ok(place_id) = state.place_id_input.parse::<u64>() {
                            let job_id = if state.job_id_input.trim().is_empty() {
                                None
                            } else {
                                Some(state.job_id_input.trim().to_string())
                            };
                            action = Some(MainPanelAction::LaunchGame { place_id, job_id });
                        }
                    }

                    // New button: Launch Roblox only (no game)
                    if ui.button("▶ Launch Roblox").clicked() {
                        action = Some(MainPanelAction::LaunchRoblox);
                    }

                    ui.add_space(8.0);

                    // Launch Studio button (disabled for now - not supported yet)
                    ui.add_enabled(false, egui::Button::new("🎨 Launch Studio"))
                        .on_hover_text("Studio login not supported yet");

                    if roblox_running && ui.button("☠  Kill All Instances").clicked() {
                        action = Some(MainPanelAction::KillAll);
                    }
                }); // launch frame
            }); // launch frame
            ui.add_space(6.0);

            // ---- Account metadata ---
            section_frame.show(ui, |ui: &mut egui::Ui| {
                ui.set_min_width(ui.available_width());

                // Sync alias input when switching accounts
                if state.alias_for_user != Some(account.user_id) {
                    state.alias_input = account.alias.clone();
                    state.alias_for_user = Some(account.user_id);
                }

                egui::Grid::new("meta_grid")
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Alias:");
                        if ui.text_edit_singleline(&mut state.alias_input).lost_focus() {
                            action = Some(MainPanelAction::UpdateAlias {
                                user_id: account.user_id,
                                alias: state.alias_input.clone(),
                            });
                        }
                        ui.end_row();

                        if !account.group.is_empty() {
                            ui.label("Group:");
                            ui.label(&account.group);
                            ui.end_row();
                        }

                        if let Some(ts) = &account.last_validated {
                            ui.label("Validated:");
                            let age = chrono::Utc::now() - *ts;
                            let color = if age.num_hours() > 24 {
                                egui::Color32::from_rgb(200, 160, 60)
                            } else {
                                ui.visuals().text_color()
                            };
                            ui.colored_label(color, ts.format("%Y-%m-%d %H:%M UTC").to_string());
                            ui.end_row();
                        }

                        if !account.last_presence.last_location.is_empty() {
                            ui.label("Location:");
                            ui.label(&account.last_presence.last_location);
                            ui.end_row();
                        }
                    });
            }); // metadata frame
            ui.add_space(6.0);

            // ---- Groups section ---
            section_frame.show(ui, |ui: &mut egui::Ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.strong("Groups");
                    ui.add_space(10.0);
                    if ui.button("Load Groups").clicked() {
                        action = Some(MainPanelAction::RefreshGroups);
                    }
                });
                ui.add_space(4.0);

                if groups.is_empty() {
                    ui.colored_label(
                        egui::Color32::from_rgb(140, 140, 140),
                        "No groups loaded. Click 'Load Groups' to fetch.",
                    );
                } else {
                    egui::Grid::new("groups_grid")
                        .num_columns(3)
                        .spacing([10.0, 4.0])
                        .show(ui, |ui| {
                            ui.strong("Group");
                            ui.strong("Role");
                            ui.strong("Robux");
                            ui.end_row();

                            for (group, robux) in groups {
                                ui.label(&group.group_name);
                                ui.label(&group.role_name);
                                match robux {
                                    Some(r) if *r == u64::MAX => {
                                        ui.colored_label(
                                            egui::Color32::from_rgb(180, 140, 80),
                                            "No permission",
                                        );
                                    }
                                    Some(r) => {
                                        ui.label(r.to_string());
                                    }
                                    None => {
                                        if ui.small_button("Check").clicked() {
                                            action = Some(MainPanelAction::FetchGroupRobux {
                                                group_id: group.group_id,
                                                group_name: group.group_name.clone(),
                                            });
                                        }
                                    }
                                }
                                ui.end_row();
                            }
                        });
                }
            });
            ui.add_space(6.0);

            // ---- Danger zone ---
            section_frame.show(ui, |ui: &mut egui::Ui| {
                ui.set_min_width(ui.available_width());
                ui.strong("Danger Zone");
                ui.add_space(4.0);
                ui.colored_label(
                    egui::Color32::from_rgb(200, 60, 60),
                    "These actions cannot be undone.",
                );
                if ui.button("🗑  Remove Account").clicked() {
                    action = Some(MainPanelAction::RemoveAccount(account.user_id));
                }
            });
        }); // ui.vertical
    }); // ScrollArea

    MainPanelResult {
        action,
        launch_btn_rect,
    }
}

/// Show a placeholder when no account is selected.
pub fn show_empty(ui: &mut egui::Ui) {
    ui.centered_and_justified(|ui| {
        ui.label("Select an account from the sidebar to get started.");
    });
}
