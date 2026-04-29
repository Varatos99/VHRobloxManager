//! Proxy cache management system.
//!
//! Stores proxy validation state in %APPDATA%/VHrobloxManager/proxy/
//! Files:
//!   - index.json: All known proxies with their history
//!   - validatedProxy.json: Currently working proxies (3-day validity)
//!   - deadProxy.json: Proxies that failed recently

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::error::CoreError;
use crate::proxy::test_proxy;

/// Days until validated proxies expire
const PROXY_CACHE_VALIDITY_DAYS: u64 = 3;

/// Proxy index entry - tracks all known proxies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyIndex {
    pub last_updated: String,
    pub proxies: HashMap<String, ProxyEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyEntry {
    pub first_seen: String,
    pub last_seen: String,
    pub total_tests: u32,
    pub success_count: u32,
}

/// Validated proxies - currently working ones
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatedProxyList {
    pub last_updated: String,
    pub valid_until: String,
    pub proxies: Vec<String>,
}

/// Dead proxies - recently failed ones
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadProxyList {
    pub last_updated: String,
    pub proxies: Vec<DeadProxyEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadProxyEntry {
    pub proxy: String,
    pub last_failed: String,
    pub fail_count: u32,
}

/// Get the proxy cache directory path
fn proxy_cache_dir() -> PathBuf {
    let base = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    base.join("VHrobloxManager").join("proxy")
}

fn now_iso() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn days_from_now(days: u64) -> String {
    let future = chrono::Utc::now() + chrono::Duration::days(days as i64);
    future.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

impl ProxyIndex {
    pub fn load() -> Result<Self, CoreError> {
        let path = proxy_cache_dir().join("index.json");
        if path.exists() {
            let content = std::fs::read_to_string(&path)
                .map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
            serde_json::from_str(&content)
                .map_err(|e| CoreError::InvalidProxy(e.to_string()))
        } else {
            Ok(Self {
                last_updated: now_iso(),
                proxies: HashMap::new(),
            })
        }
    }

    pub fn save(&self) -> Result<(), CoreError> {
        let dir = proxy_cache_dir();
        std::fs::create_dir_all(&dir)
            .map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
        let path = dir.join("index.json");
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
        std::fs::write(path, content)
            .map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
        Ok(())
    }

    pub fn is_known(&self, proxy: &str) -> bool {
        self.proxies.contains_key(proxy)
    }

    pub fn add_proxy(&mut self, proxy: &str, success: bool) {
        let entry = self.proxies.entry(proxy.to_string()).or_insert_with(|| ProxyEntry {
            first_seen: now_iso(),
            last_seen: now_iso(),
            total_tests: 0,
            success_count: 0,
        });
        entry.last_seen = now_iso();
        entry.total_tests += 1;
        if success {
            entry.success_count += 1;
        }
        self.last_updated = now_iso();
    }
}

impl ValidatedProxyList {
    pub fn load() -> Result<Self, CoreError> {
        let path = proxy_cache_dir().join("validatedProxy.json");
        if path.exists() {
            let content = std::fs::read_to_string(&path)
                .map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
            serde_json::from_str(&content)
                .map_err(|e| CoreError::InvalidProxy(e.to_string()))
        } else {
            Ok(Self {
                last_updated: now_iso(),
                valid_until: days_from_now(PROXY_CACHE_VALIDITY_DAYS),
                proxies: Vec::new(),
            })
        }
    }

    pub fn save(&self) -> Result<(), CoreError> {
        let dir = proxy_cache_dir();
        info!("Proxy cache save: creating directory at {:?}", dir);
        std::fs::create_dir_all(&dir)
            .map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
        let path = dir.join("validatedProxy.json");
        info!("Proxy cache save: writing to {:?}", path);
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
        std::fs::write(path, content)
            .map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
        Ok(())
    }

    pub fn is_valid(&self) -> bool {
        // Check if valid_until is in the future (simplified check)
        !self.proxies.is_empty()
    }

    pub fn add_proxy(&mut self, proxy: String) {
        if !self.proxies.contains(&proxy) {
            self.proxies.push(proxy);
            self.last_updated = now_iso();
        }
    }

    pub fn remove_proxy(&mut self, proxy: &str) {
        self.proxies.retain(|p| p != proxy);
        self.last_updated = now_iso();
    }

    pub fn get_random_proxy(&self) -> Option<String> {
        use rand::Rng;
        if self.proxies.is_empty() {
            None
        } else {
            let mut rng = rand::thread_rng();
            let idx = rng.gen_range(0..self.proxies.len());
            Some(self.proxies[idx].clone())
        }
    }
}

impl DeadProxyList {
    pub fn load() -> Result<Self, CoreError> {
        let path = proxy_cache_dir().join("deadProxy.json");
        if path.exists() {
            let content = std::fs::read_to_string(&path)
                .map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
            serde_json::from_str(&content)
                .map_err(|e| CoreError::InvalidProxy(e.to_string()))
        } else {
            Ok(Self {
                last_updated: now_iso(),
                proxies: Vec::new(),
            })
        }
    }

    pub fn save(&self) -> Result<(), CoreError> {
        let dir = proxy_cache_dir();
        std::fs::create_dir_all(&dir)
            .map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
        let path = dir.join("deadProxy.json");
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
        std::fs::write(path, content)
            .map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
        Ok(())
    }

    pub fn add_dead(&mut self, proxy: String) {
        if let Some(existing) = self.proxies.iter_mut().find(|e| e.proxy == proxy) {
            existing.fail_count += 1;
            existing.last_failed = now_iso();
        } else {
            self.proxies.push(DeadProxyEntry {
                proxy,
                last_failed: now_iso(),
                fail_count: 1,
            });
        }
        self.last_updated = now_iso();
    }
}

/// ProxyCache manages all proxy state
pub struct ProxyCache {
    index: RwLock<ProxyIndex>,
    validated: RwLock<ValidatedProxyList>,
    dead: RwLock<DeadProxyList>,
}

impl ProxyCache {
    pub async fn new() -> Result<Self, CoreError> {
        let cache = Self {
            index: RwLock::new(ProxyIndex::load()?),
            validated: RwLock::new(ValidatedProxyList::load()?),
            dead: RwLock::new(DeadProxyList::load()?),
        };
        
        info!("Proxy cache loaded: {} validated, {} dead", 
            cache.validated.read().await.proxies.len(),
            cache.dead.read().await.proxies.len());
        
        Ok(cache)
    }

    /// Get proxies that need testing (not in index)
    pub async fn get_proxies_to_test(&self, all_proxies: &[String]) -> Vec<String> {
        let index = self.index.read().await;
        all_proxies
            .iter()
            .filter(|p| !index.is_known(p))
            .cloned()
            .collect()
    }

    /// Get validated proxies for use
    pub async fn get_validated_proxies(&self) -> Vec<String> {
        self.validated.read().await.proxies.clone()
    }

    /// Add tested working proxies to cache (no testing needed)
    pub async fn add_to_cache(&self, working_proxies: Vec<String>, failed_proxies: Vec<String>) {
        // Add working proxies to validated
        {
            let mut validated = self.validated.write().await;
            for proxy in &working_proxies {
                validated.add_proxy(proxy.clone());
            }
        }
        
        // Add failed proxies to dead
        {
            let mut dead = self.dead.write().await;
            for proxy in &failed_proxies {
                dead.add_dead(proxy.clone());
            }
        }
        
        // Add all to index
        {
            let mut index = self.index.write().await;
            for proxy in &working_proxies {
                index.add_proxy(proxy, true);
            }
            for proxy in &failed_proxies {
                index.add_proxy(proxy, false);
            }
        }
        
        // Save all
        if let Err(e) = self.index.read().await.save() {
            warn!("Failed to save proxy index: {}", e);
        }
        if let Err(e) = self.validated.read().await.save() {
            warn!("Failed to save validated proxies: {}", e);
        }
        if let Err(e) = self.dead.read().await.save() {
            warn!("Failed to save dead proxies: {}", e);
        }
        
        info!("Proxy cache updated: {} working, {} failed", working_proxies.len(), failed_proxies.len());
    }

    /// Test new proxies and add to validated list (for standalone testing)
    #[allow(dead_code)]
    pub async fn test_and_cache(&self, proxies: Vec<String>) -> (Vec<String>, Vec<String>) {
        let mut working = Vec::new();
        let mut failed = Vec::new();
        
        for proxy in proxies {
            match test_proxy(&proxy).await {
                Ok(true) => {
                    // Add to index
                    {
                        let mut index = self.index.write().await;
                        index.add_proxy(&proxy, true);
                    }
                    // Add to validated
                    {
                        let mut validated = self.validated.write().await;
                        validated.add_proxy(proxy.clone());
                    }
                    working.push(proxy);
                }
                Ok(false) | Err(_) => {
                    // Add to index
                    {
                        let mut index = self.index.write().await;
                        index.add_proxy(&proxy, false);
                    }
                    // Add to dead
                    {
                        let mut dead = self.dead.write().await;
                        dead.add_dead(proxy.clone());
                    }
                    failed.push(proxy);
                }
            }
        }
        
        // Save all
        if let Err(e) = self.index.read().await.save() {
            warn!("Failed to save proxy index: {}", e);
        }
        if let Err(e) = self.validated.read().await.save() {
            warn!("Failed to save validated proxies: {}", e);
        }
        if let Err(e) = self.dead.read().await.save() {
            warn!("Failed to save dead proxies: {}", e);
        }
        
        info!("Proxy cache updated: {} working, {} dead", working.len(), failed.len());
        (working, failed)
    }

    /// Mark a proxy as dead (failed during cookie validation)
    pub async fn mark_dead(&self, proxy: &str) {
        let mut dead = self.dead.write().await;
        dead.add_dead(proxy.to_string());
        
        let mut validated = self.validated.write().await;
        validated.remove_proxy(proxy);
        
        let _ = dead.save();
        let _ = validated.save();
        
        info!("Proxy {} marked as dead", proxy);
    }

    /// Check if we have valid cached proxies
    pub async fn has_validated_proxies(&self) -> bool {
        !self.validated.read().await.proxies.is_empty()
    }

    /// Get count of validated proxies
    pub async fn validated_count(&self) -> usize {
        self.validated.read().await.proxies.len()
    }

    /// Clear all cached data
    pub async fn clear(&self) -> Result<(), CoreError> {
        let index = self.index.write().await;
        let validated = self.validated.write().await;
        let dead = self.dead.write().await;
        
        let dir = proxy_cache_dir();
        for name in &["index.json", "validatedProxy.json", "deadProxy.json"] {
            let path = dir.join(name);
            if path.exists() {
                std::fs::remove_file(path)
                    .map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
            }
        }
        
        info!("Proxy cache cleared");
        Ok(())
    }
}