//! Roblox authentication — CSRF token management & auth ticket generation.
//!
//! The [`RobloxClient`] wraps a `reqwest::Client` and transparently handles
//! CSRF token rotation: if a request returns `403` with a new token in the
//! `x-csrf-token` header, the client updates its state and retries once.
//! Exponential backoff is applied for `429 Too Many Requests`.

use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, COOKIE, REFERER};
use reqwest::{Client, Method, Response, StatusCode};
use serde::de::DeserializeOwned;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, warn};

use crate::error::CoreError;

const USER_AGENT: &str = "VHRobloxManager-Rust/0.1";
const MAX_RETRIES: u32 = 4;
const BASE_BACKOFF_MS: u64 = 500;

/// A stateful HTTP client that manages `.ROBLOSECURITY` cookies and CSRF tokens.
#[derive(Clone)]
pub struct RobloxClient {
    inner: Client,
    /// Current CSRF token (shared across clones via Arc<RwLock>).
    csrf_token: Arc<RwLock<Option<String>>>,
}

impl RobloxClient {
    /// Create a new client. Does NOT set a cookie yet — call [`set_cookie`] before
    /// making authenticated requests.
    pub fn new() -> Result<Self, CoreError> {
        let client = Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self {
            inner: client,
            csrf_token: Arc::new(RwLock::new(None)),
        })
    }

    // ------------------------------------------------------------------
    // Core request helpers
    // ------------------------------------------------------------------

    /// Low-level request with automatic CSRF retry + exponential backoff.
    pub async fn request(
        &self,
        method: Method,
        url: &str,
        cookie: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<Response, CoreError> {
        let mut attempt = 0u32;

        loop {
            let mut headers = HeaderMap::new();
            // Attach cookie only if not empty
            if !cookie.is_empty() {
                let cookie_val = format!(".ROBLOSECURITY={cookie}");
                headers.insert(
                    COOKIE,
                    HeaderValue::from_str(&cookie_val)
                        .map_err(|e| CoreError::AuthFailed(e.to_string()))?,
                );
            }

            // Attach CSRF token if we have one (lowercase as Roblox expects)
            {
                let token = self.csrf_token.read().await;
                if let Some(ref t) = *token {
                    headers.insert(
                        "x-csrf-token",
                        HeaderValue::from_str(t)
                            .map_err(|e| CoreError::AuthFailed(e.to_string()))?,
                    );
                }
            }

            // Add Referer for all requests (required by Roblox API)
            headers.insert(
                REFERER,
                HeaderValue::from_static("https://www.roblox.com"),
            );
            
            // Add User-Agent for better compatibility
            headers.insert(
                reqwest::header::USER_AGENT,
                HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"),
            );

            let mut req = self.inner.request(method.clone(), url).headers(headers);
            if body.is_some() {
                req = req.json(body.unwrap());
            } else if method == Method::POST {
                // Roblox POST endpoints require application/json even with no body
                req = req
                    .header(CONTENT_TYPE, "application/json");
            }

            let resp = req.send().await?;

            match resp.status() {
                // Token rotation: update and retry once
                StatusCode::FORBIDDEN => {
                    if let Some(new_token) = resp
                        .headers()
                        .get("x-csrf-token")
                        .and_then(|v| v.to_str().ok())
                    {
                        debug!("CSRF token rotated, retrying");
                        let mut token = self.csrf_token.write().await;
                        *token = Some(new_token.to_string());
                        if attempt == 0 {
                            attempt += 1;
                            continue;
                        }
                    }
                    // Debug: capture response body for 403 errors
                    let body = resp.text().await.unwrap_or_default();
                    debug!("403 Response body: {}", body.chars().take(500).collect::<String>());
                    return Err(CoreError::AuthFailed(
                        format!("403 Forbidden after CSRF retry: {}", body.chars().take(200).collect::<String>())
                    ));
                }
                // Rate-limit: exponential backoff
                StatusCode::TOO_MANY_REQUESTS => {
                    if attempt >= MAX_RETRIES {
                        return Err(CoreError::RateLimited);
                    }
                    let wait = Duration::from_millis(BASE_BACKOFF_MS * 2u64.pow(attempt));
                    warn!("Rate limited, backing off {wait:?} (attempt {attempt})");
                    tokio::time::sleep(wait).await;
                    attempt += 1;
                    continue;
                }
                _ => return Ok(resp),
            }
        }
    }

    /// Perform a GET and return raw response bytes.
    pub async fn get_bytes(
        &self,
        url: &str,
        cookie: &str,
    ) -> Result<Vec<u8>, CoreError> {
        let resp = self.request(Method::GET, url, cookie, None).await?;
        let status = resp.status();
        if !status.is_success() {
            let msg = resp.text().await.unwrap_or_default();
            return Err(CoreError::RobloxApi {
                status: status.as_u16(),
                message: msg,
            });
        }
        let bytes = resp.bytes().await?;
        Ok(bytes.to_vec())
    }

    /// Convenience: perform a GET and return the response body as a string.
    pub async fn get_text(
        &self,
        url: &str,
        cookie: &str,
    ) -> Result<String, CoreError> {
        let resp = self.request(Method::GET, url, cookie, None).await?;
        let status = resp.status();
        if !status.is_success() {
            let msg = resp.text().await.unwrap_or_default();
            return Err(CoreError::RobloxApi {
                status: status.as_u16(),
                message: msg,
            });
        }
        let text = resp.text().await?;
        Ok(text)
    }

    /// Convenience: perform a GET and deserialize JSON.
    pub async fn get_json<T: DeserializeOwned>(
        &self,
        url: &str,
        cookie: &str,
    ) -> Result<T, CoreError> {
        let resp = self.request(Method::GET, url, cookie, None).await?;
        let status = resp.status();
        if !status.is_success() {
            let msg = resp.text().await.unwrap_or_default();
            return Err(CoreError::RobloxApi {
                status: status.as_u16(),
                message: msg,
            });
        }
        // First get text, then try to parse - capture response for error reporting
        let text = resp.text().await?;
        match serde_json::from_str::<T>(&text) {
            Ok(data) => Ok(data),
            Err(e) => {
                let preview: String = text.chars().take(200).collect();
                Err(CoreError::RobloxApi {
                    status: 0,
                    message: format!("JSON decode error: {e}. Preview: {preview}"),
                })
            }
        }
    }

    /// Convenience: perform a POST and deserialize JSON.
    pub async fn post_json<T: DeserializeOwned>(
        &self,
        url: &str,
        cookie: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<T, CoreError> {
        let resp = self.request(Method::POST, url, cookie, body).await?;
        let status = resp.status();
        if !status.is_success() {
            let msg = resp.text().await.unwrap_or_default();
            return Err(CoreError::RobloxApi {
                status: status.as_u16(),
                message: msg,
            });
        }
        // First get text, then try to parse - capture response for error reporting
        let text = resp.text().await?;
        match serde_json::from_str::<T>(&text) {
            Ok(data) => Ok(data),
            Err(e) => {
                let preview: String = text.chars().take(200).collect();
                Err(CoreError::RobloxApi {
                    status: 0,
                    message: format!("JSON decode error: {e}. Preview: {preview}"),
                })
            }
        }
    }

    // ------------------------------------------------------------------
    // Auth-ticket generation
    // ------------------------------------------------------------------

    /// Request an authentication ticket from Roblox for game launch.
    /// Returns the ticket string on success.
    pub async fn generate_auth_ticket(&self, cookie: &str) -> Result<String, CoreError> {
        let resp = self
            .request(
                Method::POST,
                "https://auth.roblox.com/v1/authentication-ticket",
                cookie,
                None,
            )
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let msg = resp.text().await.unwrap_or_default();
            return Err(CoreError::AuthFailed(format!(
                "ticket request failed ({status}): {msg}"
            )));
        }

        resp.headers()
            .get("rbx-authentication-ticket")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .ok_or(CoreError::AuthFailed(
                "no rbx-authentication-ticket header in response".into(),
            ))
    }

    // ------------------------------------------------------------------
    // Validation
    // ------------------------------------------------------------------

    /// Validate a cookie by fetching the authenticated user info.
    /// Returns `(user_id, username, display_name)` on success.
    pub async fn validate_cookie(
        &self,
        cookie: &str,
    ) -> Result<(u64, String, String), CoreError> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct AuthUser {
            id: u64,
            name: String,
            display_name: String,
        }
        let user: AuthUser = self
            .get_json("https://users.roblox.com/v1/users/authenticated", cookie)
            .await?;
        Ok((user.id, user.name, user.display_name))
    }
}

impl Default for RobloxClient {
    fn default() -> Self {
        Self::new().expect("failed to build reqwest client")
    }
}
