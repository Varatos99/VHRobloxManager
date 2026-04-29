//! Cookie cache management system.
//!
//! Stores cookie validation state in %APPDATA%/VHrobloxManager/kuki/
//! Files:
//!   - indexkuki.json: All known cookies with their history
//!   - validkuki.json: Currently valid cookies
//!   - deadkuki.json: Cookies that failed recently

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{info, warn};

use crate::error::CoreError;

/// Days until valid cookies expire
const COOKIE_CACHE_VALIDITY_DAYS: u64 = 3;

/// Cookie index entry - tracks all known cookies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CookieIndex {
    pub last_updated: String,
    pub cookies: HashMap<String, CookieEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CookieEntry {
    pub first_seen: String,
    pub last_seen: String,
    pub total_validations: u32,
    pub success_count: u32,
    pub user_id: Option<u64>,
    pub username: Option<String>,
}

/// Valid cookies - currently working ones
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidCookieList {
    pub last_updated: String,
    pub valid_until: String,
    pub cookies: Vec<CookieInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CookieInfo {
    pub cookie: String,
    pub user_id: u64,
    pub username: String,
    pub display_name: String,
}

/// Dead cookies - recently failed ones
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadCookieList {
    pub last_updated: String,
    pub cookies: Vec<DeadCookieEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadCookieEntry {
    pub cookie: String,
    pub last_failed: String,
    pub fail_count: u32,
}

/// Get the cookie cache directory path
fn cookie_cache_dir() -> PathBuf {
    let base = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    base.join("VHrobloxManager").join("kuki")
}

fn now_iso() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn days_from_now(days: u64) -> String {
    let future = chrono::Utc::now() + chrono::Duration::days(days as i64);
    future.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

impl CookieIndex {
    pub fn load() -> Result<Self, CoreError> {
        let path = cookie_cache_dir().join("indexkuki.json");
        if path.exists() {
            let content = std::fs::read_to_string(&path)
                .map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
            serde_json::from_str(&content).map_err(|e| CoreError::InvalidProxy(e.to_string()))
        } else {
            Ok(Self {
                last_updated: now_iso(),
                cookies: HashMap::new(),
            })
        }
    }

    pub fn save(&self) -> Result<(), CoreError> {
        let dir = cookie_cache_dir();
        std::fs::create_dir_all(&dir).map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
        let path = dir.join("indexkuki.json");
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
        std::fs::write(path, content).map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
        Ok(())
    }

    pub fn get(&self, cookie: &str) -> Option<&CookieEntry> {
        self.cookies.get(cookie)
    }

    pub fn update(
        &mut self,
        cookie: &str,
        success: bool,
        user_id: Option<u64>,
        username: Option<String>,
    ) {
        let entry = self
            .cookies
            .entry(cookie.to_string())
            .or_insert_with(|| CookieEntry {
                first_seen: now_iso(),
                last_seen: now_iso(),
                total_validations: 0,
                success_count: 0,
                user_id: None,
                username: None,
            });
        entry.last_seen = now_iso();
        entry.total_validations += 1;
        if success {
            entry.success_count += 1;
        }
        if let Some(uid) = user_id {
            entry.user_id = Some(uid);
        }
        if let Some(name) = username {
            entry.username = Some(name);
        }
        self.last_updated = now_iso();
    }

    pub fn is_valid(&self, cookie: &str) -> bool {
        self.cookies
            .get(cookie)
            .map(|e| e.success_count > 0 && e.total_validations > 0)
            .unwrap_or(false)
    }
}

impl ValidCookieList {
    pub fn load() -> Result<Self, CoreError> {
        let path = cookie_cache_dir().join("validkuki.json");
        if path.exists() {
            let content = std::fs::read_to_string(&path)
                .map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
            serde_json::from_str(&content).map_err(|e| CoreError::InvalidProxy(e.to_string()))
        } else {
            Ok(Self {
                last_updated: now_iso(),
                valid_until: days_from_now(COOKIE_CACHE_VALIDITY_DAYS),
                cookies: Vec::new(),
            })
        }
    }

    pub fn save(&self) -> Result<(), CoreError> {
        let dir = cookie_cache_dir();
        std::fs::create_dir_all(&dir).map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
        let path = dir.join("validkuki.json");
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
        std::fs::write(path, content).map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
        Ok(())
    }

    pub fn has_cookie(&self, cookie: &str) -> bool {
        self.cookies.iter().any(|c| c.cookie == cookie)
    }

    pub fn get_cookie(&self, cookie: &str) -> Option<&CookieInfo> {
        self.cookies.iter().find(|c| c.cookie == cookie)
    }

    pub fn add_cookie(&mut self, info: CookieInfo) {
        // Remove if already exists
        self.cookies.retain(|c| c.cookie != info.cookie);
        self.cookies.push(info);
        self.last_updated = now_iso();
    }

    pub fn remove_cookie(&mut self, cookie: &str) {
        self.cookies.retain(|c| c.cookie != cookie);
        self.last_updated = now_iso();
    }
}

impl DeadCookieList {
    pub fn load() -> Result<Self, CoreError> {
        let path = cookie_cache_dir().join("deadkuki.json");
        if path.exists() {
            let content = std::fs::read_to_string(&path)
                .map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
            serde_json::from_str(&content).map_err(|e| CoreError::InvalidProxy(e.to_string()))
        } else {
            Ok(Self {
                last_updated: now_iso(),
                cookies: Vec::new(),
            })
        }
    }

    pub fn save(&self) -> Result<(), CoreError> {
        let dir = cookie_cache_dir();
        std::fs::create_dir_all(&dir).map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
        let path = dir.join("deadkuki.json");
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
        std::fs::write(path, content).map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
        Ok(())
    }

    pub fn add_dead(&mut self, cookie: String) {
        if let Some(existing) = self.cookies.iter_mut().find(|e| e.cookie == cookie) {
            existing.fail_count += 1;
            existing.last_failed = now_iso();
        } else {
            self.cookies.push(DeadCookieEntry {
                cookie,
                last_failed: now_iso(),
                fail_count: 1,
            });
        }
        self.last_updated = now_iso();
    }

    pub fn remove_cookie(&mut self, cookie: &str) {
        self.cookies.retain(|e| e.cookie != cookie);
        self.last_updated = now_iso();
    }
}

/// CookieCache manages all cookie state
pub struct CookieCache {
    index: std::sync::Mutex<CookieIndex>,
    valid: std::sync::Mutex<ValidCookieList>,
    dead: std::sync::Mutex<DeadCookieList>,
}

impl CookieCache {
    pub fn new() -> Result<Self, CoreError> {
        let cache = Self {
            index: std::sync::Mutex::new(CookieIndex::load()?),
            valid: std::sync::Mutex::new(ValidCookieList::load()?),
            dead: std::sync::Mutex::new(DeadCookieList::load()?),
        };

        info!(
            "Cookie cache loaded: {} valid, {} dead",
            cache.valid.lock().unwrap().cookies.len(),
            cache.dead.lock().unwrap().cookies.len()
        );

        Ok(cache)
    }

    /// Check if a cookie is cached as valid
    pub fn is_valid(&self, cookie: &str) -> bool {
        self.valid.lock().unwrap().has_cookie(cookie)
    }

    /// Get cookie info if cached as valid
    pub fn get_info(&self, cookie: &str) -> Option<CookieInfo> {
        self.valid.lock().unwrap().get_cookie(cookie).cloned()
    }

    /// Get all cached valid cookies
    pub fn get_valid_cookies(&self) -> Vec<CookieInfo> {
        self.valid.lock().unwrap().cookies.clone()
    }

    /// Get count of valid cookies
    pub fn valid_count(&self) -> usize {
        self.valid.lock().unwrap().cookies.len()
    }

    /// Add a valid cookie to cache
    pub fn add_valid(&self, info: CookieInfo) {
        // Update index
        if let Ok(mut index) = self.index.lock() {
            index.update(
                &info.cookie,
                true,
                Some(info.user_id),
                Some(info.username.clone()),
            );
            let _ = index.save();
        }

        // Add to valid list
        if let Ok(mut valid) = self.valid.lock() {
            valid.add_cookie(info.clone());
            let _ = valid.save();
        }

        // Remove from dead if exists
        if let Ok(mut dead) = self.dead.lock() {
            dead.remove_cookie(&info.cookie);
            let _ = dead.save();
        }

        info!(
            "Cookie added to valid cache: {} (id: {})",
            info.username, info.user_id
        );
    }

    /// Mark a cookie as dead
    pub fn add_dead(&self, cookie: &str) {
        // Update index
        if let Ok(mut index) = self.index.lock() {
            index.update(cookie, false, None, None);
            let _ = index.save();
        }

        // Add to dead list
        if let Ok(mut dead) = self.dead.lock() {
            dead.add_dead(cookie.to_string());
            let _ = dead.save();
        }

        // Remove from valid if exists
        if let Ok(mut valid) = self.valid.lock() {
            valid.remove_cookie(cookie);
            let _ = valid.save();
        }

        info!("Cookie marked as dead: {}", &cookie[..20.min(cookie.len())]);
    }

    /// Bulk add valid cookies
    pub fn bulk_add_valid(&self, cookies: Vec<CookieInfo>) {
        let count = cookies.len();

        for info in &cookies {
            if let Ok(mut index) = self.index.lock() {
                index.update(
                    &info.cookie,
                    true,
                    Some(info.user_id),
                    Some(info.username.clone()),
                );
            }
        }
        if let Ok(mut index) = self.index.lock() {
            let _ = index.save();
        }

        if let Ok(mut valid) = self.valid.lock() {
            for info in cookies {
                valid.add_cookie(info);
            }
            let _ = valid.save();
        }

        info!("Bulk added {} cookies to valid cache", count);
    }

    /// Bulk add dead cookies  
    pub fn bulk_add_dead(&self, cookies: Vec<String>) {
        let count = cookies.len();

        for cookie in &cookies {
            if let Ok(mut index) = self.index.lock() {
                index.update(cookie, false, None, None);
            }
        }
        if let Ok(mut index) = self.index.lock() {
            let _ = index.save();
        }

        if let Ok(mut dead) = self.dead.lock() {
            for cookie in cookies {
                dead.add_dead(cookie);
            }
            let _ = dead.save();
        }

        info!("Bulk marked {} cookies as dead", count);
    }

    /// Clear all cached data
    #[allow(dead_code)]
    pub fn clear(&self) -> Result<(), CoreError> {
        let dir = cookie_cache_dir();
        for name in &["indexkuki.json", "validkuki.json", "deadkuki.json"] {
            let path = dir.join(name);
            if path.exists() {
                std::fs::remove_file(path).map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
            }
        }

        info!("Cookie cache cleared");
        Ok(())
    }
}
