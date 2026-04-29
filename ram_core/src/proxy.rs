//! Proxy rotation for bulk operations.
//! Creates HTTP clients that rotate through a list of proxies.

use reqwest::{Client, Proxy};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use crate::error::CoreError;

#[derive(Clone)]
pub struct ProxyPool {
    proxies: Arc<RwLock<Vec<String>>>,
    current_index: Arc<RwLock<usize>>,
}

impl ProxyPool {
    pub fn new(proxies: Vec<String>) -> Self {
        Self {
            proxies: Arc::new(RwLock::new(proxies)),
            current_index: Arc::new(RwLock::new(0)),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.proxies.try_read().map(|p| p.is_empty()).unwrap_or(true)
    }

    pub fn get_current_proxy(&self, index: usize) -> Option<String> {
        self.proxies.try_read().ok().and_then(|p| p.get(index % p.len()).cloned())
    }

    pub async fn create_client_for_next_proxy(&self) -> Result<Client, CoreError> {
        let proxies = self.proxies.read().await;
        
        if proxies.is_empty() {
            return Err(CoreError::InvalidProxy("No proxies loaded".to_string()));
        }
        
        let proxy = {
            let mut index_guard = self.current_index.write().await;
            let p = proxies[*index_guard].clone();
            *index_guard = (*index_guard + 1) % proxies.len();
            p
        };
        
        let proxy_url = if proxy.starts_with("http://") || proxy.starts_with("https://") {
            proxy
        } else {
            format!("http://{}", proxy)
        };
        
        let client = Client::builder()
            .proxy(Proxy::all(&proxy_url).map_err(|e| CoreError::InvalidProxy(e.to_string()))?)
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
            
        Ok(client)
    }

    pub async fn validate_cookie_with_proxy(&self, cookie: &str) -> Result<(u64, String, String), CoreError> {
        let client = self.create_client_for_next_proxy().await?;
        
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct AuthUser {
            id: u64,
            name: String,
            display_name: String,
        }
        
        let url = "https://users.roblox.com/v1/users/authenticated";
        
        let resp = client
            .get(url)
            .header("Cookie", format!(".ROBLOSECURITY={cookie}"))
            .send()
            .await?;
        
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(CoreError::AuthFailed(format!("Auth failed ({}): {}", status, body)));
        }
        
        let user: AuthUser = resp.json().await?;
        Ok((user.id, user.name, user.display_name))
    }
}

/// Read proxies from a file - one per line
/// Supports formats:
///   http://proxy:port
///   http://user:pass@proxy:port
///   proxy:port (auto http:// prefix)
pub fn load_proxies_from_file(path: &str) -> Result<Vec<String>, CoreError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| CoreError::InvalidProxy(format!("Failed to read file: {}", e)))?;
    
    let proxies: Vec<String> = content
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    
    if proxies.is_empty() {
        return Err(CoreError::InvalidProxy("No valid proxies found in file".to_string()));
    }
    
    Ok(proxies)
}

const TEST_HOSTS: &[&str] = &[
    "https://www.google.com",
    "https://api.ipify.org",
    "https://api.my-ip.io/v1/ip",
];

pub async fn test_proxy(proxy: &str) -> Result<bool, CoreError> {
    let proxy_url = if proxy.starts_with("http://") || proxy.starts_with("https://") {
        proxy.to_string()
    } else {
        format!("http://{}", proxy)
    };
    
    let client = Client::builder()
        .proxy(Proxy::all(&proxy_url).map_err(|e| CoreError::InvalidProxy(e.to_string()))?)
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| CoreError::InvalidProxy(e.to_string()))?;
    
    for host in TEST_HOSTS {
        match client.get(*host).send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if status == 200 || status == 204 || status == 301 || status == 302 {
                    return Ok(true);
                }
            }
            Err(_) => continue,
        }
    }
    
    Ok(false)
}