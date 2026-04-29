//! Friends tab — view friends list, incoming requests, send friend requests.

use eframe::egui;
use ram_core::models::{Friend, FriendRequest};
use std::collections::HashMap;

/// Actions the friends panel can request.
#[derive(Debug, Clone)]
pub enum FriendsAction {
    /// Fetch friends list for the selected user.
    FetchFriends { user_id: u64 },
    /// Fetch incoming friend requests.
    FetchRequests { user_id: u64 },
    /// Search for users by username.
    SearchUsers { query: String },
    /// Send a friend request to a target user.
    SendRequest { user_id: u64, target_user_id: u64 },
    /// Accept an incoming friend request.
    AcceptRequest { user_id: u64, requester_id: u64 },
    /// Decline an incoming friend request.
    DeclineRequest { user_id: u64, requester_id: u64 },
    /// Refresh friends list and requests.
    Refresh { user_id: u64 },
    /// Select a friend to view their game info.
    SelectFriendGame {
        user_id: u64,
        friend_user_id: u64,
        place_id: u64,
    },
    /// Join a friend's game.
    JoinFriendGame { user_id: u64, place_id: u64 },
}

/// Persistent state for the friends panel.
#[derive(Default)]
pub struct FriendsState {
    /// Currently viewing friends of this user.
    pub viewing_user_id: Option<u64>,
    /// Cached friends list.
    pub friends: Vec<Friend>,
    /// Cached incoming requests.
    pub incoming_requests: Vec<FriendRequest>,
    /// Loading state.
    pub loading: bool,
    /// Error message if any.
    pub error: Option<String>,
    /// Target username for sending friend request.
    pub add_friend_input: String,
    /// Show requests panel.
    pub show_requests: bool,
    /// Search filter for friends.
    pub search_filter: String,
    /// User search results.
    pub search_results: Vec<(u64, String, String)>,
    /// Selected friend (for game info).
    pub selected_friend_id: Option<u64>,
    /// Resolved game name for selected friend (if in game).
    pub selected_friend_game: Option<String>,
}

/// Show the friends panel. Returns an optional action.
pub fn show(
    ui: &mut egui::Ui,
    state: &mut FriendsState,
    selected_user_id: Option<u64>,
    avatar_bytes: &HashMap<u64, Vec<u8>>,
) -> Option<FriendsAction> {
    let mut action: Option<FriendsAction> = None;

    // Only process if a user is selected
    if selected_user_id.is_none() {
        state.viewing_user_id = None;
        state.friends.clear();
        state.incoming_requests.clear();
        state.loading = false;
        state.error = None;
        return action;
    }

    // Check if viewing user changed
    if selected_user_id != state.viewing_user_id {
        state.viewing_user_id = selected_user_id;
        state.friends.clear();
        state.incoming_requests.clear();
        state.loading = false;
        if let Some(uid) = selected_user_id {
            state.loading = true;
            action = Some(FriendsAction::Refresh { user_id: uid });
        }
    }

    ui.horizontal(|ui| {
        ui.heading("👥 Friends");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("🔄 Refresh").clicked() {
                if let Some(uid) = state.viewing_user_id {
                    state.loading = true;
                    action = Some(FriendsAction::Refresh { user_id: uid });
                }
            }
            if ui
                .selectable_label(state.show_requests, "📩 Requests")
                .clicked()
            {
                state.show_requests = !state.show_requests;
            }
        });
    });

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(
                "⚠️ Experimental: Names & avatars may be incomplete due to Roblox API restrictions",
            )
            .small()
            .color(egui::Color32::YELLOW),
        );
    });

    ui.separator();

    if selected_user_id.is_none() {
        ui.centered_and_justified(|ui| {
            ui.label(egui::RichText::new("Select an account to view friends").italics());
        });
        return action;
    }

    if state.loading {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Loading...");
        });
    }

    if let Some(ref err) = state.error {
        ui.colored_label(egui::Color32::RED, err);
    }

    // Show incoming requests panel
    if state.show_requests && !state.incoming_requests.is_empty() {
        egui::CollapsingHeader::new("📩 Incoming Requests")
            .default_open(true)
            .show(ui, |ui| {
                for req in &state.incoming_requests {
                    ui.horizontal(|ui| {
                        // Avatar
                        if let Some(bytes) = avatar_bytes.get(&req.user_id) {
                            if let Ok(texture) = load_texture(ui.ctx(), &req.user_id, bytes) {
                                ui.add(egui::Image::new(&texture).max_height(32.0));
                            }
                        }
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new(&req.username).strong());
                            if !req.display_name.is_empty() {
                                ui.label(egui::RichText::new(&req.display_name).small());
                            }
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("✅ Accept").clicked() {
                                if let Some(uid) = state.viewing_user_id {
                                    action = Some(FriendsAction::AcceptRequest {
                                        user_id: uid,
                                        requester_id: req.user_id,
                                    });
                                }
                            }
                            if ui.button("❌ Decline").clicked() {
                                if let Some(uid) = state.viewing_user_id {
                                    action = Some(FriendsAction::DeclineRequest {
                                        user_id: uid,
                                        requester_id: req.user_id,
                                    });
                                }
                            }
                        });
                    });
                    ui.separator();
                }
            });
        ui.separator();
    }

    // Add friend section
    ui.collapsing("Add Friend", |ui| {
        ui.horizontal(|ui| {
            ui.label("Username:");
            ui.text_edit_singleline(&mut state.add_friend_input);
            if ui.button("Search").clicked() {
                if !state.add_friend_input.is_empty() {
                    action = Some(FriendsAction::SearchUsers {
                        query: state.add_friend_input.clone(),
                    });
                }
            }
        });

        // Show search results
        if !state.search_results.is_empty() {
            ui.separator();
            ui.label("Search Results:");
            let mut selected_user_id = None;
            for (user_id, username, display_name) in &state.search_results {
                ui.horizontal(|ui| {
                    ui.label(format!("{} ({})", username, display_name));
                    if ui.button("Send Request").clicked() {
                        selected_user_id = Some(*user_id);
                    }
                });
            }

            // Handle selection after the loop to avoid borrow conflicts
            if let Some(target_user_id) = selected_user_id {
                if let Some(uid) = state.viewing_user_id {
                    action = Some(FriendsAction::SendRequest {
                        user_id: uid,
                        target_user_id,
                    });
                    state.search_results.clear();
                    state.add_friend_input.clear();
                }
            }
        }
    });

    ui.separator();

    // Search filter
    ui.horizontal(|ui| {
        ui.label("Search:");
        ui.text_edit_singleline(&mut state.search_filter);
    });

    ui.separator();

    // Friends list
    let filtered_friends: Vec<&Friend> = if state.search_filter.is_empty() {
        state.friends.iter().collect()
    } else {
        let filter = state.search_filter.to_lowercase();
        state
            .friends
            .iter()
            .filter(|f| {
                f.username.to_lowercase().contains(&filter)
                    || f.display_name.to_lowercase().contains(&filter)
            })
            .collect()
    };

    egui::ScrollArea::vertical().show(ui, |ui| {
        if filtered_friends.is_empty() && !state.loading {
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new("No friends found").italics());
            });
        } else {
            for friend in &filtered_friends {
                let is_selected = state.selected_friend_id == Some(friend.user_id);
                let in_game = friend.presence.user_presence_type == 2;
                let has_place = friend.presence.place_id.is_some();

                ui.horizontal(|ui| {
                    // Online status indicator
                    let status_color = if friend.is_online {
                        if in_game {
                            egui::Color32::from_rgb(255, 165, 0) // Orange for in-game
                        } else {
                            egui::Color32::GREEN
                        }
                    } else {
                        egui::Color32::GRAY
                    };
                    ui.colored_label(status_color, "●");

                    // Name and status
                    ui.vertical(|ui| {
                        let name = if !friend.display_name.is_empty()
                            && friend.display_name != friend.username
                        {
                            format!("{} ({})", friend.display_name, friend.username)
                        } else {
                            friend.username.clone()
                        };
                        ui.label(egui::RichText::new(&name).strong());

                        let status = friend.presence.status_text();
                        let in_game = friend.presence.user_presence_type == 2;
                        let has_place = friend.presence.place_id.is_some();

                        if status != "Offline" {
                            let status_text = if in_game {
                                if let Some(ref game_name) = state.selected_friend_game {
                                    format!("🎮 {} - {}", status, game_name)
                                } else if has_place {
                                    format!("🎮 {} (loading...)", status)
                                } else {
                                    format!("🎮 {}", status)
                                }
                            } else if status == "In Studio" {
                                "🎼 In Studio".to_string()
                            } else {
                                status.to_string()
                            };
                            ui.label(
                                egui::RichText::new(&status_text)
                                    .small()
                                    .color(egui::Color32::from_gray(180)),
                            );
                        }

                        // Show Join Game button for in-game friends
                        if in_game && has_place {
                            if ui.button("🎮 Join Game").clicked() {
                                if let Some(place_id) = friend.presence.place_id {
                                    action = Some(FriendsAction::JoinFriendGame {
                                        user_id: state.viewing_user_id.unwrap_or(0),
                                        place_id,
                                    });
                                }
                            }
                        }
                    });

                    // Make clickable to show game info
                    if ui
                        .add(egui::SelectableLabel::new(is_selected, ""))
                        .clicked()
                    {
                        if in_game && has_place {
                            state.selected_friend_id = Some(friend.user_id);
                            state.selected_friend_game = None;
                            action = Some(FriendsAction::SelectFriendGame {
                                user_id: state.viewing_user_id.unwrap_or(0),
                                friend_user_id: friend.user_id,
                                place_id: friend.presence.place_id.unwrap(),
                            });
                        } else {
                            state.selected_friend_id = None;
                            state.selected_friend_game = None;
                        }
                    }

                    // Auto-fetch game name for in-game friends (if not already loaded)
                    if in_game && has_place && state.selected_friend_game.is_none() {
                        action = Some(FriendsAction::SelectFriendGame {
                            user_id: state.viewing_user_id.unwrap_or(0),
                            friend_user_id: friend.user_id,
                            place_id: friend.presence.place_id.unwrap(),
                        });
                    }
                });
                ui.separator();
            }
        }
    });

    action
}

/// Load a texture from image bytes.
fn load_texture(
    ctx: &egui::Context,
    user_id: &u64,
    bytes: &[u8],
) -> Result<egui::TextureHandle, String> {
    let texture_name = format!("friend_avatar_{user_id}");
    if let Some(existing) =
        ctx.data(|d| d.get_temp::<egui::TextureHandle>(egui::Id::new(&texture_name)))
    {
        return Ok(existing);
    }
    let image = image::load_from_memory(bytes)
        .map_err(|e| e.to_string())?
        .to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let pixel_data = image.into_raw();
    let texture = ctx.load_texture(
        &texture_name,
        egui::ColorImage::from_rgba_unmultiplied(size, &pixel_data),
        egui::TextureOptions::default(),
    );
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(&texture_name), texture.clone()));
    Ok(texture)
}
