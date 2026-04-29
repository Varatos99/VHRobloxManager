//! Bridge between the synchronous `egui` update loop and the `tokio` async runtime.
//!
//! All heavyweight operations (network, file I/O, process spawning) are dispatched
//! as [`BackendCommand`] messages to a background `tokio` runtime. Results come
//! back as [`BackendEvent`] through an mpsc channel polled each frame.

use eframe::egui;
use ram_core::auth::RobloxClient;
use ram_core::cookie_cache::CookieInfo;
use ram_core::models::{Account, AccountStore, Presence};
use ram_core::{api, cookie_cache, crypto, process, proxy, proxy_cache, CoreError};
use ram_core::process as proc;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinSet;
use tracing::{error, info, warn};

// ---------------------------------------------------------------------------
// Commands (UI → Backend)
// ---------------------------------------------------------------------------

#[allow(dead_code)]
pub enum BackendCommand {
    /// Validate a cookie and add the account.
    AddAccount {
        cookie: String,
        password: String,
        use_credential_manager: bool,
    },
    /// Remove an account by user ID.
    RemoveAccount { user_id: u64 },
    /// Refresh avatar URLs for all accounts.
    RefreshAvatars { user_ids: Vec<u64>, cookie: String },
    /// Refresh presence for all accounts.
    RefreshPresence { user_ids: Vec<u64>, cookie: String },
    /// Launch the game for an account.
    LaunchGame {
        cookie: String,
        place_id: u64,
        job_id: Option<String>,
        link_code: Option<String>,
        access_code: Option<String>,
        multi_instance: bool,
        kill_background: bool,
        privacy_mode: bool,
    },
    /// Launch the game, decrypting the cookie on the backend side.
    LaunchGameEncrypted {
        user_id: u64,
        encrypted_cookie: Option<String>,
        password: String,
        use_credential_manager: bool,
        place_id: u64,
        job_id: Option<String>,
        link_code: Option<String>,
        access_code: Option<String>,
        multi_instance: bool,
        kill_background: bool,
        privacy_mode: bool,
    },
    /// Save the account store to disk.
    SaveStore {
        store: AccountStore,
        path: PathBuf,
        password: String,
    },
    /// Load the account store from disk.
    LoadStore { path: PathBuf, password: String },
    /// Kill all Roblox instances.
    KillAll,
    /// Refresh avatars + presence for all accounts, decrypting a cookie on this side.
    RefreshAll {
        user_ids: Vec<u64>,
        /// The first account's encrypted cookie (or None if credential manager).
        first_user_id: u64,
        encrypted_cookie: Option<String>,
        password: String,
        use_credential_manager: bool,
    },
    /// Lightweight presence-only refresh for a subset of visible accounts.
    RefreshPresenceOnly {
        user_ids: Vec<u64>,
        first_user_id: u64,
        encrypted_cookie: Option<String>,
        password: String,
        use_credential_manager: bool,
    },
    /// Launch Roblox without a specific game (just open the player).
    LaunchRoblox {
        user_id: u64,
        encrypted_cookie: Option<String>,
        password: String,
        use_credential_manager: bool,
    },
    /// Launch Roblox Studio with the selected account.
    LaunchStudio {
        user_id: u64,
        encrypted_cookie: Option<String>,
        password: String,
        use_credential_manager: bool,
    },
    /// Launch multiple accounts into the same game sequentially.
    BulkLaunchEncrypted {
        /// (user_id, encrypted_cookie) pairs for each account.
        accounts: Vec<(u64, Option<String>)>,
        password: String,
        use_credential_manager: bool,
        place_id: u64,
        job_id: Option<String>,
        link_code: Option<String>,
        access_code: Option<String>,
        multi_instance: bool,
        kill_background: bool,
        privacy_mode: bool,
    },
    /// Re-validate all accounts' cookies automatically.
    RevalidateAll {
        /// (user_id, encrypted_cookie) pairs for each account.
        accounts: Vec<(u64, Option<String>)>,
        password: String,
        use_credential_manager: bool,
    },
    /// Arrange all Roblox windows in a tiled grid.
    ArrangeWindows,
    /// Check GitLab for a newer release.
    CheckForUpdates { current_version: String },
    /// Resolve a place ID to its name (for private server auto-check).
    ResolvePlace {
        place_id: u64,
        universe_id: Option<u64>,
        /// Index into the private_servers list so the UI can update the right entry.
        index: usize,
    },
    /// Resolve a share link code into (place_id, link_code) via the Roblox API.
    ResolveShareLink {
        share_code: String,
        server_name: String,
        /// The encrypted cookie + auth info needed for the authenticated API call.
        first_user_id: u64,
        encrypted_cookie: Option<String>,
        password: String,
        use_credential_manager: bool,
    },
    /// Fetch friends list for a user.
    FetchFriends {
        user_id: u64,
        encrypted_cookie: Option<String>,
        password: String,
        use_credential_manager: bool,
    },
    /// Fetch incoming friend requests.
    FetchFriendRequests {
        user_id: u64,
        encrypted_cookie: Option<String>,
        password: String,
        use_credential_manager: bool,
    },
    /// Search for users by username.
    SearchUsers { query: String },
    /// Send a friend request to a target user.
    SendFriendRequest {
        user_id: u64,
        target_user_id: u64,
        encrypted_cookie: Option<String>,
        password: String,
        use_credential_manager: bool,
    },
    /// Accept an incoming friend request.
    AcceptFriendRequest {
        user_id: u64,
        requester_id: u64,
        encrypted_cookie: Option<String>,
        password: String,
        use_credential_manager: bool,
    },
    /// Decline an incoming friend request.
    DeclineFriendRequest {
        user_id: u64,
        requester_id: u64,
        encrypted_cookie: Option<String>,
        password: String,
        use_credential_manager: bool,
    },
    /// Resolve a friend's game's place name.
    ResolveFriendPlace {
        user_id: u64,
        friend_user_id: u64,
        place_id: u64,
        encrypted_cookie: Option<String>,
        password: String,
        use_credential_manager: bool,
    },
    /// Search games by query (needs auth).
    SearchGames {
        user_id: u64,
        query: String,
        index: usize,
        encrypted_cookie: Option<String>,
        password: String,
        use_credential_manager: bool,
    },
    /// Get popular/trending games (needs auth).
    GetPopularGames {
        user_id: u64,
        index: usize,
        encrypted_cookie: Option<String>,
        password: String,
        use_credential_manager: bool,
    },
    /// Get user's favorite games (needs auth).
    GetFavoriteGames {
        user_id: u64,
        index: usize,
        encrypted_cookie: Option<String>,
        password: String,
        use_credential_manager: bool,
    },
    /// Create a VIP server for a game.
    CreateVipServer {
        universe_id: u64,
        place_id: u64,
        name: String,
        index: usize,
        encrypted_cookie: Option<String>,
        password: String,
        use_credential_manager: bool,
    },
    /// List private servers for a place.
    ListPrivateServers {
        place_id: u64,
        index: usize,
        encrypted_cookie: Option<String>,
        password: String,
        use_credential_manager: bool,
    },
    /// Check if VIP servers are allowed and get the price for a universe (requires auth).
    CheckVipServerPrice {
        universe_id: u64,
        place_id: u64,
        encrypted_cookie: Option<String>,
        password: String,
        use_credential_manager: bool,
},
    /// Bulk import cookies from a text file (one per line).
    BulkImportCookies {
        cookies: Vec<String>,
        password: String,
        use_credential_manager: bool,
        proxy_file: Option<String>,
        /// Use cached validated proxies instead of testing new ones
        use_cached_proxies: bool,
        /// Force re-test all proxies even if cached
        force_recheck_proxies: bool,
    },
        /// Fetch Robux balance for an account.
        FetchCurrency {
            user_id: u64,
            encrypted_cookie: Option<String>,
            password: String,
            use_credential_manager: bool,
        },
        /// Fetch user's groups (with roles).
        FetchUserGroups {
            user_id: u64,
            encrypted_cookie: Option<String>,
            password: String,
            use_credential_manager: bool,
        },
        /// Fetch group Robux balance.
        FetchGroupCurrency {
            user_id: u64,
            group_id: u64,
            group_name: String,
            encrypted_cookie: Option<String>,
            password: String,
            use_credential_manager: bool,
        },
    }

// ---------------------------------------------------------------------------
// Events (Backend → UI)
// ---------------------------------------------------------------------------

#[allow(dead_code)]
pub enum BackendEvent {
    /// An account was validated and is ready to be added.
    AccountValidated {
        account: Box<Account>,
        encrypted_cookie: Option<String>,
    },
    /// Account removed.
    AccountRemoved { user_id: u64 },
    /// Avatar URLs fetched.
    AvatarsUpdated(Vec<(u64, String)>),
    /// Raw avatar image bytes downloaded.
    AvatarImagesReady(Vec<(u64, Vec<u8>)>),
    /// Presences fetched.
    PresencesUpdated(Vec<(u64, Presence)>),
    /// Game launched successfully.
    GameLaunched,
    /// Store saved.
    StoreSaved,
    /// Store loaded from disk.
    StoreLoaded(AccountStore),
    /// All Roblox instances killed (count).
    Killed(usize),
    /// Progress update during a bulk launch (launched_so_far, total).
    BulkLaunchProgress { launched: usize, total: usize },
    /// Bulk launch completed.
    BulkLaunchComplete { launched: usize, failed: usize },
    /// Account cookie revalidation result.
    AccountRevalidated {
        user_id: u64,
        valid: bool,
        username: String,
        display_name: String,
    },
    /// An error occurred during a background operation.
    Error(String),
    /// Windows were arranged.
    WindowsArranged,
    /// A newer version is available on GitLab.
    UpdateAvailable { version: String, url: String },
    /// Place name resolved for a private server entry.
    PlaceResolved {
        index: usize,
        place_name: String,
        place_id: u64,
        icon_bytes: Option<Vec<u8>>,
    },
    /// Share link resolved — contains the actual place_id, link_code, access_code, and server name.
    ShareLinkResolved {
        server_name: String,
        place_id: u64,
        universe_id: Option<u64>,
        link_code: String,
        access_code: String,
    },
    /// Share link resolution failed.
    ShareLinkFailed(String),
    /// Friends list loaded.
    FriendsLoaded {
        user_id: u64,
        friends: Vec<ram_core::models::Friend>,
    },
    /// Incoming friend requests loaded.
    FriendRequestsLoaded {
        user_id: u64,
        requests: Vec<ram_core::models::FriendRequest>,
    },
    /// User search results.
    UserSearchResults {
        results: Vec<(u64, String, String)>,
    },
    /// Friend request sent successfully.
    FriendRequestSent {
        target_user_id: u64,
    },
    /// Friend request accepted.
    FriendRequestAccepted {
        requester_id: u64,
    },
    /// Friend request declined.
    FriendRequestDeclined {
        requester_id: u64,
    },
    /// Friend's game info resolved.
    FriendGameResolved {
        friend_user_id: u64,
        game_name: String,
    },
    /// Successfully joined a friend's game.
    FriendGameJoined {
        friend_user_id: u64,
        place_id: u64,
    },
    /// Search results loaded.
    GamesSearchResults {
        index: usize,
        games: Vec<ram_core::models::GameSearchResult>,
    },
    /// Popular games loaded.
    GamesPopularLoaded {
        index: usize,
        games: Vec<ram_core::models::GameSearchResult>,
    },
    /// Favorite games loaded.
    FavoriteGamesLoaded {
        index: usize,
        games: Vec<ram_core::models::GameSearchResult>,
    },
    /// VIP server created successfully.
    VipServerCreated {
        index: usize,
        vip_server_id: u64,
        access_code: String,
        name: String,
        place_id: u64,
    },
    /// Private servers list loaded.
    PrivateServersLoaded {
        index: usize,
        servers: Vec<ram_core::models::PrivateServerInfo>,
    },
    /// VIP server price check result.
    VipServerPriceChecked {
        universe_id: u64,
        allowed: bool,
        price: u64,
    },
    /// VIP server price could not be determined.
    VipServerPriceUnknown {
        universe_id: u64,
    },
    /// Bulk import progress update.
    BulkImportProgress {
        current: usize,
        total: usize,
        username: String,
        proxy: Option<String>,
        stage: String,
    },
/// Bulk import completed — list of added accounts and failed cookies.
    BulkImportComplete {
        added: Vec<(u64, String, String)>,
        failed: Vec<(String, String)>,
        proxy_stats: Option<ProxyStats>,
    },
    /// Proxy test progress update.
    ProxyTestProgress {
        tested: usize,
        total: usize,
        working: usize,
        proxy: String,
    },
    /// Proxy test completed.
    ProxyTestComplete {
        working: Vec<String>,
        failed: Vec<String>,
    },
    /// Robux balance fetched.
    CurrencyUpdated {
        user_id: u64,
        robux: u64,
        is_premium: bool,
    },
    /// User's groups fetched.
    UserGroupsLoaded {
        user_id: u64,
        groups: Vec<ram_core::api::GroupRole>,
    },
    /// Group Robux fetched.
    GroupCurrencyLoaded {
        group_id: u64,
        group_name: String,
        robux: u64,
        pending: u64,
    },
}

#[derive(Clone)]
pub struct ProxyStats {
    pub total_cookies: usize,
    pub total_proxies: usize,
    pub working_proxies: usize,
}

// ---------------------------------------------------------------------------
// Runtime
// ---------------------------------------------------------------------------

pub struct BackendBridge {
    pub cmd_tx: mpsc::UnboundedSender<BackendCommand>,
    pub evt_rx: mpsc::UnboundedReceiver<BackendEvent>,
    repaint_ctx: Option<egui::Context>,
    proxy_cache: Arc<Mutex<Option<proxy_cache::ProxyCache>>>,
    cookie_cache: Arc<cookie_cache::CookieCache>,
}

impl BackendBridge {
    /// Spawn the `tokio` runtime on a dedicated thread and return the bridge.
    pub fn spawn() -> Self {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<BackendCommand>();
        let (evt_tx, evt_rx) = mpsc::unbounded_channel::<BackendEvent>();
        let proxy_cache = Arc::new(Mutex::new(None));
        let proxy_cache_for_loop = proxy_cache.clone();
        
        // Initialize cookie cache
        let cookie_cache = match cookie_cache::CookieCache::new() {
            Ok(cc) => Arc::new(cc),
            Err(e) => {
                warn!("Failed to initialize cookie cache: {}", e);
                Arc::new(cookie_cache::CookieCache::new().unwrap())
            }
        };
        let cookie_cache_for_loop = cookie_cache.clone();

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("failed to build tokio runtime");
            rt.block_on(backend_loop(cmd_rx, evt_tx, proxy_cache_for_loop, cookie_cache_for_loop));
        });

        Self { cmd_tx, evt_rx, repaint_ctx: None, proxy_cache, cookie_cache }
    }

    /// Give the bridge an egui context so it can request repaints when events arrive.
    pub fn set_repaint_ctx(&mut self, ctx: egui::Context) {
        if self.repaint_ctx.is_none() {
            self.repaint_ctx = Some(ctx);
        }
    }

    /// Send a command to the backend (non-blocking).
    pub fn send(&self, cmd: BackendCommand) {
        let _ = self.cmd_tx.send(cmd);
    }

    /// Drain all pending events. Call once per frame.
    pub fn poll(&mut self) -> Vec<BackendEvent> {
        let mut events = Vec::new();
        while let Ok(evt) = self.evt_rx.try_recv() {
            events.push(evt);
        }
        if !events.is_empty() {
            if let Some(ctx) = &self.repaint_ctx {
                ctx.request_repaint();
            }
        }
        events
    }
}

// ---------------------------------------------------------------------------
// Async event loop
// ---------------------------------------------------------------------------

async fn backend_loop(
    mut rx: mpsc::UnboundedReceiver<BackendCommand>,
    tx: mpsc::UnboundedSender<BackendEvent>,
    proxy_cache: Arc<Mutex<Option<proxy_cache::ProxyCache>>>,
    cookie_cache: Arc<cookie_cache::CookieCache>,
) {
    let client = RobloxClient::default();
    
    // Initialize proxy cache
    match proxy_cache::ProxyCache::new().await {
        Ok(cache) => {
            info!("Proxy cache initialized");
            let mut proxy_cache_lock = proxy_cache.lock().await;
            *proxy_cache_lock = Some(cache);
        }
        Err(e) => {
            warn!("Failed to initialize proxy cache: {}", e);
        }
    }

    while let Some(cmd) = rx.recv().await {
        let client = client.clone();
        let tx = tx.clone();
        let proxy_cache = proxy_cache.clone();
        let cookie_cache = cookie_cache.clone();

        // Each command runs as its own spawned task for concurrency.
        tokio::spawn(async move {
            match handle_command(cmd, &client, &tx, &proxy_cache, &cookie_cache).await {
                Ok(evt) => {
                    let _ = tx.send(evt);
                }
                Err(e) => {
                    error!("Backend error: {e}");
                    let _ = tx.send(BackendEvent::Error(e.to_string()));
                }
            }
        });
    }
}

async fn handle_command(
    cmd: BackendCommand,
    client: &RobloxClient,
    tx: &mpsc::UnboundedSender<BackendEvent>,
    proxy_cache: &Arc<Mutex<Option<proxy_cache::ProxyCache>>>,
    cookie_cache: &Arc<cookie_cache::CookieCache>,
) -> Result<BackendEvent, CoreError> {
    match cmd {
        BackendCommand::AddAccount {
            cookie,
            password,
            use_credential_manager,
        } => {
            let (user_id, username, display_name) = client.validate_cookie(&cookie).await?;
            let mut account = Account::new(user_id, username, display_name);

            let encrypted = if use_credential_manager {
                crypto::credential_store(user_id, &cookie)?;
                None
            } else {
                Some(crypto::encrypt_cookie(&cookie, &password)?)
            };
            account.encrypted_cookie = encrypted.clone();
            account.last_validated = Some(chrono::Utc::now());

            // Fetch avatar URL and image bytes immediately after validation
            if let Ok(avatars) = api::fetch_avatars(client, &cookie, &[user_id]).await {
                if let Some((_, url)) = avatars.first() {
                    account.avatar_url = url.clone();
                }
                let images = api::download_avatar_images(client, &cookie, &avatars).await;
                if !images.is_empty() {
                    let _ = tx.send(BackendEvent::AvatarImagesReady(images));
                }
            }

            info!("Validated account {} ({})", account.username, user_id);
            Ok(BackendEvent::AccountValidated {
                account: Box::new(account),
                encrypted_cookie: encrypted,
            })
        }
        BackendCommand::RemoveAccount { user_id } => {
            // Best-effort delete from credential manager
            let _ = crypto::credential_delete(user_id);
            Ok(BackendEvent::AccountRemoved { user_id })
        }
        BackendCommand::RefreshAvatars { user_ids, cookie } => {
            let avatars = api::fetch_avatars(client, &cookie, &user_ids).await?;
            Ok(BackendEvent::AvatarsUpdated(avatars))
        }
        BackendCommand::RefreshPresence { user_ids, cookie } => {
            let presences = api::fetch_presences(client, &cookie, &user_ids).await?;
            Ok(BackendEvent::PresencesUpdated(presences))
        }
        BackendCommand::LaunchGameEncrypted {
            user_id,
            encrypted_cookie,
            password,
            use_credential_manager,
            place_id,
            job_id,
            link_code,
            access_code,
            multi_instance,
            kill_background,
            privacy_mode,
        } => {
            let cookie = if use_credential_manager {
                crypto::credential_load(user_id)?
            } else {
                let enc = encrypted_cookie.ok_or_else(|| {
                    CoreError::Crypto("no encrypted cookie stored for this account".into())
                })?;
                crypto::decrypt_cookie(&enc, &password)?
            };
            if multi_instance {
                process::enable_multi_instance()?;
            }
            if kill_background || multi_instance {
                process::kill_tray_roblox();
            }
            if privacy_mode {
                process::clear_roblox_cookies();
            }
            let ticket = client.generate_auth_ticket(&cookie).await?;
            process::launch_game(&ticket, place_id, job_id.as_deref(), link_code.as_deref(), access_code.as_deref())?;
            Ok(BackendEvent::GameLaunched)
        }
        BackendCommand::LaunchGame {
            cookie,
            place_id,
            job_id,
            link_code,
            access_code,
            multi_instance,
            kill_background,
            privacy_mode,
        } => {
            if multi_instance {
                process::enable_multi_instance()?;
            }
            if kill_background || multi_instance {
                process::kill_tray_roblox();
            }
            if privacy_mode {
                process::clear_roblox_cookies();
            }
            let ticket = client.generate_auth_ticket(&cookie).await?;
            process::launch_game(&ticket, place_id, job_id.as_deref(), link_code.as_deref(), access_code.as_deref())?;
            Ok(BackendEvent::GameLaunched)
        }
        BackendCommand::LaunchRoblox {
            user_id,
            encrypted_cookie,
            password,
            use_credential_manager,
        } => {
            // Get the cookie (decrypt if needed)
            let cookie = if use_credential_manager {
                ram_core::crypto::credential_load(user_id)?
            } else {
                let enc = encrypted_cookie.ok_or_else(|| {
                    CoreError::Crypto("no encrypted cookie for LaunchRoblox".into())
                })?;
                ram_core::crypto::decrypt_cookie(&enc, &password)?
            };

            // Generate auth ticket and launch Roblox without a specific game
            let ticket = client.generate_auth_ticket(&cookie).await?;
            // Launch with place_id = 0 (just open Roblox player)
            process::launch_game(&ticket, 0, None, None, None)?;
            Ok(BackendEvent::GameLaunched)
        }
        BackendCommand::LaunchStudio {
            user_id,
            encrypted_cookie,
            password,
            use_credential_manager,
        } => {
            // Get the cookie (decrypt if needed)
            let cookie = if use_credential_manager {
                ram_core::crypto::credential_load(user_id)?
            } else {
                let enc = encrypted_cookie.ok_or_else(|| {
                    CoreError::Crypto("no encrypted cookie for LaunchStudio".into())
                })?;
                ram_core::crypto::decrypt_cookie(&enc, &password)?
            };

            // Generate auth ticket and launch Roblox Studio
            let ticket = client.generate_auth_ticket(&cookie).await?;
            process::launch_studio(&ticket)?;
            Ok(BackendEvent::GameLaunched)
        }
        BackendCommand::SaveStore {
            store,
            path,
            password,
        } => {
            crypto::save_encrypted(&path, &store, &password)?;
            Ok(BackendEvent::StoreSaved)
        }
        BackendCommand::LoadStore { path, password } => {
            let store = crypto::load_encrypted(&path, &password)?;
            Ok(BackendEvent::StoreLoaded(store))
        }
        BackendCommand::KillAll => {
            let count = process::kill_all_roblox()?;
            Ok(BackendEvent::Killed(count))
        }
        BackendCommand::RefreshAll {
            user_ids,
            first_user_id,
            encrypted_cookie,
            password,
            use_credential_manager,
        } => {
            let cookie = if use_credential_manager {
                crypto::credential_load(first_user_id)?
            } else {
                let enc = encrypted_cookie.ok_or_else(|| {
                    CoreError::Crypto("no encrypted cookie for refresh".into())
                })?;
                crypto::decrypt_cookie(&enc, &password)?
            };
            let avatars = api::fetch_avatars(client, &cookie, &user_ids).await?;
            let _ = tx.send(BackendEvent::AvatarsUpdated(avatars.clone()));
            // Download actual image bytes (skips failures)
            let images = api::download_avatar_images(client, &cookie, &avatars).await;
            if !images.is_empty() {
                let _ = tx.send(BackendEvent::AvatarImagesReady(images));
            }
            let presences = api::fetch_presences(client, &cookie, &user_ids).await?;
            Ok(BackendEvent::PresencesUpdated(presences))
        }
        BackendCommand::RefreshPresenceOnly {
            user_ids,
            first_user_id,
            encrypted_cookie,
            password,
            use_credential_manager,
        } => {
            let cookie = if use_credential_manager {
                crypto::credential_load(first_user_id)?
            } else {
                let enc = encrypted_cookie.ok_or_else(|| {
                    CoreError::Crypto("no encrypted cookie for refresh".into())
                })?;
                crypto::decrypt_cookie(&enc, &password)?
            };
            let presences = api::fetch_presences(client, &cookie, &user_ids).await?;
            Ok(BackendEvent::PresencesUpdated(presences))
        }
        BackendCommand::BulkLaunchEncrypted {
            accounts,
            password,
            use_credential_manager,
            place_id,
            job_id,
            link_code,
            access_code,
            multi_instance,
            kill_background,
            privacy_mode,
        } => {
            if multi_instance {
                process::enable_multi_instance()?;
            }
            if kill_background || multi_instance {
                process::kill_tray_roblox();
            }
            if privacy_mode {
                process::clear_roblox_cookies();
            }

            // If no Job ID was provided and no link_code (private server), resolve
            // a public server so all accounts land in the same server.
            let resolved_job_id = if job_id.is_some() || link_code.is_some() {
                job_id
            } else {
                // Decrypt the first account's cookie to make the API call
                let first = accounts.first().ok_or_else(|| {
                    CoreError::Process("no accounts to launch".into())
                })?;
                let first_cookie = if use_credential_manager {
                    crypto::credential_load(first.0)?
                } else {
                    match &first.1 {
                        Some(enc) => crypto::decrypt_cookie(enc, &password)?,
                        None => {
                            return Err(CoreError::Crypto(
                                "no encrypted cookie for first account".into(),
                            ))
                        }
                    }
                };
                match api::fetch_servers(client, &first_cookie, place_id, None).await {
                    Ok((servers, _)) => {
                        if let Some(server) = servers.into_iter().next() {
                            info!("Bulk launch: resolved server {} ({}/{} players)",
                                  server.id, server.playing, server.max_players);
                            Some(server.id)
                        } else {
                            info!("Bulk launch: no public servers found, launching without Job ID");
                            None
                        }
                    }
                    Err(e) => {
                        info!("Bulk launch: server fetch failed ({e}), launching without Job ID");
                        None
                    }
                }
            };

            let total = accounts.len();
            let mut launched = 0usize;
            let mut failed = 0usize;
            for (i, (user_id, encrypted_cookie)) in accounts.iter().enumerate() {
                let cookie_result = if use_credential_manager {
                    crypto::credential_load(*user_id)
                } else {
                    match encrypted_cookie {
                        Some(enc) => crypto::decrypt_cookie(enc, &password),
                        None => Err(CoreError::Crypto(
                            "no encrypted cookie stored".into(),
                        )),
                    }
                };
                match cookie_result {
                    Ok(cookie) => {
                        match client.generate_auth_ticket(&cookie).await {
                            Ok(ticket) => {
                                if let Err(e) = process::launch_game(
                                    &ticket,
                                    place_id,
                                    resolved_job_id.as_deref(),
                                    link_code.as_deref(),
                                    access_code.as_deref(),
                                ) {
                                    error!("Bulk launch failed for user {user_id}: {e}");
                                    failed += 1;
                                } else {
                                    launched += 1;
                                }
                            }
                            Err(e) => {
                                error!("Auth ticket failed for user {user_id}: {e}");
                                failed += 1;
                            }
                        }
                    }
                    Err(e) => {
                        error!("Cookie decrypt failed for user {user_id}: {e}");
                        failed += 1;
                    }
                }
                let _ = tx.send(BackendEvent::BulkLaunchProgress {
                    launched: i + 1,
                    total,
                });
                // Kill tray processes that spawn between launches
                if (kill_background || multi_instance) && i + 1 < total {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    process::kill_tray_roblox();
                }
            }
            Ok(BackendEvent::BulkLaunchComplete { launched, failed })
        }
        BackendCommand::RevalidateAll {
            accounts,
            password,
            use_credential_manager,
        } => {
            for (user_id, encrypted_cookie) in &accounts {
                let cookie_result = if use_credential_manager {
                    crypto::credential_load(*user_id)
                } else {
                    match encrypted_cookie {
                        Some(enc) => crypto::decrypt_cookie(enc, &password),
                        None => continue,
                    }
                };
                let cookie = match cookie_result {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                match client.validate_cookie(&cookie).await {
                    Ok((_, username, display_name)) => {
                        let _ = tx.send(BackendEvent::AccountRevalidated {
                            user_id: *user_id,
                            valid: true,
                            username,
                            display_name,
                        });
                    }
                    Err(_) => {
                        info!("Cookie expired for user {user_id}");
                        let _ = tx.send(BackendEvent::AccountRevalidated {
                            user_id: *user_id,
                            valid: false,
                            username: String::new(),
                            display_name: String::new(),
                        });
                    }
                }
            }
            Ok(BackendEvent::StoreSaved)
        }
        BackendCommand::ArrangeWindows => {
            // Small delay to let Roblox windows finish appearing
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            process::safe_arrange_roblox_windows();
            Ok(BackendEvent::WindowsArranged)
        }
        BackendCommand::CheckForUpdates { current_version } => {
            match api::check_for_updates(&current_version).await {
                Ok(Some((version, url))) => {
                    Ok(BackendEvent::UpdateAvailable { version, url })
                }
                Ok(None) => Ok(BackendEvent::StoreSaved), // no-op event
                Err(e) => {
                    info!("Update check failed (non-fatal): {e}");
                    Ok(BackendEvent::StoreSaved) // silently ignore
                }
            }
        }
        BackendCommand::ResolvePlace { place_id, universe_id, index } => {
            // Both the game name and icon endpoints work without auth when we
            // have a universe_id. If we don't, we can't resolve without auth.
            if let Some(uid) = universe_id {
                let name = api::resolve_universe_name(client, uid).await
                    .unwrap_or_default();
                let icon_bytes = match api::fetch_game_icons(client, "", &[uid]).await {
                    Ok(icons) => {
                        if let Some((_, url)) = icons.into_iter().next() {
                            client.get_bytes(&url, "").await.ok()
                        } else {
                            None
                        }
                    }
                    Err(e) => {
                        info!("Game icon fetch failed for universe {uid}: {e}");
                        None
                    }
                };
                Ok(BackendEvent::PlaceResolved { index, place_name: name, place_id, icon_bytes })
            } else {
                // No universe_id — cannot resolve without auth. Return empty.
                Ok(BackendEvent::PlaceResolved { index, place_name: String::new(), place_id, icon_bytes: None })
            }
        }
        BackendCommand::ResolveShareLink {
            share_code,
            server_name,
            first_user_id,
            encrypted_cookie,
            password,
            use_credential_manager,
        } => {
            let cookie = if use_credential_manager {
                crypto::credential_load(first_user_id)?
            } else {
                let enc = encrypted_cookie.ok_or_else(|| {
                    CoreError::Crypto("no encrypted cookie for share link resolution".into())
                })?;
                crypto::decrypt_cookie(&enc, &password)?
            };
            match api::resolve_share_link(client, &cookie, &share_code).await {
                Ok((place_id, universe_id, link_code, access_code)) => {
                    Ok(BackendEvent::ShareLinkResolved {
                        server_name,
                        place_id,
                        universe_id,
                        link_code,
                        access_code,
                    })
                }
                Err(e) => {
                    info!("ResolveShareLink failed: {e}");
                    Ok(BackendEvent::ShareLinkFailed(e.to_string()))
                }
            }
        }
        BackendCommand::FetchFriends {
            user_id,
            encrypted_cookie,
            password,
            use_credential_manager,
        } => {
            let cookie = if use_credential_manager {
                crypto::credential_load(user_id)?
            } else {
                let enc = encrypted_cookie.ok_or_else(|| {
                    CoreError::Crypto("no encrypted cookie for fetch friends".into())
                })?;
                crypto::decrypt_cookie(&enc, &password)?
            };
            match api::fetch_friends(client, &cookie, user_id).await {
                Ok(friends_data) => {
                    let mut friends: Vec<ram_core::models::Friend> = friends_data
                        .into_iter()
                        .map(|(id, username, display_name)| ram_core::models::Friend {
                            user_id: id,
                            username,
                            display_name,
                            is_online: false,
                            presence: ram_core::models::Presence::default(),
                            avatar_bytes: None,
                        })
                        .collect();

                    // Fetch presences for friends
                    let friend_ids: Vec<u64> = friends.iter().map(|f| f.user_id).collect();
                    if !friend_ids.is_empty() {
                        if let Ok(presences) = api::fetch_presences(client, &cookie, &friend_ids).await {
                            for (uid, presence) in presences {
                                if let Some(friend) = friends.iter_mut().find(|f| f.user_id == uid) {
                                    friend.presence = presence.clone();
                                    friend.is_online = presence.is_online();
                                }
                            }
                        }
                    }

                    // Fetch avatar URLs and download images
                    if !friend_ids.is_empty() {
                        if let Ok(avatar_urls) = api::fetch_avatars(client, &cookie, &friend_ids).await {
                            let images = api::download_avatar_images(client, &cookie, &avatar_urls).await;
                            if !images.is_empty() {
                                let _ = tx.send(BackendEvent::AvatarImagesReady(images));
                            }
                        }
                    }

                    Ok(BackendEvent::FriendsLoaded { user_id, friends })
                }
                Err(e) => {
                    info!("FetchFriends failed: {e}");
                    Err(e)
                }
            }
        }
        BackendCommand::FetchFriendRequests {
            user_id,
            encrypted_cookie,
            password,
            use_credential_manager,
        } => {
            let cookie = if use_credential_manager {
                crypto::credential_load(user_id)?
            } else {
                let enc = encrypted_cookie.ok_or_else(|| {
                    CoreError::Crypto("no encrypted cookie for fetch requests".into())
                })?;
                crypto::decrypt_cookie(&enc, &password)?
            };
            match api::fetch_incoming_requests(client, &cookie, user_id).await {
                Ok(requests_data) => {
                    let requests: Vec<ram_core::models::FriendRequest> = requests_data
                        .into_iter()
                        .map(|(id, username, display_name)| ram_core::models::FriendRequest {
                            user_id: id,
                            username,
                            display_name,
                            created: String::new(),
                        })
                        .collect();
                    Ok(BackendEvent::FriendRequestsLoaded { user_id, requests })
                }
                Err(e) => {
                    info!("FetchFriendRequests failed: {e}");
                    Err(e)
                }
            }
        }
        BackendCommand::SearchUsers { query } => {
            match api::search_users(client, "", &query).await {
                Ok(results) => Ok(BackendEvent::UserSearchResults { results }),
                Err(e) => {
                    info!("SearchUsers failed: {e}");
                    Err(e)
                }
            }
        }
        // DISABLED: Add friend requires captcha verification via API
        // Will implement later with browser-based solution
        /*
        BackendCommand::SendFriendRequest {
            user_id,
            target_user_id,
            encrypted_cookie,
            password,
            use_credential_manager,
        } => {
            let cookie = if use_credential_manager {
                crypto::credential_load(user_id)?
            } else {
                let enc = encrypted_cookie.ok_or_else(|| {
                    CoreError::Crypto("no encrypted cookie for send request".into())
                })?;
                crypto::decrypt_cookie(&enc, &password)?
            };
            
            // Try API directly - no browser fallback
            match api::send_friend_request(client, &cookie, target_user_id).await {
                Ok(()) => Ok(BackendEvent::FriendRequestSent { target_user_id }),
                Err(e) => {
                    // Just return error - don't open browser
                    error!("SendFriendRequest API failed: {}", e);
                    Err(e)
                }
            }
        }
        */
        // Placeholder: Show message that add friend is disabled
        BackendCommand::SendFriendRequest { .. } => {
            error!("Add friend is currently disabled - requires captcha");
            Err(CoreError::RobloxApi {
                status: 0,
                message: "Add friend is temporarily disabled. Please use Roblox website.".into(),
            })
        }
        BackendCommand::AcceptFriendRequest {
            user_id,
            requester_id,
            encrypted_cookie,
            password,
            use_credential_manager,
        } => {
            let cookie = if use_credential_manager {
                crypto::credential_load(user_id)?
            } else {
                let enc = encrypted_cookie.ok_or_else(|| {
                    CoreError::Crypto("no encrypted cookie for accept request".into())
                })?;
                crypto::decrypt_cookie(&enc, &password)?
            };
            match api::accept_friend_request(client, &cookie, requester_id).await {
                Ok(()) => Ok(BackendEvent::FriendRequestAccepted { requester_id }),
                Err(e) => {
                    info!("AcceptFriendRequest failed: {e}");
                    Err(e)
                }
            }
        }
        BackendCommand::DeclineFriendRequest {
            user_id,
            requester_id,
            encrypted_cookie,
            password,
            use_credential_manager,
        } => {
            let cookie = if use_credential_manager {
                crypto::credential_load(user_id)?
            } else {
                let enc = encrypted_cookie.ok_or_else(|| {
                    CoreError::Crypto("no encrypted cookie for decline request".into())
                })?;
                crypto::decrypt_cookie(&enc, &password)?
            };
            match api::decline_friend_request(client, &cookie, requester_id).await {
                Ok(()) => Ok(BackendEvent::FriendRequestDeclined { requester_id }),
                Err(e) => {
                    info!("DeclineFriendRequest failed: {e}");
                    Err(e)
                }
            }
        }
        BackendCommand::ResolveFriendPlace { user_id, friend_user_id, place_id, encrypted_cookie, password, use_credential_manager } => {
            info!("ResolveFriendPlace: friend={}, place={}", friend_user_id, place_id);
            let cookie = if use_credential_manager {
                crypto::credential_load(user_id)?
            } else {
                let enc = encrypted_cookie.ok_or_else(|| {
                    CoreError::Crypto("no encrypted cookie for resolve place".into())
                })?;
                crypto::decrypt_cookie(&enc, &password)?
            };
            
            let game_name = match api::resolve_place_name(client, &cookie, place_id).await {
                Ok(name) => {
                    info!("Resolved game name: {}", name);
                    name
                }
                Err(e) => {
                    info!("Failed to resolve place {}: {}", place_id, e);
                    format!("Place {}", place_id)
                }
            };
            
            Ok(BackendEvent::FriendGameResolved {
                friend_user_id,
                game_name,
            })
        }
        BackendCommand::SearchGames { user_id, query, index, encrypted_cookie, password, use_credential_manager } => {
            info!("SearchGames: query={}", query);
            let cookie = if use_credential_manager {
                crypto::credential_load(user_id)?
            } else {
                let enc = encrypted_cookie.ok_or_else(|| {
                    CoreError::Crypto("no encrypted cookie".into())
                })?;
                crypto::decrypt_cookie(&enc, &password)?
            };
            match api::search_games(client, &cookie, &query, 25).await {
                Ok(games) => {
                    info!("SearchGames: got {} results", games.len());
                    Ok(BackendEvent::GamesSearchResults { index, games })
                }
                Err(e) => {
                    info!("SearchGames error: {}", e);
                    Err(e)
                }
            }
        }
        BackendCommand::GetPopularGames { user_id, index, encrypted_cookie, password, use_credential_manager } => {
            info!("GetPopularGames");
            let cookie = if use_credential_manager {
                crypto::credential_load(user_id)?
            } else {
                let enc = encrypted_cookie.ok_or_else(|| {
                    CoreError::Crypto("no encrypted cookie".into())
                })?;
                crypto::decrypt_cookie(&enc, &password)?
            };
            match api::get_popular_games(client, &cookie, 25).await {
                Ok(games) => {
                    info!("GetPopularGames: got {} results", games.len());
                    Ok(BackendEvent::GamesPopularLoaded { index, games })
                }
                Err(e) => {
                    info!("GetPopularGames error: {}", e);
                    Err(e)
                }
            }
        }
        BackendCommand::GetFavoriteGames {
            user_id,
            index,
            encrypted_cookie,
            password,
            use_credential_manager,
        } => {
            let cookie = if use_credential_manager {
                crypto::credential_load(user_id)?
            } else {
                let enc = encrypted_cookie.ok_or_else(|| {
                    CoreError::Crypto("no encrypted cookie".into())
                })?;
                crypto::decrypt_cookie(&enc, &password)?
            };
            match api::get_favorite_games(client, &cookie, user_id).await {
                Ok(games) => {
                    info!("GetFavoriteGames: got {} favorites", games.len());
                    Ok(BackendEvent::FavoriteGamesLoaded { index, games })
                }
                Err(e) => {
                    info!("GetFavoriteGames error: {}", e);
                    Err(e)
                }
            }
        }
        BackendCommand::CreateVipServer {
            universe_id,
            place_id,
            name,
            index,
            encrypted_cookie,
            password,
            use_credential_manager,
        } => {
            let cookie = if use_credential_manager {
                crypto::credential_load(0)?
            } else {
                let enc = encrypted_cookie.ok_or_else(|| {
                    CoreError::Crypto("no encrypted cookie".into())
                })?;
                crypto::decrypt_cookie(&enc, &password)?
            };
            match api::create_vip_server(client, &cookie, universe_id, &name).await {
                Ok(vip) => {
                    info!("CreateVipServer: created {} with code {}", vip.name, vip.access_code);
                    Ok(BackendEvent::VipServerCreated {
                        index,
                        vip_server_id: vip.vip_server_id,
                        access_code: vip.access_code,
                        name: vip.name,
                        place_id,
                    })
                }
                Err(e) => {
                    info!("CreateVipServer error: {}", e);
                    Err(e)
                }
            }
        }
        BackendCommand::ListPrivateServers {
            place_id,
            index,
            encrypted_cookie,
            password,
            use_credential_manager,
        } => {
            let cookie = if use_credential_manager {
                crypto::credential_load(0)?
            } else {
                let enc = encrypted_cookie.ok_or_else(|| {
                    CoreError::Crypto("no encrypted cookie".into())
                })?;
                crypto::decrypt_cookie(&enc, &password)?
            };
            match api::list_private_servers(client, &cookie, place_id, None).await {
                Ok((servers, _cursor)) => {
                    let server_infos: Vec<ram_core::models::PrivateServerInfo> = servers
                        .into_iter()
                        .map(|s| {
                            let owner_name = s.owner.as_ref().map(|o| o.name.clone()).unwrap_or_else(|| "Unknown".to_string());
                            let owner_display_name = s.owner.and_then(|o| o.display_name);
                            ram_core::models::PrivateServerInfo {
                                id: s.id,
                                name: s.name,
                                access_code: s.access_code,
                                active: s.active,
                                owner_name,
                                owner_display_name,
                            }
                        })
                        .collect();
                    info!("ListPrivateServers: got {} servers", server_infos.len());
                    Ok(BackendEvent::PrivateServersLoaded { index, servers: server_infos })
                }
                Err(e) => {
                    info!("ListPrivateServers error: {}", e);
                    Err(e)
                }
            }
        }
        BackendCommand::CheckVipServerPrice {
            universe_id,
            place_id,
            encrypted_cookie,
            password,
            use_credential_manager,
        } => {
            let cookie = if use_credential_manager {
                crypto::credential_load(0)?
            } else {
                let enc = encrypted_cookie.ok_or_else(|| {
                    CoreError::Crypto("no encrypted cookie".into())
                })?;
                crypto::decrypt_cookie(&enc, &password)?
            };
            match api::get_vip_server_price(client, &cookie, universe_id, place_id).await {
                Ok(api::VipPriceResult::Price(price)) => {
                    info!("CheckVipServerPrice: universe={}, place={}, price={}", universe_id, place_id, price);
                    Ok(BackendEvent::VipServerPriceChecked { universe_id, allowed: true, price })
                }
                Ok(api::VipPriceResult::Disabled) => {
                    info!("CheckVipServerPrice: universe={} - VIP disabled", universe_id);
                    Ok(BackendEvent::VipServerPriceChecked { universe_id, allowed: false, price: 0 })
                }
                Ok(api::VipPriceResult::Unknown) => {
                    info!("CheckVipServerPrice: universe={} - price unknown", universe_id);
                    Ok(BackendEvent::VipServerPriceUnknown { universe_id })
                }
                Err(e) => {
                    info!("CheckVipServerPrice error: {}", e);
                    Err(e)
                }
            }
        }
        BackendCommand::BulkImportCookies {
            cookies,
            password,
            use_credential_manager,
            proxy_file,
            use_cached_proxies,
            force_recheck_proxies,
        } => {
            let total_cookies = cookies.len();
            let mut added: Vec<(u64, String, String)> = Vec::new();
            let mut failed_cookies: Vec<(String, String)> = Vec::new();
            let mut working_proxies: Vec<String> = Vec::new();
            let mut failed_proxies: Vec<String> = Vec::new();
            
            // Get proxy cache reference
            let cache_guard = proxy_cache.lock().await;
            let proxy_cache_ref = cache_guard.as_ref();
            
            // STAGE 1: Get working proxies
            // First try cached proxies if enabled
            if use_cached_proxies && !force_recheck_proxies {
                if let Some(cache) = proxy_cache_ref {
                    if cache.has_validated_proxies().await {
                        let cached = cache.get_validated_proxies().await;
                        if !cached.is_empty() {
                            info!("Using {} cached validated proxies", cached.len());
                            working_proxies = cached;
                        }
                    }
                }
            }
            
            // Drop the lock before doing any async operations
            drop(cache_guard);
            
            // If no cached proxies, test from file
            let all_proxies = if working_proxies.is_empty() {
                match &proxy_file {
                    Some(path) if !path.is_empty() => {
                        match proxy::load_proxies_from_file(path) {
                            Ok(proxies) => {
                                info!("Loaded {} proxies from file", proxies.len());
                                proxies
                            }
                            Err(e) => {
                                info!("Failed to load proxies: {}", e);
                                vec![]
                            }
                        }
                    }
                    _ => vec![],
                }
            } else {
                vec![]
            };
            
            let total_proxies = all_proxies.len();
            
            // Test new proxies only if needed
            if !all_proxies.is_empty() && (working_proxies.is_empty() || force_recheck_proxies) {
                let _ = tx.send(BackendEvent::BulkImportProgress {
                    current: 0,
                    total: total_cookies,
                    username: String::new(),
                    proxy: None,
                    stage: format!("Testing proxies (0/{})", total_proxies),
                });
                
                let working = Arc::new(Mutex::new(Vec::<String>::new()));
                let failed = Arc::new(Mutex::new(Vec::<String>::new()));
                let tested_count = Arc::new(AtomicUsize::new(0));
                let tx = tx.clone();
                let all_proxies = all_proxies.clone();
                let total_proxies_len = total_proxies;
                
                // Test all proxies concurrently in batches of 50
                let batch_size = 50;
                for batch_start in (0..total_proxies).step_by(batch_size) {
                    let batch_end = std::cmp::min(batch_start + batch_size, total_proxies);
                    let batch_proxies: Vec<String> = all_proxies[batch_start..batch_end].to_vec();
                    
                    let mut set = JoinSet::new();
                    
                    for proxy_str in batch_proxies {
                        let working = working.clone();
                        let failed = failed.clone();
                        let tested = tested_count.clone();
                        let tx = tx.clone();
                        let total = total_proxies_len;
                        
                        set.spawn(async move {
                            let result = proxy::test_proxy(&proxy_str).await;
                            
                            match result {
                                Ok(true) => {
                                    let mut w = working.lock().await;
                                    w.push(proxy_str.clone());
                                }
                                Ok(false) | Err(_) => {
                                    let mut f = failed.lock().await;
                                    f.push(proxy_str.clone());
                                }
                            }
                            
                            // Update count and send progress
                            let count = tested.fetch_add(1, Ordering::Relaxed) + 1;
                            let w_count = working.lock().await.len();
                            
                            // Send progress every 10 proxies or at end of batch
                            if count % 10 == 0 || count == total {
                                let _ = tx.send(BackendEvent::BulkImportProgress {
                                    current: 0,
                                    total: total_cookies,
                                    username: String::new(),
                                    proxy: Some(format!("Proxy {}/{}", count, total)),
                                    stage: format!("Testing proxies ({}/{}) - {} working", count, total, w_count),
                                });
                            }
                        });
                    }
                    
                    // Wait for batch to complete
                    while set.join_next().await.is_some() {}
                }
                
                // Collect results and update cache
                {
                    let w = working.lock().await;
                    let f = failed.lock().await;
                    
                    working_proxies = w.clone();
                    failed_proxies = f.clone();
                    
                    // Update proxy cache (add to cache, no testing needed)
                    let cache_guard = proxy_cache.lock().await;
                    if let Some(cache) = cache_guard.as_ref() {
                        let _ = cache.add_to_cache(w.clone(), f.clone()).await;
                    }
                    
                    info!("Proxy test complete: {}/{} working", working_proxies.len(), total_proxies);
                }
                
                let _ = tx.send(BackendEvent::ProxyTestComplete {
                    working: working_proxies.clone(),
                    failed: failed_proxies.clone(),
                });
            }
            
            // STAGE 2: Validate cookies concurrently in batches
            let proxy_stats = ProxyStats {
                total_cookies,
                total_proxies,
                working_proxies: working_proxies.len(),
            };
            
            // Categorize cookies based on cache
            let mut to_validate: Vec<(usize, String)> = Vec::new();  // (index, cookie)
            let mut from_cache: Vec<CookieInfo> = Vec::new();  // Already valid from cache
            let mut skipped: Vec<String> = Vec::new();  // Already dead in cache
            
            for (idx, cookie) in cookies.iter().enumerate() {
                let cookie_trimmed = cookie.trim();
                if cookie_trimmed.is_empty() {
                    continue;
                }
                
                // Check cookie cache
                if cookie_cache.is_valid(cookie_trimmed) {
                    // Cookie is valid from cache, use it directly
                    if let Some(info) = cookie_cache.get_info(cookie_trimmed) {
                        from_cache.push(info);
                    }
                } else {
                    // Cookie not in cache or is dead, needs validation
                    to_validate.push((idx, cookie_trimmed.to_string()));
                }
            }
            
            info!("Cookie cache: {} valid from cache, {} to validate, {} cached as dead", 
                from_cache.len(), to_validate.len(), skipped.len());
            
            // Add cached cookies to results
            for info in &from_cache {
                added.push((info.user_id, info.username.clone(), info.display_name.clone()));
            }
            
            // Now validate the remaining cookies
            let cookies_to_validate: Vec<String> = to_validate.into_iter().map(|(_, c)| c).collect();
            let batch_size = 50;
            let total_to_validate = cookies_to_validate.len();
            let total_valid = total_to_validate + from_cache.len();
            let processed_from_cache = from_cache.len();
            
            // Check if we have proxies
            let has_proxies = !working_proxies.is_empty();
            
            // Create proxy pool for cookie validation
            let proxy_pool = if has_proxies {
                Some(proxy::ProxyPool::new(working_proxies.clone()))
            } else {
                None
            };
            
            // If no proxies, use smaller batches and add delays
            let effective_batch_size = if has_proxies { 50 } else { 10 };
            let delay_between_batches = if has_proxies { 
                std::time::Duration::from_millis(100) 
            } else { 
                std::time::Duration::from_secs(2)  // 2 second delay between batches when no proxy
            };
            
            let cache_arc = proxy_cache.clone();
            let cookie_cache = cookie_cache.clone();
            let cred_mgr = use_credential_manager;
            let pwd = password.clone();
            
            for batch_start in (0..total_to_validate).step_by(effective_batch_size) {
                let batch_end = std::cmp::min(batch_start + effective_batch_size, total_to_validate);
                let batch: Vec<String> = cookies_to_validate[batch_start..batch_end].to_vec();
                
                // Add delay between batches when no proxy (except first batch)
                if batch_start > 0 && !has_proxies {
                    tokio::time::sleep(delay_between_batches).await;
                }
                
                let mut set = JoinSet::new();
                let tx = tx.clone();
                let cache_arc = cache_arc.clone();
                let cred = cred_mgr;
                let pass = pwd.clone();
                let cookie_cache = cookie_cache.clone();
                
for (idx, cookie) in batch.into_iter().enumerate() {
                    let global_idx = batch_start + idx;
                    let pool = proxy_pool.clone();
                    let tx = tx.clone();
                    
                    set.spawn(async move {
                        let current_proxy = if has_proxies {
                            pool.as_ref().and_then(|p| p.get_current_proxy(global_idx))
                        } else {
                            None
                        };
                        
                        // Try validation with proxy first (if available), then without
                        let result = if has_proxies {
                            if let Some(ref p) = pool {
                                p.validate_cookie_with_proxy(&cookie).await
                            } else {
                                Err(CoreError::AuthFailed("No proxies available".to_string()))
                            }
                        } else {
                            // No proxies, use direct validation
                            let client = RobloxClient::default();
                            client.validate_cookie(&cookie).await
                        };
                        
                        let (user_id, username, display_name, success) = match result {
                            Ok((uid, uname, dname)) => (uid, uname, dname, true),
                            Err(_) => {
                                // Try without proxy if we haven't tried already
                                let client = RobloxClient::default();
                                match client.validate_cookie(&cookie).await {
                                    Ok((uid, uname, dname)) => (uid, uname, dname, true),
                                    Err(_) => {
                                        // Cookie validation failed entirely
                                        // Don't mark proxy as dead - cookie might be invalid
                                        (0, String::new(), String::new(), false)
                                    }
                                }
                            }
                        };
                        
                        // Try validation with proxy first (if available), then without
                        let result = if has_proxies {
                            if let Some(ref p) = pool {
                                p.validate_cookie_with_proxy(&cookie).await
                            } else {
                                Err(CoreError::AuthFailed("No proxies available".to_string()))
                            }
                        } else {
                            // No proxies, use direct validation
                            let client = RobloxClient::default();
                            client.validate_cookie(&cookie).await
                        };
                        
                        let (user_id, username, display_name, success) = match result {
                            Ok((uid, uname, dname)) => (uid, uname, dname, true),
                            Err(_) => {
                                // Try without proxy
                                let client = RobloxClient::default();
                                match client.validate_cookie(&cookie).await {
                                    Ok((uid, uname, dname)) => (uid, uname, dname, true),
                                    Err(_e) => {
                                        // Cookie validation failed entirely
                                        // Don't mark proxy as dead - cookie might be invalid
                                        // Proxy should only be marked dead during proxy testing phase
                                        (0, String::new(), String::new(), false)
                                    }
                                }
                            }
                        };
                        
                        let final_idx = processed_from_cache + global_idx + 1;
                        let _ = tx.send(BackendEvent::BulkImportProgress {
                            current: final_idx,
                            total: total_valid,
                            username: username.clone(),
                            proxy: current_proxy,
                            stage: if has_proxies {
                                format!("Validating Cookie {}/{}", final_idx, total_valid)
                            } else {
                                format!("Validating (no proxy) {}/{}", final_idx, total_valid)
                            },
                        });
                        
                        (global_idx, cookie, user_id, username, display_name, success)
                    });
                }
                
                // Collect batch results
                while let Some(result) = set.join_next().await {
                    if let Ok((_idx, cookie, user_id, username, display_name, success)) = result {
                        if success && user_id > 0 {
                            let _encrypted = if cred {
                                let _ = crypto::credential_store(user_id, &cookie);
                                None
                            } else {
                                Some(crypto::encrypt_cookie(&cookie, &pass).unwrap_or_default())
                            };
                            
                            added.push((user_id, username.clone(), display_name.clone()));
                            
                            // Add to cookie cache
                            cookie_cache.add_valid(CookieInfo {
                                cookie: cookie.clone(),
                                user_id,
                                username: username.clone(),
                                display_name,
                            });
                        } else {
                            failed_cookies.push((cookie.clone(), "Validation failed".to_string()));
                            
                            // Mark as dead in cache
                            cookie_cache.add_dead(&cookie);
                        }
                    }
                }
            }
            
            let total_added = added.len();
            let total_failed = failed_cookies.len();
            info!("Bulk import complete: {} added, {} cookies failed, {}/{} proxies working", 
                total_added, total_failed, working_proxies.len(), total_proxies);
            Ok(BackendEvent::BulkImportComplete { 
                added, 
                failed: failed_cookies,
                proxy_stats: Some(proxy_stats),
            })
        }
        BackendCommand::FetchCurrency {
            user_id,
            encrypted_cookie,
            password,
            use_credential_manager,
        } => {
            let cookie = if let Some(ref enc) = encrypted_cookie {
                if use_credential_manager {
                    crypto::credential_load(user_id)?
                } else {
                    crypto::decrypt_cookie(enc, &password)?
                }
            } else {
                return Err(CoreError::AccountNotFound(user_id.to_string()));
            };

            match api::fetch_currency(&client, &cookie).await {
                Ok((robux, is_premium)) => {
                    info!("Fetched currency for user {}: {} Robux, premium: {}", user_id, robux, is_premium);
                    Ok(BackendEvent::CurrencyUpdated { user_id, robux, is_premium })
                }
                Err(e) => {
                    info!("Failed to fetch currency for user {}: {}", user_id, e);
                    Err(e)
                }
            }
        }
        BackendCommand::FetchUserGroups {
            user_id,
            encrypted_cookie,
            password,
            use_credential_manager,
        } => {
            let cookie = if let Some(ref enc) = encrypted_cookie {
                if use_credential_manager {
                    crypto::credential_load(user_id)?
                } else {
                    crypto::decrypt_cookie(enc, &password)?
                }
            } else {
                return Err(CoreError::AccountNotFound(user_id.to_string()));
            };

            match api::fetch_user_groups(&client, &cookie, user_id).await {
                Ok(groups) => {
                    info!("Fetched {} groups for user {}", groups.len(), user_id);
                    Ok(BackendEvent::UserGroupsLoaded { user_id, groups })
                }
                Err(e) => {
                    info!("Failed to fetch groups for user {}: {}", user_id, e);
                    Err(e)
                }
            }
        }
        BackendCommand::FetchGroupCurrency {
            user_id,
            group_id,
            group_name,
            encrypted_cookie,
            password,
            use_credential_manager,
        } => {
            let cookie = if let Some(ref enc) = encrypted_cookie {
                if use_credential_manager {
                    crypto::credential_load(user_id)?
                } else {
                    crypto::decrypt_cookie(enc, &password)?
                }
            } else {
                return Err(CoreError::AccountNotFound(user_id.to_string()));
            };

            match api::fetch_group_currency(&client, &cookie, group_id).await {
                Ok(Some(currency)) => {
                    info!("Fetched {} Robux for group {}", currency.robux_balance, group_id);
                    Ok(BackendEvent::GroupCurrencyLoaded {
                        group_id,
                        group_name,
                        robux: currency.robux_balance,
                        pending: currency.pending_robux,
                    })
                }
                Ok(None) => {
                    info!("No permission to view Robux for group {}", group_id);
                    Ok(BackendEvent::GroupCurrencyLoaded {
                        group_id,
                        group_name,
                        robux: u64::MAX,
                        pending: 0,
                    })
                }
                Err(e) => {
                    info!("Failed to fetch currency for group {}: {}", group_id, e);
                    Err(e)
                }
            }
        }
    }
}
