//! Top-level application state and the `eframe::App` implementation that ties
//! the sidebar, main panel, settings, toast system, and backend bridge together.

use eframe::egui;
use ram_core::models::{Account, AccountStore, AppConfig, PrivateServer};
use rfd;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::bridge::{BackendBridge, BackendCommand, BackendEvent, ProxyStats};
use crate::components::{
    about, donate, friends, game_panel, group_panel, main_panel, private_servers, settings,
    sidebar, tutorial,
};
use crate::toast::{Toast, Toasts};

// ---------------------------------------------------------------------------
// Tabs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Accounts,
    Friends,
    Games,
    Settings,
    Donate,
    About,
}

// ---------------------------------------------------------------------------
// Add-account dialog state
// ---------------------------------------------------------------------------

#[derive(Default)]
struct AddAccountDialog {
    open: bool,
    cookie_input: String,
    /// Staging field for password — only committed on submit.
    password_input: String,
    /// True while we're waiting for the backend to validate.
    loading: bool,
    /// Error message from the last failed attempt.
    last_error: Option<String>,
}

// ---------------------------------------------------------------------------
// AppState
// ---------------------------------------------------------------------------

pub struct AppState {
    config: AppConfig,
    config_path: PathBuf,
    store: AccountStore,
    master_password: String,

    bridge: BackendBridge,
    toasts: Toasts,

    // UI state
    active_tab: Tab,
    selected_ids: HashSet<u64>,
    sidebar_state: sidebar::SidebarState,
    main_panel_state: main_panel::MainPanelState,
    group_panel_state: group_panel::GroupPanelState,
    games_state: game_panel::GamesState,
    private_servers_state: private_servers::PrivateServerState,
    settings_state: settings::SettingsState,
    add_dialog: AddAccountDialog,

    /// Downloaded avatar image bytes, keyed by user ID.
    avatar_bytes: HashMap<u64, Vec<u8>>,

    /// Downloaded game icon bytes, keyed by place ID.
    game_icon_bytes: HashMap<u64, Vec<u8>>,

    /// User IDs currently visible in the sidebar (after search filtering).
    visible_user_ids: Vec<u64>,

    /// Cached flag from sysinfo (refreshed lazily).
    roblox_running: bool,
    /// Frame counter to throttle background refreshes.
    frame_count: u64,

    /// Password prompt shown on first launch when store file exists.
    needs_unlock: bool,
    unlock_password_input: String,

    /// When set, shows a confirmation dialog before removing the account.
    confirm_remove: Option<u64>,

    /// Available update info: (version, release_url).
    update_available: Option<(String, String)>,
    /// Show the "What's New" changelog window.
    show_changelog: bool,

    /// AFK prevention timer state (AtomicBool for thread-safe access)
    afk_preventer_active: Arc<AtomicBool>,

    /// Interactive first-launch tutorial.
    tutorial: tutorial::TutorialState,

    /// Webview login: receiver for the result from the webview thread.
    webview_login_rx:
        Option<std::sync::mpsc::Receiver<crate::components::webview_login::WebViewLoginMsg>>,
    /// The password buffer to use with the webview login result.
    webview_login_password: String,

    /// Friends panel state.
    friends_state: friends::FriendsState,
    /// Downloaded avatar image bytes for friends.
    friends_avatar_bytes: HashMap<u64, Vec<u8>>,

    /// Bulk import state
    bulk_import_progress: Option<(usize, usize, String, Option<String>, String)>,
    bulk_import_result: Option<(
        Vec<(u64, String, String)>,
        Vec<(String, String)>,
        Option<ProxyStats>,
    )>,
    bulk_import_proxy_test: Option<(usize, usize, usize, String)>,
    show_bulk_import_dialog: bool,

    /// Groups cache for accounts
    user_groups: HashMap<u64, Vec<(ram_core::api::GroupRole, Option<u64>)>>,

    /// Background thread handle for AFK prevention
    #[allow(dead_code)]
    afk_preventer_handle: Option<std::thread::JoinHandle<()>>,
}

impl AppState {
    pub fn new(mut config: AppConfig, config_path: PathBuf) -> Self {
        let bridge = BackendBridge::spawn();
        let needs_unlock = config.accounts_path.is_file();

        // If multi-instance was previously enabled, run the same validation as
        // the UI toggle: kill tray processes, wait, then only acquire the mutex
        // if no Roblox instances remain.
        if config.multi_instance_enabled {
            ram_core::process::kill_tray_roblox();
            std::thread::sleep(std::time::Duration::from_millis(500));
            if ram_core::process::is_roblox_running() {
                tracing::warn!(
                    "Roblox is running at startup — cannot acquire singleton mutex. \
                     Disabling multi-instance until manually re-enabled."
                );
                config.multi_instance_enabled = false;
            } else if let Err(e) = ram_core::process::enable_multi_instance() {
                tracing::warn!("Failed to acquire singleton mutex at startup: {e}");
                config.multi_instance_enabled = false;
            }
        }

        let mut sidebar_state = sidebar::SidebarState::default();
        sidebar_state.sort_order = match config.sort_mode.as_str() {
            "Name" => sidebar::SortOrder::Name,
            "Status" => sidebar::SortOrder::Status,
            _ => sidebar::SortOrder::Custom,
        };

        let mut state = Self {
            config,
            config_path,
            store: AccountStore::default(),
            master_password: String::new(),
            bridge,
            toasts: Toasts::default(),
            active_tab: Tab::Accounts,
            selected_ids: HashSet::new(),
            sidebar_state,
            main_panel_state: main_panel::MainPanelState::default(),
            group_panel_state: group_panel::GroupPanelState::default(),
            games_state: game_panel::GamesState::default(),
            private_servers_state: private_servers::PrivateServerState::default(),
            settings_state: settings::SettingsState::default(),
            add_dialog: AddAccountDialog::default(),
            avatar_bytes: HashMap::new(),
            game_icon_bytes: HashMap::new(),
            visible_user_ids: Vec::new(),
            roblox_running: false,
            frame_count: 0,
            needs_unlock,
            unlock_password_input: String::new(),
            confirm_remove: None,
            update_available: None,
            show_changelog: false,
            afk_preventer_active: Arc::new(AtomicBool::new(false)),
            tutorial: tutorial::TutorialState::default(),
            webview_login_rx: None,
            webview_login_password: String::new(),
            friends_state: friends::FriendsState::default(),
            friends_avatar_bytes: HashMap::new(),
            bulk_import_progress: None,
            bulk_import_result: None,
            bulk_import_proxy_test: None,
            show_bulk_import_dialog: false,
            user_groups: HashMap::new(),
            afk_preventer_handle: None,
        };

        // Check for updates on startup if enabled
        if state.config.check_for_updates {
            state.bridge.send(BackendCommand::CheckForUpdates {
                current_version: env!("CARGO_PKG_VERSION").to_string(),
            });
        }

        // Update check disabled
        // Resolve game icons for saved private servers
        state.resolve_private_server_icons();

        // Detect first launch after update
        let current = env!("CARGO_PKG_VERSION");
        let is_new_version = state.config.last_seen_version.as_deref() != Some(current);
        if is_new_version && state.config.last_seen_version.is_some() {
            // Upgraded from a previous version — show changelog
            state.show_changelog = true;
        }
        // True first launch — show the tutorial (but not if an accounts file
        // already exists, which means an existing user just lost their config).
        if state.config.last_seen_version.is_none() && !state.needs_unlock {
            state.tutorial = tutorial::TutorialState::start();
        }
        // Always update the stored version
        state.config.last_seen_version = Some(current.to_string());
        let _ = state.config.save(&state.config_path);

        state
    }

    // ---- Event processing ----

    fn process_events(&mut self) {
        for event in self.bridge.poll() {
            match event {
                BackendEvent::AccountValidated {
                    account,
                    encrypted_cookie: _,
                } => {
                    let name = if self.config.anonymize_names {
                        "Account".to_string()
                    } else {
                        account.username.clone()
                    };
                    // Avoid duplicates
                    self.store.remove_by_id(account.user_id);
                    self.store.accounts.push(*account);
                    self.toasts.push(Toast::success(format!("Added {name}")));
                    self.add_dialog.loading = false;
                    self.add_dialog.last_error = None;
                    self.add_dialog.open = false;
                    self.add_dialog.cookie_input.clear();
                    self.add_dialog.password_input.clear();
                    self.tutorial
                        .advance_from(tutorial::TutorialStep::EnterCookie);
                    self.auto_save();
                }
                BackendEvent::AccountRemoved { user_id } => {
                    self.store.remove_by_id(user_id);
                    self.selected_ids.remove(&user_id);
                    self.toasts.push(Toast::info("Account removed"));
                    self.auto_save();
                }
                BackendEvent::AvatarsUpdated(avatars) => {
                    for (id, url) in avatars {
                        if let Some(acc) = self.store.find_by_id_mut(id) {
                            acc.avatar_url = url;
                        }
                    }
                }
                BackendEvent::AvatarImagesReady(images) => {
                    for (id, bytes) in images {
                        self.avatar_bytes.insert(id, bytes);
                    }
                }
                BackendEvent::PresencesUpdated(presences) => {
                    for (id, p) in presences {
                        if let Some(acc) = self.store.find_by_id_mut(id) {
                            acc.last_presence = p;
                        }
                    }
                }
                BackendEvent::GameLaunched => {
                    self.toasts.push(Toast::success("Game launched"));
                    if self.config.auto_arrange_windows {
                        self.bridge.send(BackendCommand::ArrangeWindows);
                    }
                }
                BackendEvent::BulkLaunchProgress { launched, total } => {
                    self.toasts
                        .push(Toast::info(format!("Launching {launched}/{total}...")));
                }
                BackendEvent::BulkLaunchComplete { launched, failed } => {
                    if failed == 0 {
                        self.toasts.push(Toast::success(format!(
                            "Bulk launch complete — {launched} launched"
                        )));
                    } else {
                        self.toasts.push(Toast::error(format!(
                            "Bulk launch done — {launched} launched, {failed} failed"
                        )));
                    }
                    if self.config.auto_arrange_windows {
                        self.bridge.send(BackendCommand::ArrangeWindows);
                    }
                }
                BackendEvent::StoreSaved => {
                    // silent
                }
                BackendEvent::StoreLoaded(store) => {
                    self.store = store;
                    self.needs_unlock = false;
                    self.toasts.push(Toast::success("Account store unlocked"));
                    self.trigger_refresh();
                    self.trigger_revalidation();
                }
                BackendEvent::Killed(count) => {
                    self.toasts
                        .push(Toast::info(format!("Killed {count} instance(s)")));
                }
                BackendEvent::WindowsArranged => {
                    // silent — arrangement complete
                }
                BackendEvent::AccountRevalidated {
                    user_id,
                    valid,
                    username,
                    display_name,
                } => {
                    if let Some(acc) = self.store.find_by_id_mut(user_id) {
                        if valid {
                            acc.last_validated = Some(chrono::Utc::now());
                            acc.username = username;
                            acc.display_name = display_name;
                            acc.cookie_expired = false;
                        } else {
                            acc.cookie_expired = true;
                        }
                    }
                    self.auto_save();
                    if !valid {
                        if let Some(acc) = self.store.find_by_id(user_id) {
                            let label = if self.config.anonymize_names {
                                "An account".to_string()
                            } else {
                                acc.label().to_string()
                            };
                            self.toasts.push(Toast::error(format!(
                                "Cookie expired for {label} — re-add with a fresh cookie"
                            )));
                        }
                    }
                }
                BackendEvent::Error(msg) => {
                    // If the add dialog is loading, show error there for retry
                    if self.add_dialog.loading {
                        self.add_dialog.loading = false;
                        self.add_dialog.last_error = Some(msg.clone());
                    }
                    self.toasts.push(Toast::error(msg));
                }
                BackendEvent::UpdateAvailable { version, url } => {
                    self.update_available = Some((version, url));
                }
                BackendEvent::PlaceResolved {
                    index,
                    place_name,
                    place_id,
                    icon_bytes,
                } => {
                    if let Some(server) = self.config.private_servers.get_mut(index) {
                        // Only update place_name if the new one is non-empty
                        // (don't overwrite good cached data on transient failures).
                        if !place_name.is_empty() {
                            server.place_name = place_name;
                            let _ = self.config.save(&self.config_path);
                        }
                    }
                    if let Some(bytes) = icon_bytes {
                        self.game_icon_bytes.insert(place_id, bytes);
                    }
                }
                BackendEvent::ShareLinkResolved {
                    server_name,
                    place_id,
                    universe_id,
                    link_code,
                    access_code,
                } => {
                    let server = PrivateServer {
                        name: server_name,
                        place_id,
                        universe_id,
                        link_code,
                        access_code,
                        place_name: String::new(),
                    };
                    let idx = self.config.private_servers.len();
                    self.config.private_servers.push(server);
                    let _ = self.config.save(&self.config_path);
                    // Auto-resolve the place name and icon
                    self.bridge.send(BackendCommand::ResolvePlace {
                        place_id,
                        universe_id,
                        index: idx,
                    });
                    self.toasts
                        .push(Toast::success("Share link resolved — private server added"));
                }
                BackendEvent::ShareLinkFailed(msg) => {
                    self.toasts
                        .push(Toast::error(format!("Failed to resolve share link: {msg}")));
                }
                BackendEvent::FriendsLoaded {
                    user_id: _,
                    friends,
                } => {
                    self.friends_state.friends = friends;
                    self.friends_state.loading = false;
                    self.friends_state.error = None;
                }
                BackendEvent::FriendRequestsLoaded {
                    user_id: _,
                    requests,
                } => {
                    self.friends_state.incoming_requests = requests;
                }
                BackendEvent::UserSearchResults { results } => {
                    self.friends_state.search_results = results;
                }
                BackendEvent::FriendRequestSent { target_user_id: _ } => {
                    self.toasts.push(Toast::success("Friend request sent!"));
                    // Refresh friends list
                    if let Some(uid) = self.friends_state.viewing_user_id {
                        self.friends_state.loading = true;
                        self.bridge.send(BackendCommand::FetchFriends {
                            user_id: uid,
                            encrypted_cookie: self
                                .store
                                .find_by_id(uid)
                                .and_then(|a| a.encrypted_cookie.clone()),
                            password: self.master_password.clone(),
                            use_credential_manager: self.config.use_credential_manager,
                        });
                    }
                }
                BackendEvent::FriendRequestAccepted { requester_id } => {
                    self.toasts.push(Toast::success("Friend request accepted!"));
                    self.friends_state
                        .incoming_requests
                        .retain(|r| r.user_id != requester_id);
                    // Refresh friends list
                    if let Some(uid) = self.friends_state.viewing_user_id {
                        self.friends_state.loading = true;
                        self.bridge.send(BackendCommand::FetchFriends {
                            user_id: uid,
                            encrypted_cookie: self
                                .store
                                .find_by_id(uid)
                                .and_then(|a| a.encrypted_cookie.clone()),
                            password: self.master_password.clone(),
                            use_credential_manager: self.config.use_credential_manager,
                        });
                    }
                }
                BackendEvent::FriendRequestDeclined { requester_id } => {
                    self.toasts.push(Toast::info("Friend request declined"));
                    self.friends_state
                        .incoming_requests
                        .retain(|r| r.user_id != requester_id);
                }
                BackendEvent::FriendGameResolved {
                    friend_user_id,
                    game_name,
                } => {
                    self.friends_state.selected_friend_game = Some(game_name);
                }
                BackendEvent::FriendGameJoined {
                    friend_user_id,
                    place_id,
                } => {
                    self.toasts
                        .push(Toast::info(format!("Joined friend's game")));
                }
                BackendEvent::GamesSearchResults { index, games } => {
                    self.games_state.loading_search = false;
                    self.games_state.search_results = games;
                }
                BackendEvent::GamesPopularLoaded { index, games } => {
                    self.games_state.loading_popular = false;
                    self.games_state.popular_games = games;
                }
                BackendEvent::FavoriteGamesLoaded { index, games } => {
                    self.games_state.loading_favorites = false;
                    self.games_state.favorite_games = games;
                }
                BackendEvent::VipServerCreated {
                    vip_server_id: _,
                    access_code,
                    name,
                    place_id,
                    ..
                } => {
                    self.toasts
                        .push(Toast::success(format!("VIP Server '{}' created!", name)));
                    if self.selected_ids.len() == 1 {
                        let uid = *self.selected_ids.iter().next().unwrap();
                        if let Some(acc) = self.store.find_by_id(uid) {
                            let launch_place_id = if place_id > 0 {
                                place_id
                            } else {
                                self.games_state
                                    .selected_game
                                    .as_ref()
                                    .map(|g| g.place_id)
                                    .unwrap_or(0)
                            };
                            self.bridge.send(BackendCommand::LaunchGameEncrypted {
                                user_id: acc.user_id,
                                encrypted_cookie: acc.encrypted_cookie.clone(),
                                password: self.master_password.clone(),
                                use_credential_manager: self.config.use_credential_manager,
                                place_id: launch_place_id,
                                job_id: None,
                                link_code: None,
                                access_code: Some(access_code),
                                multi_instance: self.config.multi_instance_enabled,
                                kill_background: self.config.kill_background_roblox,
                                privacy_mode: self.config.privacy_mode,
                            });
                        }
                    }
                }
                BackendEvent::VipServerPriceChecked {
                    universe_id,
                    allowed,
                    price,
                } => {
                    self.games_state.loading_vip_price = false;
                    if universe_id > 0 {
                        self.games_state
                            .vip_prices
                            .insert(universe_id, (price, allowed));
                    }
                    if !allowed {
                        self.toasts.push(Toast::info("VIP Servers are disabled"));
                    } else if price > 0 {
                        self.toasts
                            .push(Toast::info(format!("VIP Server: {} Robux", price)));
                    } else {
                        self.toasts.push(Toast::info("VIP Server: FREE"));
                    }
                }
                BackendEvent::VipServerPriceUnknown { universe_id } => {
                    self.games_state.loading_vip_price = false;
                    if universe_id > 0 {
                        self.games_state.vip_price_unknown.insert(universe_id, true);
                    }
                    self.toasts.push(Toast::info("VIP price unknown"));
                }
                BackendEvent::PrivateServersLoaded { index: _, servers } => {
                    self.games_state.loading_private_servers = false;
                    self.games_state.private_servers = servers;
                    if self.games_state.private_servers.is_empty() {
                        self.toasts.push(Toast::info("No private servers found"));
                    } else {
                        self.toasts.push(Toast::success(format!(
                            "Found {} private servers",
                            self.games_state.private_servers.len()
                        )));
                    }
                }
                BackendEvent::BulkImportProgress {
                    current,
                    total,
                    username,
                    proxy,
                    stage,
                } => {
                    self.bulk_import_progress =
                        Some((current, total, username, proxy.clone(), stage.clone()));
                }
                BackendEvent::ProxyTestProgress {
                    tested,
                    total,
                    working,
                    proxy,
                } => {
                    self.bulk_import_proxy_test = Some((tested, total, working, proxy));
                }
                BackendEvent::ProxyTestComplete { working, failed } => {
                    self.bulk_import_proxy_test = None;
                    self.toasts.push(Toast::info(format!(
                        "Proxy test: {}/{} working",
                        working.len(),
                        working.len() + failed.len()
                    )));
                }
                BackendEvent::BulkImportComplete {
                    added,
                    failed,
                    proxy_stats,
                } => {
                    self.bulk_import_progress = None;
                    self.bulk_import_result =
                        Some((added.clone(), failed.clone(), proxy_stats.clone()));
                    self.show_bulk_import_dialog = true;

                    // Add successfully imported accounts to the store
                    for (user_id, username, display_name) in &added {
                        // Check if account already exists
                        if self.store.find_by_id(*user_id).is_none() {
                            let account =
                                Account::new(*user_id, username.clone(), display_name.clone());
                            self.store.accounts.push(account);
                        }
                    }

                    // Save the updated store
                    if !added.is_empty() {
                        self.auto_save();
                        self.trigger_refresh();
                    }

                    if added.is_empty() && failed.is_empty() {
                        self.toasts.push(Toast::info("No cookies were processed"));
                    } else if added.is_empty() {
                        self.toasts
                            .push(Toast::error(format!("All {} cookies failed", failed.len())));
                    } else {
                        self.toasts
                            .push(Toast::success(format!("Added {} accounts", added.len())));
                    }
                }
                BackendEvent::CurrencyUpdated {
                    user_id,
                    robux,
                    is_premium,
                } => {
                    if let Some(acc) = self.store.find_by_id_mut(user_id) {
                        acc.robux_balance = Some(robux);
                        acc.is_premium = is_premium;
                        self.auto_save();
                    }
                    tracing::info!("Updated currency for user {}: {} Robux", user_id, robux);
                }
                BackendEvent::UserGroupsLoaded { user_id, groups } => {
                    let groups_with_robux: Vec<_> = groups.into_iter().map(|g| (g, None)).collect();
                    self.user_groups.insert(user_id, groups_with_robux);
                    tracing::info!("Groups loaded for user {}", user_id);
                }
                BackendEvent::GroupCurrencyLoaded {
                    group_id,
                    group_name,
                    robux,
                    pending: _,
                } => {
                    let robux_value = if robux == u64::MAX {
                        Some(u64::MAX)
                    } else {
                        Some(robux)
                    };
                    for groups in self.user_groups.values_mut() {
                        for (group, current_robux) in groups.iter_mut() {
                            if group.group_id == group_id {
                                *current_robux = robux_value;
                                if let Some(r) = robux_value {
                                    if r == u64::MAX {
                                        tracing::info!(
                                            "No permission to view Robux for group {} ({})",
                                            group_id,
                                            group_name
                                        );
                                    } else {
                                        tracing::info!(
                                            "Group {} ({}) has {} Robux",
                                            group_id,
                                            group_name,
                                            r
                                        );
                                    }
                                }
                                return;
                            }
                        }
                    }
                    if let Some(r) = robux_value {
                        if r != u64::MAX {
                            tracing::info!("Group {} ({}) has {} Robux", group_id, group_name, r);
                        } else {
                            tracing::info!(
                                "No permission to view Robux for group {} ({})",
                                group_id,
                                group_name
                            );
                        }
                    }
                }
            }
        }
    }

    fn auto_save(&self) {
        if !self.master_password.is_empty() {
            self.bridge.send(BackendCommand::SaveStore {
                store: self.store.clone(),
                path: self.config.accounts_path.clone(),
                password: self.master_password.clone(),
            });
        }
    }

    /// Get the first available cookie for API calls (decrypted from credential
    /// manager or in-memory encrypted cookie).
    fn first_account_with_cookie(&self) -> Option<&ram_core::models::Account> {
        self.store
            .accounts
            .iter()
            .find(|a| self.config.use_credential_manager || a.encrypted_cookie.is_some())
    }

    fn trigger_refresh(&self) {
        let user_ids: Vec<u64> = self.store.accounts.iter().map(|a| a.user_id).collect();
        if user_ids.is_empty() {
            return;
        }
        if let Some(first) = self.first_account_with_cookie() {
            self.bridge.send(BackendCommand::RefreshAll {
                user_ids,
                first_user_id: first.user_id,
                encrypted_cookie: first.encrypted_cookie.clone(),
                password: self.master_password.clone(),
                use_credential_manager: self.config.use_credential_manager,
            });
        }
    }

    /// Lightweight presence-only refresh for the currently visible accounts.
    fn trigger_presence_refresh(&self) {
        if self.visible_user_ids.is_empty() {
            return;
        }
        if let Some(first) = self.first_account_with_cookie() {
            self.bridge.send(BackendCommand::RefreshPresenceOnly {
                user_ids: self.visible_user_ids.clone(),
                first_user_id: first.user_id,
                encrypted_cookie: first.encrypted_cookie.clone(),
                password: self.master_password.clone(),
                use_credential_manager: self.config.use_credential_manager,
            });
        }
    }

    /// Resolve place names and game icons for private servers that are missing them.
    fn resolve_private_server_icons(&self) {
        for (i, server) in self.config.private_servers.iter().enumerate() {
            if server.place_name.is_empty() || !self.game_icon_bytes.contains_key(&server.place_id)
            {
                self.bridge.send(BackendCommand::ResolvePlace {
                    place_id: server.place_id,
                    universe_id: server.universe_id,
                    index: i,
                });
            }
        }
    }

    /// Revalidate all account cookies in the background.
    fn trigger_revalidation(&self) {
        if self.store.accounts.is_empty() {
            return;
        }
        let accounts: Vec<(u64, Option<String>)> = self
            .store
            .accounts
            .iter()
            .map(|a| (a.user_id, a.encrypted_cookie.clone()))
            .collect();
        self.bridge.send(BackendCommand::RevalidateAll {
            accounts,
            password: self.master_password.clone(),
            use_credential_manager: self.config.use_credential_manager,
        });
    }
}

// ---------------------------------------------------------------------------
// eframe::App
// ---------------------------------------------------------------------------

impl eframe::App for AppState {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame_count += 1;
        self.bridge.set_repaint_ctx(ctx.clone());

        // Check for window close request - warn if multi-instance is enabled and Roblox is running
        if ctx.input(|i| i.viewport().close_requested())
            && self.config.multi_instance_enabled
            && !self.config.ignore_multi_instance_warning
            && self.roblox_running
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            let result = rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Warning)
                .set_title("VH Roblox Manager — Warning")
                .set_description(
                    "Multi-instance is enabled!\n\n\
                    Roblox windows will close when you quit the program.\n\n\
                    Close Roblox windows manually first, or they will be terminated.\n\n\
                    Don't show this warning again?",
                )
                .set_buttons(rfd::MessageButtons::YesNo)
                .show();

            if result == rfd::MessageDialogResult::Yes {
                self.config.ignore_multi_instance_warning = true;
                let _ = self.config.save(&self.config_path);
            }
        }

        // Process all pending events first
        self.process_events();

        // Request repaint if we have ongoing bulk import or proxy test
        if self.bulk_import_progress.is_some() || self.bulk_import_proxy_test.is_some() {
            ctx.request_repaint();
        }

        // Periodically refresh roblox_running flag (every ~120 frames ≈ 2s)
        if self.frame_count.is_multiple_of(120) {
            self.roblox_running = ram_core::process::is_roblox_running();
        }

        // Periodically kill background tray Roblox processes when enabled
        // (every ~600 frames ≈ 10s)
        if (self.config.kill_background_roblox || self.config.multi_instance_enabled)
            && self.frame_count.is_multiple_of(600)
        {
            ram_core::process::kill_tray_roblox();
        }

        // Periodically refresh presence for visible accounts (every ~600 frames ≈ 10s)
        if self.frame_count.is_multiple_of(600) && !self.visible_user_ids.is_empty() {
            self.trigger_presence_refresh();
        }

        // Periodically refresh avatars for all accounts (every ~3600 frames ≈ 60s)
        if self.frame_count % 3600 == 300 && !self.store.accounts.is_empty() {
            self.trigger_refresh();
        }

        // Periodically revalidate all account cookies (every ~18000 frames ≈ 5 min)
        if self.frame_count % 18000 == 900 && !self.store.accounts.is_empty() {
            self.trigger_revalidation();
        }

        // Check for webview login result
        if let Some(ref mut rx) = self.webview_login_rx {
            match rx.try_recv() {
                Ok(msg) => {
                    self.webview_login_rx = None;
                    self.add_dialog.loading = false;
                    match msg {
                        crate::components::webview_login::WebViewLoginMsg::Cookie(cookie) => {
                            self.master_password = self.webview_login_password.clone();
                            self.add_dialog.cookie_input = cookie;
                            self.bridge.send(BackendCommand::AddAccount {
                                cookie: self.add_dialog.cookie_input.clone(),
                                password: self.master_password.clone(),
                                use_credential_manager: self.config.use_credential_manager,
                            });
                            self.add_dialog.loading = true;
                            self.add_dialog.last_error = None;
                        }
                        crate::components::webview_login::WebViewLoginMsg::Error(err) => {
                            self.add_dialog.last_error = Some(err);
                        }
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.webview_login_rx = None;
                    self.add_dialog.loading = false;
                }
            }
        }

        // ---- Unlock screen ----
        if self.needs_unlock {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(80.0);
                    ui.heading("🔒 VH | Unlock Account Store");
                    ui.add_space(16.0);
                    ui.label("Enter your master password to decrypt accounts:");
                    ui.add_space(8.0);

                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.unlock_password_input)
                            .password(true)
                            .hint_text("Master password"),
                    );

                    ui.add_space(8.0);
                    let enter_pressed =
                        response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

                    if ui.button("Unlock").clicked() || enter_pressed {
                        let pw = self.unlock_password_input.clone();
                        self.master_password = pw.clone();
                        self.bridge.send(BackendCommand::LoadStore {
                            path: self.config.accounts_path.clone(),
                            password: pw,
                        });
                    }
                });
            });
            self.toasts.show(ctx);
            return;
        }

        // ---- Top bar ----
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.active_tab, Tab::Accounts, "📋 Accounts");
                ui.selectable_value(&mut self.active_tab, Tab::Friends, "👥 Friends");
                ui.selectable_value(&mut self.active_tab, Tab::Games, "🎮 Games");
                ui.selectable_value(&mut self.active_tab, Tab::Settings, "⚙ Settings");
                ui.selectable_value(&mut self.active_tab, Tab::Donate, "💰 Donate");
                ui.selectable_value(&mut self.active_tab, Tab::About, "ℹ About");
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some((ref version, ref url)) = self.update_available {
                    let text = format!("⬆ Update v{version} available");
                    if ui
                        .link(text)
                        .on_hover_text("Click to open the download page")
                        .clicked()
                    {
                        ui.output_mut(|o| o.open_url = Some(egui::output::OpenUrl::new_tab(url)));
                    }
                    ui.separator();
                }
                if self.roblox_running {
                    ui.colored_label(egui::Color32::from_rgb(30, 144, 255), "● Roblox Running");
                }
                ui.label(format!("{} account(s)", self.store.accounts.len()));
            });
        });

        // ---- Status bar ----
        egui::TopBottomPanel::bottom("status_bar")
            .exact_height(22.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(format!("{} account(s)", self.store.accounts.len()));
                    ui.separator();
                    if self.roblox_running {
                        let count = ram_core::process::roblox_instance_count();
                        ui.colored_label(
                            egui::Color32::from_rgb(30, 144, 255),
                            format!("● {count} Roblox instance(s)"),
                        );
                    } else {
                        ui.colored_label(egui::Color32::GRAY, "○ Roblox not running");
                    }
                    ui.separator();
                    ui.label(format!("{} selected", self.selected_ids.len()));
                });
            });

        match self.active_tab {
            Tab::Accounts => self.show_accounts_tab(ctx),
            Tab::Friends => self.show_friends_tab(ctx),
            Tab::Games => self.show_games_tab(ctx),
            Tab::Settings => self.show_settings_tab(ctx),
            Tab::Donate => donate::show_donate_tab(ctx, &mut self.toasts),
            Tab::About => about::show_about_tab(ctx),
        }

        // ---- Floating add-account dialog ----
        self.show_add_dialog(ctx);

        // ---- Confirmation dialog for account removal ----
        self.show_confirm_remove_dialog(ctx);

        // ---- Changelog window ----
        self.show_changelog_window(ctx);

        // ---- Bulk import dialog ----
        self.show_bulk_import_dialog(ctx);

        // ---- First-launch tutorial overlay ----
        tutorial::show_overlay(ctx, &mut self.tutorial);

        // ---- Toasts ----
        self.toasts.show(ctx);
    }
}

// ---------------------------------------------------------------------------
// Tab rendering
// ---------------------------------------------------------------------------

impl AppState {
    fn show_accounts_tab(&mut self, ctx: &egui::Context) {
        // Sidebar
        egui::SidePanel::left("sidebar")
            .default_width(220.0)
            .width_range(140.0..=400.0)
            .resizable(true)
            .show(ctx, |ui| {
                let result = sidebar::show(
                    ui,
                    &mut self.sidebar_state,
                    &self.store.accounts,
                    &self.selected_ids,
                    self.config.anonymize_names,
                    &self.config.groups,
                );
                self.visible_user_ids = result.visible_user_ids;
                self.tutorial.add_btn_rect = result.add_btn_rect;
                self.tutorial.sidebar_accounts_rect = result.accounts_rect;
                // Tutorial: advance when the sidebar account list area is known
                if !self.selected_ids.is_empty() {
                    self.tutorial
                        .advance_from(tutorial::TutorialStep::SelectAccount);
                }
                for a in result.actions {
                    match a {
                        sidebar::SidebarAction::Select(id) => {
                            self.selected_ids.clear();
                            self.selected_ids.insert(id);
                        }
                        sidebar::SidebarAction::ToggleSelect(id) => {
                            if self.selected_ids.contains(&id) {
                                self.selected_ids.remove(&id);
                            } else {
                                self.selected_ids.insert(id);
                            }
                        }
                        sidebar::SidebarAction::RangeSelect(ids) => {
                            for id in ids {
                                self.selected_ids.insert(id);
                            }
                        }
                        sidebar::SidebarAction::AddAccountDialog => {
                            self.add_dialog.open = true;
                            self.add_dialog.cookie_input.clear();
                            self.add_dialog.last_error = None;
                            self.add_dialog.loading = false;
                            self.add_dialog.password_input = self.master_password.clone();
                            self.tutorial
                                .advance_from(tutorial::TutorialStep::AddAccount);
                        }
                        sidebar::SidebarAction::CopyJobId(job_id) => {
                            ui.output_mut(|o| o.copied_text = job_id.clone());
                            self.toasts.push(Toast::info("Copied to clipboard"));
                        }
                        sidebar::SidebarAction::QuickLaunch(user_id) => {
                            // Use the first favorite place, or fall back to the main panel place_id_input
                            let place_id = self
                                .config
                                .favorite_places
                                .first()
                                .map(|f| f.place_id)
                                .or_else(|| {
                                    self.main_panel_state.place_id_input.parse::<u64>().ok()
                                });
                            if let Some(place_id) = place_id {
                                if let Some(acc) = self.store.find_by_id(user_id) {
                                    self.bridge.send(BackendCommand::LaunchGameEncrypted {
                                        user_id: acc.user_id,
                                        encrypted_cookie: acc.encrypted_cookie.clone(),
                                        password: self.master_password.clone(),
                                        use_credential_manager: self.config.use_credential_manager,
                                        place_id,
                                        job_id: None,
                                        link_code: None,
                                        access_code: None,
                                        multi_instance: self.config.multi_instance_enabled,
                                        kill_background: self.config.kill_background_roblox,
                                        privacy_mode: self.config.privacy_mode,
                                    });
                                }
                            } else {
                                self.toasts.push(Toast::error(
                                    "No favorite place or Place ID set — enter one first",
                                ));
                            }
                        }
                        sidebar::SidebarAction::AssignGroup { user_ids, group } => {
                            for uid in &user_ids {
                                if let Some(acc) = self.store.find_by_id_mut(*uid) {
                                    acc.group = group.clone();
                                }
                            }
                            self.auto_save();
                        }
                        sidebar::SidebarAction::CreateGroup {
                            name,
                            color,
                            assign_user_ids,
                        } => {
                            self.config.groups.insert(
                                name.clone(),
                                ram_core::models::GroupMeta {
                                    color,
                                    description: String::new(),
                                    sort_order: u32::MAX,
                                },
                            );
                            for uid in &assign_user_ids {
                                if let Some(acc) = self.store.find_by_id_mut(*uid) {
                                    acc.group = name.clone();
                                }
                            }
                            let _ = self.config.save(&self.config_path);
                            self.auto_save();
                        }
                        sidebar::SidebarAction::DeleteGroup(name) => {
                            self.config.groups.remove(&name);
                            for acc in &mut self.store.accounts {
                                if acc.group == name {
                                    acc.group = String::new();
                                }
                            }
                            self.sidebar_state.collapsed_groups.remove(&name);
                            let _ = self.config.save(&self.config_path);
                            self.auto_save();
                        }
                        sidebar::SidebarAction::EditGroup {
                            old_name,
                            new_name,
                            color,
                        } => {
                            let old_meta = self.config.groups.remove(&old_name);
                            let desc = old_meta
                                .as_ref()
                                .map(|m| m.description.clone())
                                .unwrap_or_default();
                            let old_sort = old_meta.map(|m| m.sort_order).unwrap_or(u32::MAX);
                            self.config.groups.insert(
                                new_name.clone(),
                                ram_core::models::GroupMeta {
                                    color,
                                    description: desc,
                                    sort_order: old_sort,
                                },
                            );
                            if old_name != new_name {
                                for acc in &mut self.store.accounts {
                                    if acc.group == old_name {
                                        acc.group = new_name.clone();
                                    }
                                }
                                if self.sidebar_state.collapsed_groups.remove(&old_name) {
                                    self.sidebar_state.collapsed_groups.insert(new_name.clone());
                                }
                            }
                            let _ = self.config.save(&self.config_path);
                            self.auto_save();
                        }
                        sidebar::SidebarAction::ReorderAccount {
                            user_id,
                            target_user_id,
                            insert_after,
                        } => {
                            // Move `user_id` before or after `target_user_id` within the
                            // same group (or both ungrouped). Reassign sort_order values.
                            let group = self
                                .store
                                .find_by_id(user_id)
                                .map(|a| a.group.clone())
                                .unwrap_or_default();
                            // Collect accounts in this group, sorted by current sort_order then name.
                            let mut peers: Vec<(u32, String, u64)> = self
                                .store
                                .accounts
                                .iter()
                                .filter(|a| a.group == group)
                                .map(|a| (a.sort_order, a.label().to_lowercase(), a.user_id))
                                .collect();
                            peers.sort();
                            let mut ids: Vec<u64> =
                                peers.into_iter().map(|(_, _, id)| id).collect();
                            // Remove the dragged account.
                            if let Some(drag_pos) = ids.iter().position(|id| *id == user_id) {
                                ids.remove(drag_pos);
                            }
                            // Find target and insert before or after it.
                            let target_pos = ids
                                .iter()
                                .position(|id| *id == target_user_id)
                                .unwrap_or(ids.len());
                            let insert_pos = if insert_after {
                                target_pos + 1
                            } else {
                                target_pos
                            };
                            ids.insert(insert_pos.min(ids.len()), user_id);
                            // Reassign sequential sort_order values.
                            for (i, uid) in ids.iter().enumerate() {
                                if let Some(acc) = self.store.find_by_id_mut(*uid) {
                                    acc.sort_order = i as u32;
                                }
                            }
                            self.auto_save();
                        }
                        sidebar::SidebarAction::ReorderGroup {
                            group_name,
                            target_group,
                            insert_after,
                        } => {
                            // Move `group_name` before or after `target_group`.
                            let mut ordered: Vec<(u32, String)> = self
                                .config
                                .groups
                                .iter()
                                .map(|(name, meta)| (meta.sort_order, name.clone()))
                                .collect();
                            ordered.sort();
                            let mut names: Vec<String> =
                                ordered.into_iter().map(|(_, n)| n).collect();
                            if let Some(pos) = names.iter().position(|n| *n == group_name) {
                                names.remove(pos);
                            }
                            let target_pos = names
                                .iter()
                                .position(|n| *n == target_group)
                                .unwrap_or(names.len());
                            let insert_pos = if insert_after {
                                target_pos + 1
                            } else {
                                target_pos
                            };
                            names.insert(insert_pos.min(names.len()), group_name);
                            for (i, name) in names.iter().enumerate() {
                                if let Some(meta) = self.config.groups.get_mut(name) {
                                    meta.sort_order = i as u32;
                                }
                            }
                            let _ = self.config.save(&self.config_path);
                        }
                        sidebar::SidebarAction::ResetCustomOrder => {
                            // Clear all custom sort_order values.
                            for acc in &mut self.store.accounts {
                                acc.sort_order = u32::MAX;
                            }
                            for meta in self.config.groups.values_mut() {
                                meta.sort_order = u32::MAX;
                            }
                            let _ = self.config.save(&self.config_path);
                            self.auto_save();
                        }
                    }
                }
                // Persist sort mode if it changed.
                let current_mode = self.sidebar_state.sort_order.to_string();
                if self.config.sort_mode != current_mode {
                    self.config.sort_mode = current_mode;
                    let _ = self.config.save(&self.config_path);
                }
            });

        // Main panel — single selection shows detail, multi shows group panel
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.selected_ids.len() > 1 {
                // Group control panel
                let selected_accounts: Vec<&ram_core::models::Account> = self
                    .store
                    .accounts
                    .iter()
                    .filter(|a| self.selected_ids.contains(&a.user_id))
                    .collect();
                let action = group_panel::show(
                    ui,
                    &selected_accounts,
                    &mut self.group_panel_state,
                    self.roblox_running,
                    self.config.anonymize_names,
                    &self.config.favorite_places,
                );
                if let Some(a) = action {
                    match a {
                        group_panel::GroupPanelAction::BulkLaunch { place_id, job_id } => {
                            let accounts: Vec<(u64, Option<String>)> = self
                                .store
                                .accounts
                                .iter()
                                .filter(|a| self.selected_ids.contains(&a.user_id))
                                .map(|a| (a.user_id, a.encrypted_cookie.clone()))
                                .collect();
                            self.bridge.send(BackendCommand::BulkLaunchEncrypted {
                                accounts,
                                password: self.master_password.clone(),
                                use_credential_manager: self.config.use_credential_manager,
                                place_id,
                                job_id,
                                link_code: None,
                                access_code: None,
                                multi_instance: self.config.multi_instance_enabled,
                                kill_background: self.config.kill_background_roblox,
                                privacy_mode: self.config.privacy_mode,
                            });
                        }
                        group_panel::GroupPanelAction::BulkLaunchRoblox => {
                            for uid in &self.selected_ids {
                                if let Some(acc) = self.store.find_by_id(*uid) {
                                    self.bridge.send(BackendCommand::LaunchRoblox {
                                        user_id: acc.user_id,
                                        encrypted_cookie: acc.encrypted_cookie.clone(),
                                        password: self.master_password.clone(),
                                        use_credential_manager: self.config.use_credential_manager,
                                    });
                                }
                            }
                        }
                        group_panel::GroupPanelAction::ClearSelection => {
                            self.selected_ids.clear();
                        }
                        group_panel::GroupPanelAction::KillAll => {
                            self.bridge.send(BackendCommand::KillAll);
                        }
                    }
                }
            } else if self.selected_ids.len() == 1 {
                let id = *self.selected_ids.iter().next().unwrap();
                let account = self.store.find_by_id(id).cloned();
                if let Some(account) = account {
                    let avatar_bytes = self.avatar_bytes.get(&account.user_id);
                    let groups = self
                        .user_groups
                        .get(&account.user_id)
                        .cloned()
                        .unwrap_or_default();
                    let result = main_panel::show(
                        ui,
                        &account,
                        &mut self.main_panel_state,
                        self.roblox_running,
                        avatar_bytes,
                        &self.config.favorite_places,
                        self.config.anonymize_names,
                        &groups,
                    );
                    self.tutorial.launch_btn_rect = result.launch_btn_rect;
                    if let Some(a) = result.action {
                        match a {
                            main_panel::MainPanelAction::LaunchGame { place_id, job_id } => {
                                self.bridge.send(BackendCommand::LaunchGameEncrypted {
                                    user_id: account.user_id,
                                    encrypted_cookie: account.encrypted_cookie.clone(),
                                    password: self.master_password.clone(),
                                    use_credential_manager: self.config.use_credential_manager,
                                    place_id,
                                    job_id,
                                    link_code: None,
                                    access_code: None,
                                    multi_instance: self.config.multi_instance_enabled,
                                    kill_background: self.config.kill_background_roblox,
                                    privacy_mode: self.config.privacy_mode,
                                });
                            }
                            main_panel::MainPanelAction::LaunchRoblox => {
                                self.bridge.send(BackendCommand::LaunchRoblox {
                                    user_id: account.user_id,
                                    encrypted_cookie: account.encrypted_cookie.clone(),
                                    password: self.master_password.clone(),
                                    use_credential_manager: self.config.use_credential_manager,
                                });
                            }
                            main_panel::MainPanelAction::LaunchStudio => {
                                self.bridge.send(BackendCommand::LaunchStudio {
                                    user_id: account.user_id,
                                    encrypted_cookie: account.encrypted_cookie.clone(),
                                    password: self.master_password.clone(),
                                    use_credential_manager: self.config.use_credential_manager,
                                });
                            }
                            main_panel::MainPanelAction::RemoveAccount(uid) => {
                                self.confirm_remove = Some(uid);
                            }
                            main_panel::MainPanelAction::UpdateAlias { user_id, alias } => {
                                if let Some(acc) = self.store.find_by_id_mut(user_id) {
                                    acc.alias = alias;
                                }
                                self.auto_save();
                            }
                            main_panel::MainPanelAction::SaveFavorite { name, place_id } => {
                                self.config
                                    .favorite_places
                                    .push(ram_core::models::FavoritePlace { name, place_id });
                                let _ = self.config.save(&self.config_path);
                                self.toasts.push(Toast::success("Favorite saved"));
                            }
                            main_panel::MainPanelAction::RemoveFavorite(index) => {
                                if index < self.config.favorite_places.len() {
                                    self.config.favorite_places.remove(index);
                                    let _ = self.config.save(&self.config_path);
                                    self.toasts.push(Toast::info("Favorite removed"));
                                }
                            }
                            main_panel::MainPanelAction::KillAll => {
                                self.bridge.send(BackendCommand::KillAll);
                            }
                            main_panel::MainPanelAction::RefreshRobux => {
                                let acc = self.store.find_by_id(account.user_id);
                                if let Some(acc) = acc {
                                    self.bridge.send(BackendCommand::FetchCurrency {
                                        user_id: account.user_id,
                                        encrypted_cookie: acc.encrypted_cookie.clone(),
                                        password: self.master_password.clone(),
                                        use_credential_manager: self.config.use_credential_manager,
                                    });
                                }
                            }
                            main_panel::MainPanelAction::RefreshGroups => {
                                let acc = self.store.find_by_id(account.user_id);
                                if let Some(acc) = acc {
                                    self.toasts.push(Toast::info("Loading groups..."));
                                    self.bridge.send(BackendCommand::FetchUserGroups {
                                        user_id: account.user_id,
                                        encrypted_cookie: acc.encrypted_cookie.clone(),
                                        password: self.master_password.clone(),
                                        use_credential_manager: self.config.use_credential_manager,
                                    });
                                }
                            }
                            main_panel::MainPanelAction::FetchGroupRobux {
                                group_id,
                                group_name,
                            } => {
                                let acc = self.store.find_by_id(account.user_id);
                                if let Some(acc) = acc {
                                    self.bridge.send(BackendCommand::FetchGroupCurrency {
                                        user_id: account.user_id,
                                        group_id,
                                        group_name,
                                        encrypted_cookie: acc.encrypted_cookie.clone(),
                                        password: self.master_password.clone(),
                                        use_credential_manager: self.config.use_credential_manager,
                                    });
                                }
                            }
                        }
                    }
                } else {
                    main_panel::show_empty(ui);
                }
            } else {
                main_panel::show_empty(ui);
            }
        });

        // ---- Keyboard shortcuts ----
        let any_text_focused = ctx.memory(|m| m.focused().is_some());
        ctx.input(|i| {
            // Ctrl+A: select all accounts
            if i.modifiers.ctrl && i.key_pressed(egui::Key::A) && !any_text_focused {
                for acc in &self.store.accounts {
                    self.selected_ids.insert(acc.user_id);
                }
            }
            // Escape: clear selection
            if i.key_pressed(egui::Key::Escape) {
                self.selected_ids.clear();
            }
            // Delete: prompt to remove selected account(s)
            if i.key_pressed(egui::Key::Delete) && !any_text_focused && self.selected_ids.len() == 1
            {
                let uid = *self.selected_ids.iter().next().unwrap();
                self.confirm_remove = Some(uid);
            }
        });
    }

    fn show_friends_tab(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let selected_user_id = if self.selected_ids.len() == 1 {
                self.selected_ids.iter().next().copied()
            } else {
                None
            };

            let action = friends::show(
                ui,
                &mut self.friends_state,
                selected_user_id,
                &self.avatar_bytes,
            );

            if let Some(a) = action {
                match a {
                    friends::FriendsAction::FetchFriends { user_id } => {
                        self.friends_state.loading = true;
                        self.bridge.send(BackendCommand::FetchFriends {
                            user_id,
                            encrypted_cookie: self
                                .store
                                .find_by_id(user_id)
                                .and_then(|a| a.encrypted_cookie.clone()),
                            password: self.master_password.clone(),
                            use_credential_manager: self.config.use_credential_manager,
                        });
                    }
                    friends::FriendsAction::FetchRequests { user_id } => {
                        self.bridge.send(BackendCommand::FetchFriendRequests {
                            user_id,
                            encrypted_cookie: self
                                .store
                                .find_by_id(user_id)
                                .and_then(|a| a.encrypted_cookie.clone()),
                            password: self.master_password.clone(),
                            use_credential_manager: self.config.use_credential_manager,
                        });
                    }
                    friends::FriendsAction::SearchUsers { query } => {
                        self.bridge.send(BackendCommand::SearchUsers { query });
                    }
                    friends::FriendsAction::SendRequest {
                        user_id,
                        target_user_id,
                    } => {
                        self.bridge.send(BackendCommand::SendFriendRequest {
                            user_id,
                            target_user_id,
                            encrypted_cookie: self
                                .store
                                .find_by_id(user_id)
                                .and_then(|a| a.encrypted_cookie.clone()),
                            password: self.master_password.clone(),
                            use_credential_manager: self.config.use_credential_manager,
                        });
                    }
                    friends::FriendsAction::AcceptRequest {
                        user_id,
                        requester_id,
                    } => {
                        self.bridge.send(BackendCommand::AcceptFriendRequest {
                            user_id,
                            requester_id,
                            encrypted_cookie: self
                                .store
                                .find_by_id(user_id)
                                .and_then(|a| a.encrypted_cookie.clone()),
                            password: self.master_password.clone(),
                            use_credential_manager: self.config.use_credential_manager,
                        });
                    }
                    friends::FriendsAction::DeclineRequest {
                        user_id,
                        requester_id,
                    } => {
                        self.bridge.send(BackendCommand::DeclineFriendRequest {
                            user_id,
                            requester_id,
                            encrypted_cookie: self
                                .store
                                .find_by_id(user_id)
                                .and_then(|a| a.encrypted_cookie.clone()),
                            password: self.master_password.clone(),
                            use_credential_manager: self.config.use_credential_manager,
                        });
                    }
                    friends::FriendsAction::Refresh { user_id } => {
                        self.friends_state.loading = true;
                        self.friends_state.friends.clear();
                        self.friends_state.incoming_requests.clear();
                        self.bridge.send(BackendCommand::FetchFriends {
                            user_id,
                            encrypted_cookie: self
                                .store
                                .find_by_id(user_id)
                                .and_then(|a| a.encrypted_cookie.clone()),
                            password: self.master_password.clone(),
                            use_credential_manager: self.config.use_credential_manager,
                        });
                        self.bridge.send(BackendCommand::FetchFriendRequests {
                            user_id,
                            encrypted_cookie: self
                                .store
                                .find_by_id(user_id)
                                .and_then(|a| a.encrypted_cookie.clone()),
                            password: self.master_password.clone(),
                            use_credential_manager: self.config.use_credential_manager,
                        });
                    }
                    friends::FriendsAction::SelectFriendGame {
                        user_id,
                        friend_user_id,
                        place_id,
                    } => {
                        self.friends_state.selected_friend_id = Some(friend_user_id);
                        self.friends_state.selected_friend_game = None;
                        self.bridge.send(BackendCommand::ResolveFriendPlace {
                            user_id,
                            friend_user_id,
                            place_id,
                            encrypted_cookie: self
                                .store
                                .find_by_id(user_id)
                                .and_then(|a| a.encrypted_cookie.clone()),
                            password: self.master_password.clone(),
                            use_credential_manager: self.config.use_credential_manager,
                        });
                    }
                    friends::FriendsAction::JoinFriendGame { user_id, place_id } => {
                        let cookie = self
                            .store
                            .find_by_id(user_id)
                            .and_then(|a| a.encrypted_cookie.clone());
                        self.bridge.send(BackendCommand::LaunchGameEncrypted {
                            user_id,
                            encrypted_cookie: cookie,
                            password: self.master_password.clone(),
                            use_credential_manager: self.config.use_credential_manager,
                            place_id,
                            job_id: None,
                            link_code: None,
                            access_code: None,
                            multi_instance: self.config.multi_instance_enabled,
                            kill_background: self.config.kill_background_roblox,
                            privacy_mode: self.config.privacy_mode,
                        });
                    }
                }
            }
        });
    }

    fn show_games_tab(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let action = game_panel::show_games_tab(ui, &mut self.games_state);
            if let Some(a) = action {
                match a {
                    game_panel::GameAction::Search { query } => {
                        if self.selected_ids.len() == 1 {
                            let uid = *self.selected_ids.iter().next().unwrap();
                            if let Some(acc) = self.store.find_by_id(uid) {
                                self.games_state.loading_search = true;
                                self.games_state.search_results.clear();
                                self.bridge.send(BackendCommand::SearchGames {
                                    user_id: uid,
                                    query,
                                    index: 0,
                                    encrypted_cookie: acc.encrypted_cookie.clone(),
                                    password: self.master_password.clone(),
                                    use_credential_manager: self.config.use_credential_manager,
                                });
                            }
                        } else {
                            self.toasts.push(Toast::info("Select an account first"));
                        }
                    }
                    game_panel::GameAction::RefreshPopular => {
                        if self.selected_ids.len() == 1 {
                            let uid = *self.selected_ids.iter().next().unwrap();
                            if let Some(acc) = self.store.find_by_id(uid) {
                                self.games_state.loading_popular = true;
                                self.games_state.popular_games.clear();
                                self.bridge.send(BackendCommand::GetPopularGames {
                                    user_id: uid,
                                    index: 0,
                                    encrypted_cookie: acc.encrypted_cookie.clone(),
                                    password: self.master_password.clone(),
                                    use_credential_manager: self.config.use_credential_manager,
                                });
                            }
                        } else {
                            self.toasts.push(Toast::info("Select an account first"));
                        }
                    }
                    game_panel::GameAction::RefreshFavorites => {
                        if self.selected_ids.len() == 1 {
                            let uid = *self.selected_ids.iter().next().unwrap();
                            if let Some(acc) = self.store.find_by_id(uid) {
                                self.games_state.loading_favorites = true;
                                self.games_state.favorite_games.clear();
                                self.bridge.send(BackendCommand::GetFavoriteGames {
                                    user_id: uid,
                                    index: 0,
                                    encrypted_cookie: acc.encrypted_cookie.clone(),
                                    password: self.master_password.clone(),
                                    use_credential_manager: self.config.use_credential_manager,
                                });
                            }
                        } else {
                            self.toasts.push(Toast::info("Select an account first"));
                        }
                    }
                    game_panel::GameAction::Launch { place_id } => {
                        if self.selected_ids.len() == 1 {
                            let uid = *self.selected_ids.iter().next().unwrap();
                            if let Some(acc) = self.store.find_by_id(uid) {
                                self.bridge.send(BackendCommand::LaunchGameEncrypted {
                                    user_id: acc.user_id,
                                    encrypted_cookie: acc.encrypted_cookie.clone(),
                                    password: self.master_password.clone(),
                                    use_credential_manager: self.config.use_credential_manager,
                                    place_id,
                                    job_id: None,
                                    link_code: None,
                                    access_code: None,
                                    multi_instance: self.config.multi_instance_enabled,
                                    kill_background: self.config.kill_background_roblox,
                                    privacy_mode: self.config.privacy_mode,
                                });
                            }
                        } else {
                            self.toasts
                                .push(Toast::info("Select an account to launch game"));
                        }
                    }
                    game_panel::GameAction::AddFavorite {
                        user_id: _,
                        place_id: _,
                    } => {
                        // TODO: Implement add to favorites
                    }
                    game_panel::GameAction::LaunchPrivateServer {
                        access_code,
                        place_id,
                    } => {
                        if self.selected_ids.len() == 1 {
                            let uid = *self.selected_ids.iter().next().unwrap();
                            if let Some(acc) = self.store.find_by_id(uid) {
                                self.bridge.send(BackendCommand::LaunchGameEncrypted {
                                    user_id: acc.user_id,
                                    encrypted_cookie: acc.encrypted_cookie.clone(),
                                    password: self.master_password.clone(),
                                    use_credential_manager: self.config.use_credential_manager,
                                    place_id,
                                    job_id: None,
                                    link_code: None,
                                    access_code: Some(access_code),
                                    multi_instance: self.config.multi_instance_enabled,
                                    kill_background: self.config.kill_background_roblox,
                                    privacy_mode: self.config.privacy_mode,
                                });
                            }
                        } else {
                            self.toasts.push(Toast::info("Select an account first"));
                        }
                    }
                    game_panel::GameAction::CreateVipServer {
                        universe_id,
                        place_id,
                        name,
                    } => {
                        if self.selected_ids.len() == 1 {
                            let uid = *self.selected_ids.iter().next().unwrap();
                            if let Some(acc) = self.store.find_by_id(uid) {
                                self.bridge.send(BackendCommand::CreateVipServer {
                                    universe_id,
                                    place_id,
                                    name,
                                    index: 0,
                                    encrypted_cookie: acc.encrypted_cookie.clone(),
                                    password: self.master_password.clone(),
                                    use_credential_manager: self.config.use_credential_manager,
                                });
                            }
                        } else {
                            self.toasts.push(Toast::info("Select an account first"));
                        }
                    }
                    game_panel::GameAction::CheckVipPrice {
                        universe_id,
                        place_id,
                    } => {
                        if self.selected_ids.len() == 1 {
                            let uid = *self.selected_ids.iter().next().unwrap();
                            if let Some(acc) = self.store.find_by_id(uid) {
                                self.games_state.loading_vip_price = true;
                                self.bridge.send(BackendCommand::CheckVipServerPrice {
                                    universe_id,
                                    place_id,
                                    encrypted_cookie: acc.encrypted_cookie.clone(),
                                    password: self.master_password.clone(),
                                    use_credential_manager: self.config.use_credential_manager,
                                });
                            }
                        } else {
                            self.toasts.push(Toast::info("Select an account first"));
                        }
                    }
                    game_panel::GameAction::ListPrivateServers { place_id } => {
                        if self.selected_ids.len() == 1 {
                            let uid = *self.selected_ids.iter().next().unwrap();
                            if let Some(acc) = self.store.find_by_id(uid) {
                                self.games_state.loading_private_servers = true;
                                self.games_state.private_servers.clear();
                                self.bridge.send(BackendCommand::ListPrivateServers {
                                    place_id,
                                    index: 0,
                                    encrypted_cookie: acc.encrypted_cookie.clone(),
                                    password: self.master_password.clone(),
                                    use_credential_manager: self.config.use_credential_manager,
                                });
                            }
                        } else {
                            self.toasts.push(Toast::info("Select an account first"));
                        }
                    }
                }
            }
        });
    }

    fn show_private_servers_tab(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let has_selection = !self.selected_ids.is_empty();
            let action = private_servers::show(
                ui,
                &mut self.private_servers_state,
                &self.config.private_servers,
                has_selection,
                &self.game_icon_bytes,
            );
            if let Some(a) = action {
                match a {
                    private_servers::PrivateServerAction::Add(server) => {
                        let idx = self.config.private_servers.len();
                        let place_id = server.place_id;
                        let universe_id = server.universe_id;
                        self.config.private_servers.push(server);
                        let _ = self.config.save(&self.config_path);
                        // Auto-resolve the place name
                        self.bridge.send(BackendCommand::ResolvePlace {
                            place_id,
                            universe_id,
                            index: idx,
                        });
                        self.toasts.push(Toast::success("Private server added"));
                    }
                    private_servers::PrivateServerAction::Remove(idx) => {
                        if idx < self.config.private_servers.len() {
                            self.config.private_servers.remove(idx);
                            let _ = self.config.save(&self.config_path);
                            self.toasts.push(Toast::info("Private server removed"));
                        }
                    }
                    private_servers::PrivateServerAction::Launch {
                        place_id,
                        link_code,
                        access_code,
                    } => {
                        let ac = if access_code.is_empty() {
                            None
                        } else {
                            Some(access_code.clone())
                        };
                        if self.selected_ids.len() == 1 {
                            let uid = *self.selected_ids.iter().next().unwrap();
                            if let Some(acc) = self.store.find_by_id(uid) {
                                self.bridge.send(BackendCommand::LaunchGameEncrypted {
                                    user_id: acc.user_id,
                                    encrypted_cookie: acc.encrypted_cookie.clone(),
                                    password: self.master_password.clone(),
                                    use_credential_manager: self.config.use_credential_manager,
                                    place_id,
                                    job_id: None,
                                    link_code: Some(link_code.clone()),
                                    access_code: ac.clone(),
                                    multi_instance: self.config.multi_instance_enabled,
                                    kill_background: self.config.kill_background_roblox,
                                    privacy_mode: self.config.privacy_mode,
                                });
                            }
                        } else if self.selected_ids.len() > 1 {
                            let accounts: Vec<(u64, Option<String>)> = self
                                .store
                                .accounts
                                .iter()
                                .filter(|a| self.selected_ids.contains(&a.user_id))
                                .map(|a| (a.user_id, a.encrypted_cookie.clone()))
                                .collect();
                            self.bridge.send(BackendCommand::BulkLaunchEncrypted {
                                accounts,
                                password: self.master_password.clone(),
                                use_credential_manager: self.config.use_credential_manager,
                                place_id,
                                job_id: None,
                                link_code: Some(link_code),
                                access_code: ac,
                                multi_instance: self.config.multi_instance_enabled,
                                kill_background: self.config.kill_background_roblox,
                                privacy_mode: self.config.privacy_mode,
                            });
                        }
                    }
                    private_servers::PrivateServerAction::Resolve(idx) => {
                        if let Some(server) = self.config.private_servers.get(idx) {
                            self.bridge.send(BackendCommand::ResolvePlace {
                                place_id: server.place_id,
                                universe_id: server.universe_id,
                                index: idx,
                            });
                        }
                    }
                    private_servers::PrivateServerAction::ResolveShareLink {
                        share_code,
                        server_name,
                    } => {
                        // Need an authenticated account to resolve share links
                        if let Some(acc) = self.store.accounts.first() {
                            self.bridge.send(BackendCommand::ResolveShareLink {
                                share_code,
                                server_name,
                                first_user_id: acc.user_id,
                                encrypted_cookie: acc.encrypted_cookie.clone(),
                                password: self.master_password.clone(),
                                use_credential_manager: self.config.use_credential_manager,
                            });
                            self.toasts.push(Toast::info("Resolving share link..."));
                        } else {
                            self.toasts.push(Toast::error(
                                "Add at least one account before using share links",
                            ));
                        }
                    }
                }
            }
        });
    }

    fn show_settings_tab(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let has_password = !self.master_password.is_empty();
            let action = settings::show(
                ui,
                &mut self.config,
                has_password,
                &mut self.settings_state,
                self.roblox_running,
                self.afk_preventer_active.clone(),
            );
            match action {
                Some(settings::SettingsAction::SaveConfig) => {
                    if let Err(e) = self.config.save(&self.config_path) {
                        self.toasts
                            .push(Toast::error(format!("Save failed: {e}")));
                    } else {
                        self.toasts.push(Toast::success("Settings saved"));
                    }
                }
                Some(settings::SettingsAction::EnableMultiInstance) => {
                    if self.roblox_running {
                        // Kill tray processes first, then check again
                        ram_core::process::kill_tray_roblox();
                        // Brief wait for the OS to reap terminated processes
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        // Re-check after killing tray processes
                        let still_running = ram_core::process::is_roblox_running();
                        if still_running {
                            self.toasts.push(Toast::error(
                                "Close all Roblox instances (including tray) before enabling multi-instance.",
                            ));
                            // Don't enable — the checkbox was toggled but we
                            // leave config unchanged, so next frame it resets.
                        } else {
                            // Tray killed, nothing else running — safe to acquire
                            match ram_core::process::enable_multi_instance() {
                                Ok(()) => {
                                    self.config.multi_instance_enabled = true;
                                    let _ = self.config.save(&self.config_path);
                                    self.toasts.push(Toast::success("Multi-instance enabled & saved"));
                                }
                                Err(e) => {
                                    self.toasts.push(Toast::error(format!("Failed: {e}")));
                                }
                            }
                        }
                    } else {
                        match ram_core::process::enable_multi_instance() {
                            Ok(()) => {
                                self.config.multi_instance_enabled = true;
                                let _ = self.config.save(&self.config_path);
                                self.toasts.push(Toast::success("Multi-instance enabled & saved"));
                            }
                            Err(e) => {
                                self.toasts.push(Toast::error(format!("Failed: {e}")));
                            }
                        }
                    }
                }
                Some(settings::SettingsAction::DisableMultiInstance) => {
                    self.config.multi_instance_enabled = false;
                    let _ = self.config.save(&self.config_path);
                    self.toasts.push(Toast::info("Multi-instance disabled & saved (takes effect after restart)"));
                }
                Some(settings::SettingsAction::ChangePassword { new_password }) => {
                    let old_password = self.master_password.clone();
                    // Re-encrypt every account's cookie with the new password
                    for account in &mut self.store.accounts {
                        if let Some(ref enc) = account.encrypted_cookie {
                            if let Ok(plain) = ram_core::crypto::decrypt_cookie(enc, &old_password) {
                                if let Ok(new_enc) = ram_core::crypto::encrypt_cookie(&plain, &new_password) {
                                    account.encrypted_cookie = Some(new_enc);
                                }
                            }
                        }
                    }
                    self.master_password = new_password;
                    self.auto_save();
                    self.toasts.push(Toast::success("Password changed - store re-encrypted"));
                }
                Some(settings::SettingsAction::ClearPassword) => {
                    self.master_password.clear();
                    self.toasts.push(Toast::info("Password cleared"));
                }
                Some(settings::SettingsAction::MultiTabOrganize) => {
                    match ram_core::process::organize_roblox_windows() {
                        Ok(count) => self.toasts.push(Toast::success(format!("Organized {} windows", count))),
                        Err(e) => self.toasts.push(Toast::error(format!("Failed: {}", e))),
                    }
                }
                Some(settings::SettingsAction::MultiTabMinimizeAll) => {
                    match ram_core::process::minimize_all_roblox() {
                        Ok(count) => self.toasts.push(Toast::success(format!("Minimized {} windows", count))),
                        Err(e) => self.toasts.push(Toast::error(format!("Failed: {}", e))),
                    }
                }
                Some(settings::SettingsAction::MultiTabRestoreAll) => {
                    match ram_core::process::restore_all_roblox() {
                        Ok(count) => self.toasts.push(Toast::success(format!("Restored {} windows", count))),
                        Err(e) => self.toasts.push(Toast::error(format!("Failed: {}", e))),
                    }
                }
                Some(settings::SettingsAction::MultiTabMemoryCleanup) => {
                    match ram_core::process::memory_cleanup_all_roblox() {
                        Ok(count) => self.toasts.push(Toast::success(format!("Cleaned {} processes", count))),
                        Err(e) => self.toasts.push(Toast::error(format!("Failed: {}", e))),
                    }
                }
                Some(settings::SettingsAction::MultiTabStartAfkPreventer) => {
                    // Stop existing thread if any
                    if let Some(handle) = self.afk_preventer_handle.take() {
                        self.afk_preventer_active.store(false, Ordering::Relaxed);
                        handle.join().ok();
                    }
                    
                    // Reset flag
                    self.afk_preventer_active.store(true, Ordering::Relaxed);
                    
                    // Spawn background thread for AFK prevention
                    let stop = self.afk_preventer_active.clone();
                    let handle = std::thread::spawn(move || {
                        loop {
                            // Check stop flag before sleeping
                            if !stop.load(Ordering::Relaxed) {
                                tracing::info!("AFK Prevention: Thread stopping");
                                break;
                            }
                            
                            std::thread::sleep(std::time::Duration::from_secs(600)); // 10 minutes
                            
                            // Check again after sleep
                            if !stop.load(Ordering::Relaxed) {
                                tracing::info!("AFK Prevention: Thread stopping after sleep");
                                break;
                            }
                            
                            // Send ESC heartbeat with 4 variations (SendInput, PostMessageW, keybd_event, SendInput again)
                            if let Ok(count) = ram_core::process::send_esc_to_all_roblox() {
                                if count > 0 {
                                    tracing::info!("AFK Prevention: Sent ESC to {} windows", count);
                                }
                            }
                        }
                    });
                    
                    self.afk_preventer_handle = Some(handle);
                    self.toasts.push(Toast::info("AFK Prevention started (every 10 min)"));
                }
                Some(settings::SettingsAction::MultiTabStopAfkPreventer) => {
                    self.afk_preventer_active.store(false, Ordering::Relaxed);
                    self.toasts.push(Toast::info("AFK Prevention stopped"));
                }
                Some(settings::SettingsAction::MultiTabTestAfk) => {
                    // Test AFK immediately - manual test
                    if let Ok(count) = ram_core::process::send_esc_to_all_roblox() {
                        self.toasts.push(Toast::info(format!("AFK Test: Sent to {} windows, check logs!", count)));
                    } else {
                        self.toasts.push(Toast::error("AFK Test failed"));
                    }
                }
                Some(settings::SettingsAction::BulkImportCookies { cookie_file, proxy_file }) => {
                    // Read the cookie file and parse cookies
                    match std::fs::read_to_string(&cookie_file) {
                        Ok(content) => {
                            let cookies: Vec<String> = content
                                .lines()
                                .filter(|line| line.trim().starts_with("_|WARNING:-DO-NOT-SHARE-THIS"))
                                .map(|s| s.trim().to_string())
                                .collect();
                            
                            if cookies.is_empty() {
                                self.toasts.push(Toast::error("No valid cookies found in file"));
                            } else {
                                // Show the dialog immediately when starting
                                self.bulk_import_progress = Some((0, cookies.len(), String::new(), None, "Initializing...".to_string()));
                                self.show_bulk_import_dialog = true;
                                self.toasts.push(Toast::info(format!("Starting import of {} cookies...", cookies.len())));
                                self.bridge.send(BackendCommand::BulkImportCookies {
                                    cookies,
                                    password: self.master_password.clone(),
                                    use_credential_manager: self.config.use_credential_manager,
                                    proxy_file,
                                    use_cached_proxies: true,  // Default: use cached proxies
                                    force_recheck_proxies: false,  // Default: don't force recheck
                                });
                            }
                        }
                        Err(e) => {
                            self.toasts.push(Toast::error(format!("Failed to read cookie file: {}", e)));
                        }
                    }
                }
                None => {}
            }
        });
    }

    fn show_add_dialog(&mut self, ctx: &egui::Context) {
        if !self.add_dialog.open {
            return;
        }

        let mut open = self.add_dialog.open;
        egui::Window::new("Add Account")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("Paste or type the .ROBLOSECURITY cookie:");
                ui.add_space(4.0);

                egui::ScrollArea::vertical()
                    .max_height(100.0)
                    .show(ui, |ui| {
                        let cookie_edit =
                            egui::TextEdit::multiline(&mut self.add_dialog.cookie_input)
                                .desired_rows(3)
                                .hint_text("_|WARNING:-DO-NOT-SHARE-THIS...");
                        let cookie_resp = ui.add_enabled(!self.add_dialog.loading, cookie_edit);
                        self.tutorial.cookie_field_rect = cookie_resp.rect;
                    });
                ui.add_space(8.0);

                ui.separator();
                ui.add_space(4.0);

                if ui.button("🌐  Login with Browser").clicked() {
                    if !self.add_dialog.password_input.is_empty() {
                        self.webview_login_password = self.add_dialog.password_input.clone();
                        self.master_password = self.add_dialog.password_input.clone();
                        let rx = crate::components::webview_login::spawn_webview_login();
                        self.webview_login_rx = Some(rx);
                        self.add_dialog.loading = true;
                        self.add_dialog.last_error = None;
                    } else {
                        self.add_dialog.last_error =
                            Some("Set a master password first (same field above).".to_string());
                    }
                }
                ui.label("Login to Roblox in the popup window. Cookie is captured automatically.");

                ui.add_space(8.0);

                // Always show password field — uses a staging buffer so
                // partial input is never committed.
                ui.label(if self.master_password.is_empty() {
                    "Set a master password for encryption:"
                } else {
                    "Master password:"
                });
                ui.add_enabled(
                    !self.add_dialog.loading,
                    egui::TextEdit::singleline(&mut self.add_dialog.password_input)
                        .password(true)
                        .hint_text("Master password"),
                );
                ui.add_space(4.0);

                // Show error from last attempt with retry option
                if let Some(err) = &self.add_dialog.last_error {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.colored_label(egui::Color32::from_rgb(200, 60, 60), format!("⚠ {err}"));
                    });
                    ui.add_space(4.0);
                }

                if self.add_dialog.loading {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Validating cookie...");
                    });
                } else {
                    let valid = !self.add_dialog.cookie_input.trim().is_empty()
                        && !self.add_dialog.password_input.is_empty();

                    let button_label = if self.add_dialog.last_error.is_some() {
                        "Retry"
                    } else {
                        "Add"
                    };

                    if ui
                        .add_enabled(valid, egui::Button::new(button_label))
                        .clicked()
                    {
                        let cookie = self.add_dialog.cookie_input.trim().to_string();
                        // Commit the password only on explicit submit
                        self.master_password = self.add_dialog.password_input.clone();
                        self.add_dialog.loading = true;
                        self.add_dialog.last_error = None;
                        self.bridge.send(BackendCommand::AddAccount {
                            cookie,
                            password: self.master_password.clone(),
                            use_credential_manager: self.config.use_credential_manager,
                        });
                    }
                }
            });
        self.add_dialog.open = open;
    }

    fn show_confirm_remove_dialog(&mut self, ctx: &egui::Context) {
        let Some(uid) = self.confirm_remove else {
            return;
        };
        let label = if self.config.anonymize_names {
            "this account".to_string()
        } else {
            self.store
                .find_by_id(uid)
                .map(|a| a.label().to_string())
                .unwrap_or_else(|| uid.to_string())
        };

        let mut keep_open = true;
        egui::Window::new("Confirm Removal")
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(format!(
                    "Remove account \"{label}\"? This cannot be undone."
                ));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("🗑  Remove").clicked() {
                        self.bridge
                            .send(BackendCommand::RemoveAccount { user_id: uid });
                        keep_open = false;
                    }
                    if ui.button("Cancel").clicked() {
                        keep_open = false;
                    }
                });
            });
        if !keep_open {
            self.confirm_remove = None;
        }
    }

    fn show_changelog_window(&mut self, ctx: &egui::Context) {
        if !self.show_changelog {
            return;
        }
        let mut open = true;
        egui::Window::new(format!("What's New in v{}", env!("CARGO_PKG_VERSION")))
            .open(&mut open)
            .resizable(true)
            .default_width(480.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(400.0)
                    .show(ui, |ui| {
                        let changelog = include_str!("../../CHANGELOG.md");
                        // Show only the section for the current version
                        let current = format!("## v{}", env!("CARGO_PKG_VERSION"));
                        let section = if let Some(start) = changelog.find(&current) {
                            let rest = &changelog[start..];
                            let end = rest[current.len()..]
                                .find("\n## v")
                                .map(|i| i + current.len())
                                .unwrap_or(rest.len());
                            &rest[..end]
                        } else {
                            changelog
                        };
                        // Render markdown-lite
                        for line in section.lines() {
                            let trimmed = line.trim();
                            if trimmed.is_empty() {
                                ui.add_space(2.0);
                            } else if let Some(h) = trimmed.strip_prefix("### ") {
                                ui.add_space(4.0);
                                ui.strong(h);
                            } else if let Some(h) = trimmed.strip_prefix("## ") {
                                ui.heading(h);
                            } else if let Some(item) = trimmed.strip_prefix("- ") {
                                Self::render_md_line(ui, &format!("  • {item}"));
                            } else {
                                Self::render_md_line(ui, trimmed);
                            }
                        }
                    });
                ui.add_space(8.0);
                if ui.button("Close").clicked() {
                    self.show_changelog = false;
                }
            });
        if !open {
            self.show_changelog = false;
        }
    }

    /// Render a single line with **bold** spans converted to egui RichText.
    fn render_md_line(ui: &mut egui::Ui, line: &str) {
        let mut job = egui::text::LayoutJob::default();
        let style = ui.style();
        let normal_color = style.visuals.text_color();
        let normal_font = egui::FontId::proportional(14.0);
        let bold_font = egui::FontId {
            size: 14.0,
            family: egui::FontFamily::Proportional,
        };

        let mut remaining = line;
        while let Some(start) = remaining.find("**") {
            // Text before the bold marker
            let before = &remaining[..start];
            if !before.is_empty() {
                job.append(
                    before,
                    0.0,
                    egui::text::TextFormat {
                        font_id: normal_font.clone(),
                        color: normal_color,
                        ..Default::default()
                    },
                );
            }
            remaining = &remaining[start + 2..];
            // Find the closing **
            if let Some(end) = remaining.find("**") {
                let bold_text = &remaining[..end];
                job.append(
                    bold_text,
                    0.0,
                    egui::text::TextFormat {
                        font_id: bold_font.clone(),
                        color: normal_color,
                        italics: false,
                        ..Default::default()
                    },
                );
                remaining = &remaining[end + 2..];
            } else {
                // No closing ** — just emit the rest as normal
                job.append(
                    &format!("**{remaining}"),
                    0.0,
                    egui::text::TextFormat {
                        font_id: normal_font.clone(),
                        color: normal_color,
                        ..Default::default()
                    },
                );
                remaining = "";
            }
        }
        // Remaining plain text
        if !remaining.is_empty() {
            job.append(
                remaining,
                0.0,
                egui::text::TextFormat {
                    font_id: normal_font,
                    color: normal_color,
                    ..Default::default()
                },
            );
        }
        ui.label(job);
    }
}

// ---------------------------------------------------------------------------
// Friends system (inline - all code in one place as requested)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum FriendsAction {
    FetchFriends {
        user_id: u64,
    },
    FetchRequests {
        user_id: u64,
    },
    SendRequest {
        user_id: u64,
        target_username: String,
    },
    AcceptRequest {
        user_id: u64,
        requester_id: u64,
    },
    DeclineRequest {
        user_id: u64,
        requester_id: u64,
    },
    Refresh {
        user_id: u64,
    },
}

#[derive(Default)]
struct FriendsState {
    viewing_user_id: Option<u64>,
    friends: Vec<ram_core::models::Friend>,
    incoming_requests: Vec<ram_core::models::FriendRequest>,
    loading: bool,
    error: Option<String>,
    add_friend_input: String,
    show_requests: bool,
    search_filter: String,
}

fn show_friends_panel(
    ui: &mut egui::Ui,
    state: &mut FriendsState,
    selected_user_id: Option<u64>,
    avatar_bytes: &HashMap<u64, Vec<u8>>,
) -> Option<FriendsAction> {
    let mut action: Option<FriendsAction> = None;

    if selected_user_id != state.viewing_user_id {
        state.viewing_user_id = selected_user_id;
        state.friends.clear();
        state.incoming_requests.clear();
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

    if state.show_requests && !state.incoming_requests.is_empty() {
        egui::CollapsingHeader::new("📩 Incoming Requests")
            .default_open(true)
            .show(ui, |ui| {
                for req in &state.incoming_requests {
                    ui.horizontal(|ui| {
                        if let Some(bytes) = avatar_bytes.get(&req.user_id) {
                            if let Ok(texture) = load_friend_texture(ui.ctx(), &req.user_id, bytes)
                            {
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

    ui.horizontal(|ui| {
        ui.label("Add friend:");
        let response = ui.text_edit_singleline(&mut state.add_friend_input);
        if ui.button("Send Request").clicked()
            || response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
        {
            if let Some(uid) = state.viewing_user_id {
                if !state.add_friend_input.is_empty() {
                    action = Some(FriendsAction::SendRequest {
                        user_id: uid,
                        target_username: state.add_friend_input.clone(),
                    });
                    state.add_friend_input.clear();
                }
            }
        }
    });

    ui.separator();

    ui.horizontal(|ui| {
        ui.label("Search:");
        ui.text_edit_singleline(&mut state.search_filter);
    });

    ui.separator();

    let filtered_friends: Vec<&ram_core::models::Friend> = if state.search_filter.is_empty() {
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
                ui.horizontal(|ui| {
                    let status_color = if friend.is_online {
                        egui::Color32::GREEN
                    } else {
                        egui::Color32::GRAY
                    };
                    ui.colored_label(status_color, "●");

                    if let Some(bytes) = avatar_bytes.get(&friend.user_id) {
                        if let Ok(texture) = load_friend_texture(ui.ctx(), &friend.user_id, bytes) {
                            ui.add(egui::Image::new(&texture).max_height(32.0));
                        }
                    }

                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(&friend.username).strong());
                        if !friend.display_name.is_empty() {
                            ui.label(egui::RichText::new(&friend.display_name).small());
                        }
                        let status = friend.presence.status_text();
                        if status != "Offline" {
                            ui.label(
                                egui::RichText::new(status)
                                    .small()
                                    .color(egui::Color32::from_gray(150)),
                            );
                        }
                    });
                });
                ui.separator();
            }
        }
    });

    action
}

fn load_friend_texture(
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

impl AppState {
    fn show_bulk_import_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_bulk_import_dialog {
            return;
        }

        // Show proxy test progress if available
        if let Some((tested, total, working, proxy)) = &self.bulk_import_proxy_test {
            let mut keep_open = self.show_bulk_import_dialog;
            egui::Window::new("Testing Proxies...")
                .open(&mut keep_open)
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.heading("Testing Proxies");
                    ui.add_space(8.0);
                    ui.label(format!("Testing proxy {}/{}", tested, total));
                    ui.add_space(8.0);
                    let progress = *tested as f32 / *total as f32;
                    ui.add(egui::widgets::ProgressBar::new(progress).desired_width(300.0));
                    ui.add_space(8.0);
                    ui.label(format!("Working: {}", working));
                    ui.add_space(4.0);
                    // Show proxy without full URL for privacy
                    let proxy_short = if proxy.len() > 30 {
                        format!("...{}", &proxy[proxy.len() - 30..])
                    } else {
                        proxy.clone()
                    };
                    ui.monospace(proxy_short);
                });

            if !keep_open {
                self.show_bulk_import_dialog = false;
                self.bulk_import_proxy_test = None;
            }
            return;
        }

        if let Some((current, total, username, proxy, stage)) = &self.bulk_import_progress {
            let mut keep_open = self.show_bulk_import_dialog;
            egui::Window::new("Bulk Import - Processing")
                .open(&mut keep_open)
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.heading("Cookie Validation");
                    ui.add_space(8.0);
                    ui.label(stage);
                    ui.add_space(8.0);
                    let progress = if *total > 0 {
                        *current as f32 / *total as f32
                    } else {
                        0.0
                    };
                    ui.add(egui::widgets::ProgressBar::new(progress).desired_width(300.0));
                    ui.add_space(4.0);
                    ui.label(format!("{}/{}", current, total));
                    ui.add_space(8.0);
                    if !username.is_empty() {
                        ui.label(format!("Validating: {}", username));
                    }
                    if let Some(p) = proxy {
                        ui.add_space(4.0);
                        let proxy_short = if p.len() > 40 {
                            format!("Proxy: ...{}", &p[p.len() - 40..])
                        } else {
                            format!("Proxy: {}", p)
                        };
                        ui.colored_label(egui::Color32::from_rgb(80, 180, 80), proxy_short);
                    }
                });

            if !keep_open {
                self.show_bulk_import_dialog = false;
                self.bulk_import_progress = None;
            }
        } else if let Some((added, failed, proxy_stats)) = &self.bulk_import_result.clone() {
            let added_count = added.len();
            let failed_count = failed.len();
            let added_list = added.clone();
            let failed_list = failed.clone();
            let mut keep_open = true;

            egui::Window::new("Bulk Import Complete")
                .resizable(true)
                .default_width(400.0)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.add_space(8.0);
                    ui.heading("Import Results");
                    ui.add_space(8.0);

                    ui.label(format!("Successfully added: {}", added_count));
                    ui.label(format!("Failed: {}", failed_count));
                    ui.add_space(8.0);

                    if !added_list.is_empty() {
                        ui.separator();
                        ui.strong("Added Accounts:");
                        egui::ScrollArea::vertical()
                            .max_height(150.0)
                            .show(ui, |ui| {
                                for (_, username, display_name) in added_list.iter() {
                                    ui.label(format!("* {} (@{})", display_name, username));
                                }
                            });
                    }

                    if !failed_list.is_empty() {
                        ui.add_space(8.0);
                        ui.separator();
                        ui.strong("Failed Cookies:");
                        egui::ScrollArea::vertical()
                            .max_height(100.0)
                            .show(ui, |ui| {
                                for (_, error) in failed_list.iter().take(10) {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(200, 60, 60),
                                        format!("* {}", error),
                                    );
                                }
                                if failed_list.len() > 10 {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(180, 180, 180),
                                        format!("... and {} more", failed_list.len() - 10),
                                    );
                                }
                            });
                    }

                    ui.add_space(16.0);
                    if ui.button("Close").clicked() {
                        keep_open = false;
                    }
                });

            if !keep_open {
                self.show_bulk_import_dialog = false;
                self.bulk_import_result = None;
            }
        }
    }
}
