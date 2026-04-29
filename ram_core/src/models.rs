use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// A single Roblox account managed by RM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    /// Roblox user ID.
    pub user_id: u64,
    /// Display name on Roblox.
    pub display_name: String,
    /// Roblox username.
    pub username: String,
    /// The encrypted `.ROBLOSECURITY` cookie (never stored in plaintext).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted_cookie: Option<String>,
    /// Optional alias set by the user for quick identification.
    #[serde(default)]
    pub alias: String,
    /// Optional group/tag for organizing accounts.
    #[serde(default)]
    pub group: String,
    /// Cached avatar thumbnail URL.
    #[serde(default)]
    pub avatar_url: String,
    /// Last known online presence.
    #[serde(default)]
    pub last_presence: Presence,
    /// User's Robux balance.
    #[serde(default)]
    pub robux_balance: Option<u64>,
    /// Premium membership status.
    #[serde(default)]
    pub is_premium: bool,
    /// Timestamp of the last successful login/validation.
    pub last_validated: Option<DateTime<Utc>>,
    /// True if the last automatic revalidation found the cookie expired.
    #[serde(default)]
    pub cookie_expired: bool,
    /// Manual sort position (used in Custom sort mode). `u32::MAX` = not yet positioned.
    #[serde(default = "default_sort_order")]
    pub sort_order: u32,
}

impl Account {
    pub fn new(user_id: u64, username: String, display_name: String) -> Self {
        Self {
            user_id,
            display_name,
            username,
            encrypted_cookie: None,
            alias: String::new(),
            group: String::new(),
            avatar_url: String::new(),
            last_presence: Presence::default(),
            last_validated: None,
            cookie_expired: false,
            sort_order: u32::MAX,
            robux_balance: None,
            is_premium: false,
        }
    }

    /// Returns the label shown in the sidebar (alias if set, otherwise username).
    pub fn label(&self) -> &str {
        if self.alias.is_empty() {
            &self.username
        } else {
            &self.alias
        }
    }
}

/// Roblox user presence information.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Presence {
    /// 0 = Offline, 1 = Online, 2 = InGame, 3 = InStudio
    pub user_presence_type: u8,
    /// Place ID the user is currently in (if in-game).
    pub place_id: Option<u64>,
    /// Job/server ID (if in-game).
    pub game_id: Option<String>,
    /// Universe ID (if in-game).
    #[serde(default)]
    pub universe_id: Option<u64>,
    /// Human-readable status text from Roblox.
    pub last_location: String,
}

impl Presence {
    pub fn status_text(&self) -> &str {
        match self.user_presence_type {
            0 => "Offline",
            1 => "Online",
            2 => "In Game",
            3 => "In Studio",
            _ => "Unknown",
        }
    }

    pub fn is_online(&self) -> bool {
        self.user_presence_type > 0
    }
}

/// The persistent store of all accounts, serialized to disk as encrypted JSON.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AccountStore {
    pub accounts: Vec<Account>,
}

impl AccountStore {
    pub fn find_by_id(&self, user_id: u64) -> Option<&Account> {
        self.accounts.iter().find(|a| a.user_id == user_id)
    }

    pub fn find_by_id_mut(&mut self, user_id: u64) -> Option<&mut Account> {
        self.accounts.iter_mut().find(|a| a.user_id == user_id)
    }

    pub fn remove_by_id(&mut self, user_id: u64) -> bool {
        let before = self.accounts.len();
        self.accounts.retain(|a| a.user_id != user_id);
        self.accounts.len() < before
    }
}

/// Global application configuration persisted to `config.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Path to the encrypted accounts file.
    pub accounts_path: PathBuf,
    /// Whether to use Windows Credential Manager instead of file-based encryption.
    pub use_credential_manager: bool,
    /// Enable multi-instance mutex patching (risky — user opt-in).
    #[serde(default)]
    pub multi_instance_enabled: bool,
    /// Automatically kill Roblox background tray processes (`--launch-to-tray`).
    /// Always active when multi-instance is enabled; can also be used standalone.
    #[serde(default)]
    pub kill_background_roblox: bool,
    /// Custom Roblox player install path override.
    pub roblox_player_path: Option<PathBuf>,
    /// Saved window dimensions.
    pub window_width: f32,
    pub window_height: f32,
    /// Per-group color/tag metadata.
    #[serde(default)]
    pub groups: HashMap<String, GroupMeta>,
    /// Saved favorite places for quick launching.
    #[serde(default)]
    pub favorite_places: Vec<FavoritePlace>,
    /// Clear RobloxCookies.dat before each launch to prevent account association.
    #[serde(default = "default_true")]
    pub privacy_mode: bool,
    /// Automatically arrange Roblox windows in a grid after launching.
    #[serde(default)]
    pub auto_arrange_windows: bool,
    /// Replace usernames/display names with generic "Account 1", "Account 2", etc.
    #[serde(default)]
    pub anonymize_names: bool,
    /// Ignore multi-instance close warning (show once per session).
    #[serde(default)]
    pub ignore_multi_instance_warning: bool,
    /// Last version the user has seen — used to detect first launch after update.
    #[serde(default)]
    pub last_seen_version: Option<String>,
    /// Persisted sidebar sort mode: "Custom", "Name", or "Status".
    #[serde(default = "default_sort_mode")]
    pub sort_mode: String,
    /// Check for updates on startup.
    #[serde(default)]
    pub check_for_updates: bool,
    /// Saved private servers for quick launching.
    #[serde(default)]
    pub private_servers: Vec<PrivateServer>,
}

fn default_sort_mode() -> String {
    "Custom".to_string()
}

fn default_true() -> bool {
    true
}

fn default_sort_order() -> u32 {
    u32::MAX
}

impl Default for AppConfig {
    fn default() -> Self {
        let data_dir = std::env::var("APPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("."));
        Self {
            accounts_path: data_dir.join("VHRobloxManager").join("accounts.dat"),
            use_credential_manager: false,
            multi_instance_enabled: false,
            kill_background_roblox: false,
            roblox_player_path: None,
            window_width: 960.0,
            window_height: 640.0,
            groups: HashMap::new(),
            favorite_places: Vec::new(),
            privacy_mode: true,
            auto_arrange_windows: false,
            anonymize_names: false,
            last_seen_version: None,
            sort_mode: "Custom".to_string(),
            private_servers: Vec::new(),
            check_for_updates: false,
            ignore_multi_instance_warning: false,
        }
    }
}

impl AppConfig {
    /// Load from a JSON file, falling back to defaults.
    pub fn load(path: &std::path::Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Persist to a JSON file.
    pub fn save(&self, path: &std::path::Path) -> Result<(), crate::CoreError> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }
}

/// Optional metadata for account groupings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupMeta {
    pub color: [u8; 3],
    pub description: String,
    /// Manual sort position for group ordering. `u32::MAX` = not yet positioned.
    #[serde(default = "default_sort_order")]
    pub sort_order: u32,
}

/// A saved favorite place for quick launching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FavoritePlace {
    pub name: String,
    pub place_id: u64,
}

/// A saved private server for quick launching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivateServer {
    /// User-assigned name for this private server.
    pub name: String,
    /// The Roblox place ID.
    pub place_id: u64,
    /// The universe (experience) ID — used to resolve game name and icon without auth.
    #[serde(default)]
    pub universe_id: Option<u64>,
    /// The private server link code (from the URL parameter `privateServerLinkCode`).
    pub link_code: String,
    /// The UUID access code needed for launching (scraped from game page).
    #[serde(default)]
    pub access_code: String,
    /// Resolved place name from Roblox API (cached).
    #[serde(default)]
    pub place_name: String,
}

/// A Roblox friend entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Friend {
    pub user_id: u64,
    pub username: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub is_online: bool,
    #[serde(default)]
    pub presence: Presence,
    #[serde(skip)]
    pub avatar_bytes: Option<Vec<u8>>,
}

/// Incoming friend request entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FriendRequest {
    pub user_id: u64,
    pub username: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub created: String,
}

/// Game search result entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameSearchResult {
    pub place_id: u64,
    pub name: String,
    pub description: String,
    pub root_place_id: u64,
    pub thumbnail_url: String,
    #[serde(default)]
    pub universe_id: Option<u64>,
    #[serde(default)]
    pub playing: u64,
    #[serde(default)]
    pub visits: u64,
    #[serde(default)]
    pub max_players: u64,
    #[serde(default)]
    pub create_vip_servers_allowed: bool,
    #[serde(default)]
    pub vip_server_price: u64,
}

/// VIP Server created for a game
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VipServerInfo {
    pub vip_server_id: u64,
    pub access_code: String,
    pub name: String,
    pub active: bool,
    pub place_id: u64,
    pub universe_id: u64,
}

/// Private server listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivateServerInfo {
    pub id: u64,
    pub name: String,
    pub access_code: String,
    pub active: bool,
    pub owner_name: String,
    pub owner_display_name: Option<String>,
}
