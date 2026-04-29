use eframe::egui;
use ram_core::models::{GameSearchResult, PrivateServerInfo};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum GameAction {
    Search {
        query: String,
    },
    RefreshPopular,
    RefreshFavorites,
    Launch {
        place_id: u64,
    },
    LaunchPrivateServer {
        access_code: String,
        place_id: u64,
    },
    CreateVipServer {
        universe_id: u64,
        place_id: u64,
        name: String,
    },
    ListPrivateServers {
        place_id: u64,
    },
    AddFavorite {
        user_id: u64,
        place_id: u64,
    },
    CheckVipPrice {
        universe_id: u64,
        place_id: u64,
    },
}

#[derive(Default, Clone)]
pub struct GamesState {
    pub search_query: String,
    pub search_results: Vec<GameSearchResult>,
    pub popular_games: Vec<GameSearchResult>,
    pub favorite_games: Vec<GameSearchResult>,
    pub private_servers: Vec<PrivateServerInfo>,
    pub selected_game_index: Option<usize>,
    pub selected_private_server_index: Option<usize>,
    pub selected_game: Option<GameSearchResult>,
    pub loading_search: bool,
    pub loading_popular: bool,
    pub loading_favorites: bool,
    pub loading_private_servers: bool,
    pub loading_vip_price: bool,
    pub vip_server_name_input: String,
    pub error: Option<String>,
    pub show_vip_input: bool,
    pub vip_prices: HashMap<u64, (u64, bool)>,
    pub vip_price_unknown: HashMap<u64, bool>,
}

#[derive(Clone, Copy, PartialEq)]
enum GamesTab {
    Search,
    Popular,
    Favorites,
    PrivateServers,
}

pub fn show_games_tab(ui: &mut egui::Ui, state: &mut GamesState) -> Option<GameAction> {
    let mut action = None;
    static selected_tab: std::sync::Mutex<GamesTab> = std::sync::Mutex::new(GamesTab::Search);

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.heading("Games");
        ui.add_space(8.0);

        let tab = *selected_tab.lock().unwrap();

        ui.horizontal(|ui| {
            if ui
                .selectable_label(tab == GamesTab::Search, "🔍 Search")
                .clicked()
            {
                *selected_tab.lock().unwrap() = GamesTab::Search;
            }
            ui.add_space(8.0);
            if ui
                .selectable_label(tab == GamesTab::Popular, "🔥 Popular")
                .clicked()
            {
                *selected_tab.lock().unwrap() = GamesTab::Popular;
            }
            ui.add_space(8.0);
            if ui
                .selectable_label(tab == GamesTab::Favorites, "⭐ Favorites")
                .clicked()
            {
                *selected_tab.lock().unwrap() = GamesTab::Favorites;
            }
            ui.add_space(8.0);
            if ui
                .selectable_label(tab == GamesTab::PrivateServers, "🔒 Private Servers")
                .clicked()
            {
                *selected_tab.lock().unwrap() = GamesTab::PrivateServers;
            }
        });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        if tab == GamesTab::Search {
            ui.horizontal(|ui| {
                ui.label("Search:");
                let response = ui.text_edit_singleline(&mut state.search_query);
                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    if !state.search_query.is_empty() {
                        action = Some(GameAction::Search {
                            query: state.search_query.clone(),
                        });
                    }
                }
            });

            ui.horizontal(|ui| {
                if ui.button("Search").clicked() {
                    if !state.search_query.is_empty() {
                        action = Some(GameAction::Search {
                            query: state.search_query.clone(),
                        });
                    }
                }
            });
        }

        if tab == GamesTab::Popular {
            if ui.button("Refresh Popular").clicked() {
                action = Some(GameAction::RefreshPopular);
            }
        }

        if tab == GamesTab::Favorites {
            if ui.button("Refresh Favorites").clicked() {
                action = Some(GameAction::RefreshFavorites);
            }
        }

        ui.add_space(16.0);

        let games = match tab {
            GamesTab::Search => &state.search_results,
            GamesTab::Popular => &state.popular_games,
            GamesTab::Favorites => &state.favorite_games,
            GamesTab::PrivateServers => &state.favorite_games,
        };

        if tab == GamesTab::PrivateServers {
            if let Some(ref game) = state.selected_game {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(&game.name).strong());
                ui.label(format!("Place ID: {}", game.place_id));
                if let Some(universe_id) = game.universe_id {
                    ui.label(format!("Universe ID: {}", universe_id));
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("📋 Load Private Servers").clicked() {
                        action = Some(GameAction::ListPrivateServers {
                            place_id: game.place_id,
                        });
                    }
                    if state.loading_vip_price {
                        ui.label("Checking price...");
                    } else if let Some((price, allowed)) = game
                        .universe_id
                        .and_then(|uid| state.vip_prices.get(&uid))
                        .copied()
                    {
                        if !allowed {
                            ui.label(egui::RichText::new("VIP: Disabled").small().weak());
                        } else if price > 0 {
                            ui.label(
                                egui::RichText::new(format!("VIP: {} Robux", price))
                                    .small()
                                    .weak(),
                            );
                        } else {
                            ui.label(egui::RichText::new("VIP: FREE").small().weak());
                        }
                    } else {
                        // Check Price disabled - Roblox API doesn't return price for games without user servers
                        ui.label(
                            egui::RichText::new("VIP price: see on Roblox")
                                .small()
                                .weak(),
                        );
                    }
                });
                ui.add_space(4.0);
                #[allow(unused_variables)]
                ui.horizontal(|_ui| {
                    // Create VIP Server disabled - use Roblox website
                });
                // Show private servers ONLY in PrivateServers tab
                if tab == GamesTab::PrivateServers && !state.private_servers.is_empty() {
                    ui.add_space(8.0);
                    ui.label("Your Private Servers:");
                    for (idx, server) in state.private_servers.iter().enumerate() {
                        let is_selected = state.selected_private_server_index == Some(idx);
                        ui.horizontal(|ui| {
                            let mut label = server.name.clone();
                            if let Some(ref display_name) = server.owner_display_name {
                                label.push_str(&format!(" ({})", display_name));
                            }
                            if ui.selectable_label(is_selected, &label).clicked() {
                                state.selected_private_server_index = Some(idx);
                            }
                        });
                        if is_selected {
                            ui.indent("server_details", |ui| {
                                let owner_text = server
                                    .owner_display_name
                                    .as_deref()
                                    .unwrap_or(&server.owner_name);
                                ui.label(format!("Owner: {}", owner_text));
                                if ui.button("▶ Join").clicked() {
                                    action = Some(GameAction::LaunchPrivateServer {
                                        access_code: server.access_code.clone(),
                                        place_id: game.place_id,
                                    });
                                }
                            });
                        }
                    }
                }
            } else {
                ui.label("Select a game from Favorites tab first");
            }
        } else if games.is_empty() {
            ui.label("No games found.");
        } else if games.is_empty() {
            ui.label("No games found.");
        } else {
            for (idx, game) in games.iter().enumerate() {
                let is_selected = state.selected_game_index == Some(idx);
                ui.horizontal(|ui| {
                    if ui.selectable_label(is_selected, &game.name).clicked() {
                        state.selected_game_index = Some(idx);
                        state.selected_game = Some(game.clone());
                        // Clear private servers when game changes
                        state.private_servers.clear();
                        state.selected_private_server_index = None;
                    }
                    if is_selected {
                        ui.label(format!("ID: {}", game.place_id));
                    }
                });
            }
        }

        if state.loading_favorites {
            ui.label("Loading favorites...");
        }

        if state.loading_search {
            ui.label("Searching...");
        }

        if state.loading_popular {
            ui.label("Loading popular games...");
        }

        if state.loading_private_servers {
            ui.label("Loading private servers...");
        }

        if let Some(idx) = state.selected_game_index {
            if let Some(game) = games.get(idx) {
                // Skip this section for PrivateServers tab (already shown above)
                if tab != GamesTab::PrivateServers {
                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(8.0);

                    ui.label(egui::RichText::new(&game.name).strong());
                    ui.label(format!("Place ID: {}", game.place_id));
                    if let Some(universe_id) = game.universe_id {
                        ui.label(format!("Universe ID: {}", universe_id));
                    }

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("▶ Launch Game").clicked() {
                            action = Some(GameAction::Launch {
                                place_id: game.place_id,
                            });
                        }

                        // Create VIP Server disabled - use Roblox website directly
                    });
                }
            }
        }
    });

    action
}
