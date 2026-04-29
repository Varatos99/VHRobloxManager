//! Roblox REST API wrappers — avatar thumbnails, presence, place resolution.

use serde::Deserialize;

use crate::auth::RobloxClient;
use crate::error::CoreError;
use crate::models::{GameSearchResult, Presence};

// ---------------------------------------------------------------------------
// Avatar thumbnails
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ThumbnailResponse {
    data: Vec<ThumbnailEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThumbnailEntry {
    image_url: Option<String>,
}

/// Fetch avatar headshot thumbnail URLs for a batch of user IDs.
/// Returns a vec of `(user_id, url)` pairs.
pub async fn fetch_avatars(
    client: &RobloxClient,
    cookie: &str,
    user_ids: &[u64],
) -> Result<Vec<(u64, String)>, CoreError> {
    if user_ids.is_empty() {
        return Ok(vec![]);
    }
    let ids: Vec<String> = user_ids.iter().map(|id| id.to_string()).collect();
    let ids_param = ids.join(",");
    let url = format!(
        "https://thumbnails.roblox.com/v1/users/avatar-headshot\
         ?userIds={ids_param}&size=150x150&format=Png&isCircular=false"
    );

    let resp: ThumbnailResponse = client.get_json(&url, cookie).await?;

    Ok(user_ids
        .iter()
        .zip(resp.data.iter())
        .filter_map(|(id, entry)| entry.image_url.clone().map(|url| (*id, url)))
        .collect())
}

/// Download the actual image bytes for each avatar URL.
/// Returns a vec of `(user_id, raw_bytes)` pairs (skips failures).
pub async fn download_avatar_images(
    client: &RobloxClient,
    cookie: &str,
    avatars: &[(u64, String)],
) -> Vec<(u64, Vec<u8>)> {
    let mut results = Vec::new();
    for (id, url) in avatars {
        match client.get_bytes(url, cookie).await {
            Ok(bytes) => results.push((*id, bytes)),
            Err(e) => tracing::warn!("Failed to download avatar for {id}: {e}"),
        }
    }
    results
}

// ---------------------------------------------------------------------------
// Presence
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PresenceResponse {
    user_presences: Vec<PresenceEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PresenceEntry {
    user_presence_type: u8,
    place_id: Option<u64>,
    game_id: Option<String>,
    last_location: Option<String>,
    universe_id: Option<u64>,
}

/// Fetch presence info for multiple user IDs.
pub async fn fetch_presences(
    client: &RobloxClient,
    cookie: &str,
    user_ids: &[u64],
) -> Result<Vec<(u64, Presence)>, CoreError> {
    if user_ids.is_empty() {
        return Ok(vec![]);
    }
    let body = serde_json::json!({ "userIds": user_ids });
    let resp: PresenceResponse = client
        .post_json(
            "https://presence.roblox.com/v1/presence/users",
            cookie,
            Some(&body),
        )
        .await?;

    Ok(user_ids
        .iter()
        .zip(resp.user_presences.iter())
        .map(|(id, p)| {
            (
                *id,
                Presence {
                    user_presence_type: p.user_presence_type,
                    place_id: p.place_id,
                    game_id: p.game_id.clone(),
                    universe_id: p.universe_id,
                    last_location: p.last_location.clone().unwrap_or_default(),
                },
            )
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Place / Universe resolution
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UniverseDetails {
    name: String,
}

#[derive(Deserialize)]
struct UniverseResponse {
    data: Vec<UniverseDetails>,
}

/// Resolve a place ID to its game name. Requires authentication.
pub async fn resolve_place_name(
    client: &RobloxClient,
    cookie: &str,
    place_id: u64,
) -> Result<String, CoreError> {
    let url = format!("https://games.roblox.com/v1/games/{}", place_id);
    tracing::info!("[resolve_place_name] Fetching: {}", url);
    
    let text = client.get_text(&url, cookie).await?;
    tracing::debug!("[resolve_place_name] Response: {}", text.chars().take(500).collect::<String>());
    
    let resp: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
        CoreError::RobloxApi { status: 400, message: format!("parse error: {}", e) }
    })?;
    
    let name = resp
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CoreError::RobloxApi {
            status: 404,
            message: format!("place {place_id} not found"),
        })?;
    Ok(name.to_string())
}

/// Resolve a universe ID to its game name. Works unauthenticated.
pub async fn resolve_universe_name_simple(
    client: &RobloxClient,
    universe_id: u64,
) -> Result<String, CoreError> {
    let url = format!("https://games.roblox.com/v1/games?universeIds={}", universe_id);
    tracing::info!("[resolve_universe_name_simple] Fetching: {}", url);
    
    let text = client.get_text(&url, "").await?;
    tracing::debug!("[resolve_universe_name_simple] Response: {}", text.chars().take(500).collect::<String>());
    
    #[derive(Deserialize)]
    struct GameResponse {
        data: Vec<GameEntry>,
    }
    #[derive(Deserialize)]
    struct GameEntry {
        name: String,
    }
    
    let resp: GameResponse = serde_json::from_str(&text).map_err(|e| {
        CoreError::RobloxApi { status: 400, message: format!("parse error: {}", e) }
    })?;
    
    resp.data
        .into_iter()
        .next()
        .map(|g| g.name)
        .ok_or_else(|| CoreError::RobloxApi {
            status: 404,
            message: format!("universe {universe_id} not found"),
        })
}

/// Resolve a universe ID to its game name. Works unauthenticated.
pub async fn resolve_universe_name(
    client: &RobloxClient,
    universe_id: u64,
) -> Result<String, CoreError> {
    let url = format!("https://games.roblox.com/v1/games?universeIds={universe_id}");
    let resp: UniverseResponse = client.get_json(&url, "").await?;
    resp.data
        .into_iter()
        .next()
        .map(|d| d.name)
        .ok_or_else(|| CoreError::RobloxApi {
            status: 404,
            message: format!("universe {universe_id} not found"),
        })
}

/// Fetch game icon thumbnail URLs for a batch of universe IDs.
/// Returns a vec of `(universe_id, url)` pairs.
pub async fn fetch_game_icons(
    client: &RobloxClient,
    _cookie: &str,
    universe_ids: &[u64],
) -> Result<Vec<(u64, String)>, CoreError> {
    if universe_ids.is_empty() {
        return Ok(vec![]);
    }
    let ids: Vec<String> = universe_ids.iter().map(|id| id.to_string()).collect();
    let ids_param = ids.join(",");
    let url = format!(
        "https://thumbnails.roblox.com/v1/games/icons\
         ?universeIds={ids_param}&returnPolicy=PlaceHolder&size=150x150&format=Png&isCircular=false"
    );

    let resp: ThumbnailResponse = client.get_json(&url, "").await?;

    Ok(universe_ids
        .iter()
        .zip(resp.data.iter())
        .filter_map(|(id, entry)| entry.image_url.clone().map(|url| (*id, url)))
        .collect())
}

// ---------------------------------------------------------------------------
// Server list (for Job ID joining)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameServer {
    pub id: String,
    pub max_players: u32,
    pub playing: u32,
    pub fps: f32,
    pub ping: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServerListResponse {
    data: Vec<GameServer>,
    next_page_cursor: Option<String>,
}

/// Fetch one page of public servers for a place.
pub async fn fetch_servers(
    client: &RobloxClient,
    cookie: &str,
    place_id: u64,
    cursor: Option<&str>,
) -> Result<(Vec<GameServer>, Option<String>), CoreError> {
    let mut url = format!(
        "https://games.roblox.com/v1/games/{place_id}/servers/0?sortOrder=Asc&limit=25"
    );
    if let Some(c) = cursor {
        url.push_str(&format!("&cursor={c}"));
    }
    let resp: ServerListResponse = client.get_json(&url, cookie).await?;
    Ok((resp.data, resp.next_page_cursor))
}

// ---------------------------------------------------------------------------
// Share link resolution
// ---------------------------------------------------------------------------

/// Resolve a Roblox share link code (from `/share?code=CODE&type=Server`)
/// into `(place_id, universe_id, link_code, access_code)`.
///
/// Two-step process:
/// 1. POST `apis.roblox.com/sharelinks/v1/resolve-link` to get placeId + linkCode.
/// 2. GET `/games/{placeId}/game?privateServerLinkCode={linkCode}` and scrape
///    the UUID access code from the `joinPrivateGame(...)` JS call.
pub async fn resolve_share_link(
    client: &RobloxClient,
    cookie: &str,
    share_code: &str,
) -> Result<(u64, Option<u64>, String, String), CoreError> {
    use regex::Regex;

    // --- Step 1: Resolve share code → placeId + linkCode via API ---
    let body = serde_json::json!({
        "linkId": share_code,
        "linkType": "Server",
    });
    let resp: serde_json::Value = client
        .post_json(
            "https://apis.roblox.com/sharelinks/v1/resolve-link",
            cookie,
            Some(&body),
        )
        .await?;

    let ps_data = resp
        .get("privateServerInviteData")
        .ok_or_else(|| CoreError::RobloxApi {
            status: 400,
            message: "share link response missing privateServerInviteData".into(),
        })?;

    let place_id = ps_data
        .get("placeId")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CoreError::RobloxApi {
            status: 400,
            message: "share link response missing placeId".into(),
        })?;

    let link_code = ps_data
        .get("linkCode")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CoreError::RobloxApi {
            status: 400,
            message: "share link response missing linkCode".into(),
        })?
        .to_string();

    let universe_id = ps_data.get("universeId").and_then(|v| v.as_u64());

    let status = ps_data
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown");
    if status != "Valid" {
        return Err(CoreError::RobloxApi {
            status: 400,
            message: format!("private server invite status: {status}"),
        });
    }

    tracing::info!("Share link resolved → placeId={place_id}, linkCode={link_code}");

    // --- Step 2: Scrape accessCode (UUID) from the game page ---
    let game_url = format!(
        "https://www.roblox.com/games/{place_id}/game?privateServerLinkCode={link_code}"
    );
    let html = client.get_text(&game_url, cookie).await?;

    let access_re = Regex::new(
        r"Roblox\.GameLauncher\.joinPrivateGame\(\d+\s*,\s*'([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})'"
    ).expect("invalid regex");

    let access_code = access_re
        .captures(&html)
        .and_then(|cap| cap.get(1))
        .ok_or_else(|| CoreError::RobloxApi {
            status: 400,
            message: "could not scrape accessCode from game page".into(),
        })?
        .as_str()
        .to_string();

    tracing::info!("Access code resolved → {access_code}");

    Ok((place_id, universe_id, link_code, access_code))
}

// ---------------------------------------------------------------------------
// GitLab update check
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ReleaseLinks {
    #[serde(rename = "self")]
    self_url: String,
}

#[derive(Deserialize)]
struct GitLabRelease {
    tag_name: String,
    _links: ReleaseLinks,
}

// ---------------------------------------------------------------------------
// Friends API
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct FriendsPage {
    data: Vec<FriendEntry>,
    #[serde(default)]
    next_page_cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FriendEntry {
    id: i64,
    name: Option<String>,
    display_name: Option<String>,
}

/// Fetch friends list - gets ALL friend IDs and names
pub async fn fetch_friends(
    client: &RobloxClient,
    cookie: &str,
    user_id: u64,
) -> Result<Vec<(u64, String, String)>, CoreError> {
    tracing::info!("[fetch_friends] START user_id={}", user_id);
    
    let url = format!("https://friends.roblox.com/v1/users/{}/friends?limit=100", user_id);
    let text = client.get_text(&url, cookie).await?;
    tracing::debug!("[fetch_friends] Response: {}", text.chars().take(500).collect::<String>());
    
    let first_page: FriendsPage = serde_json::from_str(&text).map_err(|e| {
        tracing::warn!("[fetch_friends] Parse error: {}", e);
        CoreError::RobloxApi { status: 400, message: "parse error".into() }
    })?;
    
    let mut friend_ids: Vec<u64> = first_page.data.iter().filter(|f| f.id > 0).map(|f| f.id as u64).collect();
    
    // Fetch ALL pages
    let mut cursor = first_page.next_page_cursor.clone();
    while cursor.is_some() {
        let cursor_str = cursor.unwrap();
        let next_url = format!("https://friends.roblox.com/v1/users/{}/friends?limit=100&cursor={}", user_id, cursor_str);
        let next_text = match client.get_text(&next_url, cookie).await {
            Ok(t) => t,
            Err(_) => break,
        };
        let next_page: FriendsPage = match serde_json::from_str(&next_text) {
            Ok(p) => p,
            Err(_) => break,
        };
        for f in next_page.data {
            if f.id > 0 {
                friend_ids.push(f.id as u64);
            }
        }
        cursor = next_page.next_page_cursor;
    }
    
    tracing::info!("[fetch_friends] Got {} friend IDs", friend_ids.len());
    
    if friend_ids.is_empty() {
        return Ok(vec![]);
    }
    
    // Fetch ALL names
    let mut friends = vec![];
    for id in friend_ids.iter() {
        let user_url = format!("https://users.roblox.com/v1/users/{}", id);
        if let Ok(user_text) = client.get_text(&user_url, cookie).await {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&user_text) {
                let name = json.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let display = json.get("displayName").and_then(|v| v.as_str()).unwrap_or(name);
                if !name.is_empty() {
                    friends.push((*id, name.to_string(), display.to_string()));
                }
            }
        }
    }
    
    if friends.is_empty() {
        friends = friend_ids.iter().map(|id| (*id, format!("User_{}", id), format!("User_{}", id))).collect();
    }
    
    Ok(friends)
}

/// Fetch incoming friend requests
pub async fn fetch_incoming_requests(
    client: &RobloxClient,
    cookie: &str,
    user_id: u64,
) -> Result<Vec<(u64, String, String)>, CoreError> {
    tracing::info!("[fetch_incoming_requests] START user_id={}", user_id);
    
    let url = "https://friends.roblox.com/v1/my/new-friend-requests".to_string();
    let text = match client.get_text(&url, cookie).await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("[fetch_incoming_requests] failed: {} - returning empty", e);
            return Ok(vec![]);
        }
    };
    tracing::debug!("Incoming requests response: {}", text.chars().take(500).collect::<String>());
    
    #[derive(Deserialize)]
    struct RequestsResponse {
        data: Vec<RequestEntry>,
    }
    #[derive(Deserialize)]
    struct RequestEntry {
        id: u64,
        #[serde(rename = "senderId")]
        sender_id: u64,
    }
    
    let resp: RequestsResponse = serde_json::from_str(&text).map_err(|e| {
        tracing::warn!("[fetch_incoming_requests] Parse error: {}", e);
        CoreError::RobloxApi { status: 400, message: "parse error".into() }
    })?;
    
    let requests: Vec<(u64, String, String)> = resp.data.iter()
        .map(|r| (r.sender_id, format!("User_{}", r.sender_id), format!("User_{}", r.sender_id)))
        .collect();
    
    Ok(requests)
}

/// Accept a friend request
pub async fn accept_friend_request(client: &RobloxClient, cookie: &str, requester_id: u64) -> Result<(), CoreError> {
    let url = format!("https://friends.roblox.com/v1/users/{}/accept-friend-request", requester_id);
    client.request(reqwest::Method::POST, &url, cookie, None).await?;
    Ok(())
}

/// Decline a friend request  
pub async fn decline_friend_request(client: &RobloxClient, cookie: &str, requester_id: u64) -> Result<(), CoreError> {
    let url = format!("https://friends.roblox.com/v1/users/{}/decline-friend-request", requester_id);
    client.request(reqwest::Method::DELETE, &url, cookie, None).await?;
    Ok(())
}

/// Search users by username (placeholder - needs implementation)
pub async fn search_users(
    _client: &RobloxClient,
    _cookie: &str,
    _query: &str,
) -> Result<Vec<(u64, String, String)>, CoreError> {
    Ok(vec![])
}

// ---------------------------------------------------------------------------
// Game Search & Popular Games
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GameSearchResponse {
    data: Vec<GameSearchEntry>,
    #[serde(default)]
    paging: Option<PagingInfo>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PagingInfo {
    cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GameSearchEntry {
    id: u64,
    name: String,
    description: Option<String>,
    root_place_id: Option<u64>,
    thumbnail: Option<GameThumbnail>,
    #[serde(default)]
    universe_id: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GameThumbnail {
    final_url: Option<String>,
}

use uuid::Uuid;

/// Search games by query string using the new omni-search API
pub async fn search_games(
    client: &RobloxClient,
    _cookie: &str,
    query: &str,
    limit: u32,
) -> Result<Vec<GameSearchResult>, CoreError> {
    if query.trim().is_empty() {
        return Ok(vec![]);
    }
    
    let session_id = Uuid::new_v4().to_string();
    let url = format!(
        "https://apis.roblox.com/search-api/omni-search?searchQuery={}&sessionId={}&pageType=all",
        urlencoding::encode(query),
        session_id
    );
    tracing::info!("[search_games] Fetching: {}", url);
    
    let text = client.get_text(&url, _cookie).await?;
    tracing::debug!("[search_games] Response: {}", text.chars().take(1000).collect::<String>());
    
    #[derive(Deserialize)]
    struct OmniSearchResponse {
        searchResults: Vec<SearchResultGroup>,
    }
    #[derive(Deserialize)]
    struct SearchResultGroup {
        contents: Vec<GameContent>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct GameContent {
        universe_id: u64,
        name: String,
        description: String,
        player_count: Option<u64>,
        root_place_id: u64,
    }
    
    let resp: OmniSearchResponse = serde_json::from_str(&text).map_err(|e| CoreError::RobloxApi { status: 400, message: format!("parse error: {}", e) })?;
    
    let results: Vec<GameSearchResult> = resp.searchResults
        .into_iter()
        .flat_map(|group| group.contents)
        .map(|g| GameSearchResult {
            place_id: g.root_place_id,
            name: g.name,
            description: g.description,
            root_place_id: g.root_place_id,
            thumbnail_url: String::new(),
            universe_id: Some(g.universe_id),
            playing: 0,
            visits: 0,
            max_players: 0,
            create_vip_servers_allowed: false,
            vip_server_price: 0,
        })
        .take(limit as usize)
        .collect();
    
    tracing::info!("[search_games] Got {} results", results.len());
    Ok(results)
}

/// Get popular/trending games using the search API with default sort
pub async fn get_popular_games(
    client: &RobloxClient,
    _cookie: &str,
    limit: u32,
) -> Result<Vec<GameSearchResult>, CoreError> {
    // Use a generic query to get popular games
    let session_id = Uuid::new_v4().to_string();
    let url = format!(
        "https://apis.roblox.com/search-api/omni-search?searchQuery=game&sessionId={}&pageType=all",
        session_id
    );
    tracing::info!("[get_popular_games] Fetching: {}", url);
    
    let text = client.get_text(&url, _cookie).await?;
    tracing::debug!("[get_popular_games] Response: {}", text.chars().take(1000).collect::<String>());
    
    #[derive(Deserialize)]
    struct OmniSearchResponse {
        searchResults: Vec<SearchResultGroup>,
    }
    #[derive(Deserialize)]
    struct SearchResultGroup {
        contents: Vec<GameContent>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct GameContent {
        universe_id: u64,
        name: String,
        description: String,
        player_count: Option<u64>,
        root_place_id: u64,
    }
    
    let resp: OmniSearchResponse = serde_json::from_str(&text).map_err(|e| CoreError::RobloxApi { status: 400, message: format!("parse error: {}", e) })?;
    
    let results: Vec<GameSearchResult> = resp.searchResults
        .into_iter()
        .flat_map(|group| group.contents)
        .map(|g| GameSearchResult {
            place_id: g.root_place_id,
            name: g.name,
            description: g.description,
            root_place_id: g.root_place_id,
            thumbnail_url: String::new(),
            universe_id: Some(g.universe_id),
            playing: g.player_count.unwrap_or(0),
            visits: 0,
            max_players: 0,
            create_vip_servers_allowed: false,
            vip_server_price: 0,
        })
        .take(limit as usize)
        .collect();
    
    tracing::info!("[get_popular_games] Got {} results", results.len());
    Ok(results)
}

/// Get user's favorite games
pub async fn get_favorite_games(
    client: &RobloxClient,
    cookie: &str,
    user_id: u64,
) -> Result<Vec<GameSearchResult>, CoreError> {
    let url = format!("https://games.roblox.com/v2/users/{}/favorite/games", user_id);
    tracing::info!("[get_favorite_games] Fetching: {}", url);
    
    let text = client.get_text(&url, cookie).await?;
    tracing::info!("[get_favorite_games] Response: {}", text.chars().take(2000).collect::<String>());
    
    #[derive(Deserialize)]
    struct FavoriteResponse {
        data: Vec<FavoriteGameEntry>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FavoriteGameEntry {
        #[serde(default)]
        id: Option<u64>,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        root_place: Option<RootPlace>,
    }
    #[derive(Deserialize)]
    struct RootPlace {
        id: u64,
    }
    
    let resp: FavoriteResponse = serde_json::from_str(&text).map_err(|e| {
        CoreError::RobloxApi { status: 400, message: format!("parse error: {}", e) }
    })?;
    
    if resp.data.is_empty() {
        return Ok(vec![]);
    }
    
tracing::info!("[get_favorite_games] Got {} favorites", resp.data.len());
    
    let mut results: Vec<GameSearchResult> = vec![];
    for g in resp.data {
        let root_place_id = g.root_place.map(|r| r.id).unwrap_or(g.id.unwrap_or(0));
        if root_place_id == 0 {
            continue;
        }
        results.push(GameSearchResult {
            place_id: root_place_id,
            name: g.name.unwrap_or_else(|| "Unknown Game".to_string()),
            description: g.description.unwrap_or_default(),
            root_place_id,
            thumbnail_url: String::new(),
            universe_id: g.id,
            playing: 0,
            visits: 0,
            max_players: 0,
            create_vip_servers_allowed: false,
            vip_server_price: 0,
        });
    }

tracing::info!("[get_favorite_games] Got {} favorites with place IDs", results.len());
    
    // Skip stats fetching for now - too slow, causing timeouts
    // Stats can be fetched on-demand when needed
    
    Ok(results)
}

// ---------------------------------------------------------------------------
// Private Servers API
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VipServer {
    pub vip_server_id: u64,
    pub access_code: String,
    pub name: String,
    pub active: bool,
}

#[derive(Debug, Deserialize)]
struct VipServerCreateResponse {
    #[serde(rename = "vipServerId")]
    vip_server_id: u64,
    #[serde(rename = "accessCode")]
    access_code: String,
    name: String,
    #[serde(default)]
    active: bool,
}

/// Create a VIP (private) server for a universe
pub async fn create_vip_server(
    client: &RobloxClient,
    cookie: &str,
    universe_id: u64,
    name: &str,
) -> Result<VipServer, CoreError> {
    let url = format!("https://games.roblox.com/v1/games/vip-servers/{}", universe_id);
    tracing::info!("[create_vip_server] Creating VIP server for universe {}", universe_id);
    
    let body = serde_json::json!({ "name": name, "expectedPrice": 0 });
    tracing::info!("[create_vip_server] Request body: {}", body);
    let resp: VipServerCreateResponse = client
        .post_json(&url, cookie, Some(&body))
        .await?;
    
    tracing::info!("[create_vip_server] Created VIP server: id={}, code={}", resp.vip_server_id, resp.access_code);
    
    Ok(VipServer {
        vip_server_id: resp.vip_server_id,
        access_code: resp.access_code,
        name: resp.name,
        active: resp.active,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivateServer {
    pub id: u64,
    pub name: String,
    pub access_code: String,
    pub active: bool,
    pub owner: Option<PrivateServerOwner>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivateServerOwner {
    pub id: u64,
    pub name: String,
    pub display_name: Option<String>,
}

#[derive(Deserialize)]
struct PrivateServersResponse {
    data: Vec<PrivateServerEntry>,
    #[serde(default)]
    next_page_cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrivateServerEntry {
    #[serde(default)]
    id: Option<u64>,
    name: Option<String>,
    #[serde(default)]
    access_code: Option<String>,
    #[serde(default)]
    active: bool,
    owner: Option<OwnerEntry>,
    #[serde(rename = "vipServerId", default)]
    vip_server_id: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OwnerEntry {
    id: u64,
    name: String,
    display_name: Option<String>,
}

/// List private servers for a place
pub async fn list_private_servers(
    client: &RobloxClient,
    cookie: &str,
    place_id: u64,
    cursor: Option<&str>,
) -> Result<(Vec<PrivateServer>, Option<String>), CoreError> {
    let mut url = format!(
        "https://games.roblox.com/v1/games/{}/private-servers?limit=25",
        place_id
    );
    if let Some(c) = cursor {
        url.push_str(&format!("&cursor={}", c));
    }
    tracing::info!("[list_private_servers] Fetching: {}", url);
    
    let text = client.get_text(&url, cookie).await?;
    tracing::info!("[list_private_servers] Response: {}", text.chars().take(2000).collect::<String>());
    
    let resp: PrivateServersResponse = serde_json::from_str(&text).map_err(|e| {
        CoreError::RobloxApi { status: 400, message: format!("parse error: {}", e) }
    })?;
    
    let servers: Vec<PrivateServer> = resp.data.into_iter().map(|e| PrivateServer {
        id: e.vip_server_id,
        name: e.name.unwrap_or_else(|| "Private Server".to_string()),
        access_code: e.access_code.unwrap_or_default(),
        active: e.active,
        owner: e.owner.map(|o| PrivateServerOwner {
            id: o.id,
            name: o.name,
            display_name: o.display_name,
        }),
    }).collect();
    
    tracing::info!("[list_private_servers] Got {} servers", servers.len());
    Ok((servers, resp.next_page_cursor))
}

/// Game statistics from the games API
pub struct GameStats {
    pub playing: u64,
    pub visits: u64,
    pub max_players: u64,
    pub create_vip_servers_allowed: bool,
    pub vip_server_price: u64,
}

/// Get game statistics for a universe
pub async fn get_game_stats(
    client: &RobloxClient,
    _cookie: &str,
    universe_id: u64,
) -> Result<GameStats, CoreError> {
    let url = format!(
        "https://games.roblox.com/v1/games?universeIds={}",
        universe_id
    );
    tracing::debug!("[get_game_stats] Fetching stats for universe {}", universe_id);
    
    let text = client.get_text(&url, _cookie).await?;
    
    #[derive(Deserialize)]
    struct GameDetailsResponse {
        data: Vec<GameData>,
    }
    #[derive(Deserialize)]
    struct GameData {
        playing: Option<u64>,
        visits: Option<u64>,
        max_players: Option<u64>,
        #[serde(rename = "createVipServersAllowed", default)]
        create_vip_servers_allowed: bool,
        #[serde(rename = "price")]
        vip_server_price: Option<u64>,
    }
    
    let resp: GameDetailsResponse = serde_json::from_str(&text).map_err(|e| {
        CoreError::RobloxApi { status: 400, message: format!("parse error: {}", e) }
    })?;
    
    if let Some(game) = resp.data.into_iter().next() {
        Ok(GameStats {
            playing: game.playing.unwrap_or(0),
            visits: game.visits.unwrap_or(0),
            max_players: game.max_players.unwrap_or(0),
            create_vip_servers_allowed: game.create_vip_servers_allowed,
            vip_server_price: game.vip_server_price.unwrap_or(0),
        })
    } else {
        Ok(GameStats {
            playing: 0,
            visits: 0,
            max_players: 0,
            create_vip_servers_allowed: false,
            vip_server_price: 0,
        })
    }
}

/// Check if private servers are enabled for a universe (uses games.roblox.com)
pub async fn check_private_server_enabled(
    client: &RobloxClient,
    cookie: &str,
    universe_id: u64,
) -> Result<(bool, u64), CoreError> {
    let url = format!(
        "https://games.roblox.com/v1/games?universeIds={}",
        universe_id
    );
    tracing::info!("[check_private_server_enabled] Checking universe {}", universe_id);
    
    let text = client.get_text(&url, cookie).await?;
    tracing::info!("[check_private_server_enabled] Response: {}", text.chars().take(1000).collect::<String>());
    
    #[derive(Deserialize)]
    struct GameDetailsResponse {
        data: Vec<GameData>,
    }
    #[derive(Deserialize)]
    struct GameData {
        #[serde(rename = "createVipServersAllowed", default)]
        create_vip_servers_allowed: bool,
        #[serde(default)]
        price: Option<u64>,
    }
    
    let resp: GameDetailsResponse = serde_json::from_str(&text).map_err(|e| {
        CoreError::RobloxApi { status: 400, message: format!("parse error: {}", e) }
    })?;
    
    if let Some(game) = resp.data.into_iter().next() {
        Ok((game.create_vip_servers_allowed, game.price.unwrap_or(0)))
    } else {
        Ok((false, 0))
    }
}

/// Get VIP server details - includes subscription price
#[derive(Debug, Deserialize)]
pub struct VipServerDetail {
    pub id: u64,
    pub name: String,
    #[serde(rename = "joinCode")]
    join_code: String,
    pub active: bool,
    #[serde(rename = "subscription")]
    subscription: VipServerSubscription,
}

#[derive(Debug, Deserialize)]
pub struct VipServerSubscription {
    pub active: bool,
    pub expired: bool,
    #[serde(rename = "expirationDate")]
    expiration_date: Option<String>,
    pub price: u64,
    #[serde(rename = "canRenew")]
    can_renew: bool,
    #[serde(rename = "hasInsufficientFunds")]
    has_insufficient_funds: bool,
}

pub async fn get_vip_server_detail(
    client: &RobloxClient,
    cookie: &str,
    vip_server_id: u64,
) -> Result<VipServerDetail, CoreError> {
    let url = format!("https://games.roblox.com/v1/vip-servers/{}", vip_server_id);
    tracing::info!("[get_vip_server_detail] Fetching: {}", url);
    
    let resp: VipServerDetail = client.get_json(&url, cookie).await?;
    
    tracing::info!(
        "[get_vip_server_detail] Server {}: price={}, active={}",
        vip_server_id, resp.subscription.price, resp.active
    );
    
    Ok(resp)
}

#[derive(Debug, Deserialize)]
struct MyPrivateServersResponse {
    data: Vec<MyPrivateServerEntry>,
    #[serde(default)]
    next_page_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MyPrivateServerEntry {
    active: bool,
    #[serde(rename = "universeId", default)]
    universe_id: u64,
    #[serde(rename = "placeId", default)]
    place_id: u64,
    name: String,
    #[serde(rename = "ownerId", default)]
    owner_id: u64,
    #[serde(rename = "ownerName", default)]
    owner_name: String,
    #[serde(rename = "priceInRobux", default)]
    price_in_robux: Option<u64>,
}

pub enum VipPriceResult {
    Price(u64),
    Disabled,
    Unknown,
}

pub async fn get_vip_server_price(
    client: &RobloxClient,
    cookie: &str,
    target_universe_id: u64,
    target_place_id: u64,
) -> Result<VipPriceResult, CoreError> {
    let mut all_servers: Vec<MyPrivateServerEntry> = Vec::new();
    let mut cursor: Option<String> = None;
    
    loop {
        let url = if let Some(ref c) = cursor {
            format!("https://games.roblox.com/v1/private-servers/my-private-servers?limit=100&cursor={}", c)
        } else {
            "https://games.roblox.com/v1/private-servers/my-private-servers?limit=100".to_string()
        };
        
        tracing::info!("[get_vip_server_price] Fetching: {}", url);
        
        let resp: MyPrivateServersResponse = client.get_json(&url, cookie).await?;
        all_servers.extend(resp.data);
        
        if let Some(next) = resp.next_page_cursor {
            cursor = Some(next);
        } else {
            break;
        }
    }
    
    tracing::info!("[get_vip_server_price] Total servers fetched: {}", all_servers.len());
    
    for server in all_servers.iter() {
        if server.universe_id == target_universe_id {
            if let Some(price) = server.price_in_robux {
                tracing::info!("[get_vip_server_price] Found matching server '{}' for universe {} with price {}", 
                    server.name, target_universe_id, price);
                return Ok(VipPriceResult::Price(price));
            }
        }
        if server.place_id == target_place_id {
            if let Some(price) = server.price_in_robux {
                tracing::info!("[get_vip_server_price] Found matching server '{}' for place {} with price {}", 
                    server.name, target_place_id, price);
                return Ok(VipPriceResult::Price(price));
            }
        }
    }
    
    tracing::info!("[get_vip_server_price] No server found for universe {} or place {}, checking multiget-place-details", target_universe_id, target_place_id);
    
    let multiget_url = format!("https://games.roblox.com/v1/games/multiget-place-details?placeIds={}", target_place_id);
    match client.get_text(&multiget_url, cookie).await {
        Ok(text) => {
            tracing::info!("[get_vip_server_price] multiget-place-details response for place {}: {}", target_place_id, text.chars().take(5000).collect::<String>());
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                let items = if let Some(arr) = json.as_array() {
                    arr.clone()
                } else if let Some(data) = json.get("data").and_then(|v| v.as_array()) {
                    data.clone()
                } else {
                    tracing::warn!("[get_vip_server_price] Unexpected JSON format");
                    return Ok(VipPriceResult::Unknown);
                };
                
                if let Some(first) = items.first() {
                    if let Some(ps_info) = first.get("privateServerInfo") {
                        let is_enabled = ps_info.get("isEnabled").and_then(|v| v.as_bool()).unwrap_or(false);
                        let price = ps_info.get("price").and_then(|v| v.as_u64()).unwrap_or(0);
                        tracing::info!("[get_vip_server_price] multiget-place-details: enabled={}, price={}", is_enabled, price);
                        if !is_enabled {
                            return Ok(VipPriceResult::Disabled);
                        }
                        return Ok(VipPriceResult::Price(price));
                    }
                    tracing::info!("[get_vip_server_price] No privateServerInfo field - VIP may not be supported");
                    return Ok(VipPriceResult::Disabled);
                }
            }
            tracing::info!("[get_vip_server_price] No price info in multiget-place-details response");
        }
        Err(e) => {
            tracing::warn!("[get_vip_server_price] multiget-place-details failed: {}", e);
        }
    }
    
    tracing::info!("[get_vip_server_price] Price unknown for this game");
    
    Ok(VipPriceResult::Unknown)
}

/// Get VIP server details by ID
pub async fn get_vip_server(
    client: &RobloxClient,
    cookie: &str,
    vip_server_id: u64,
) -> Result<VipServer, CoreError> {
    let url = format!("https://games.roblox.com/v1/vip-servers/{}", vip_server_id);
    tracing::info!("[get_vip_server] Fetching VIP server {}", vip_server_id);
    
    #[derive(Deserialize)]
    struct VipServerDetailResponse {
        #[serde(rename = "vipServerId")]
        vip_server_id: u64,
        #[serde(rename = "accessCode")]
        access_code: String,
        name: String,
        active: bool,
    }
    
    let resp: VipServerDetailResponse = client.get_json(&url, cookie).await?;
    
    Ok(VipServer {
        vip_server_id: resp.vip_server_id,
        access_code: resp.access_code,
        name: resp.name,
        active: resp.active,
})
}

/// Fetch user's Robux balance and premium status.
pub async fn fetch_currency(
    client: &RobloxClient,
    cookie: &str,
) -> Result<(u64, bool), CoreError> {
    #[derive(Deserialize)]
    struct CurrencyResponse {
        #[serde(rename = "robux", default)]
        robux: u64,
    }
    
    tracing::debug!("Fetching currency from economy.roblox.com...");
    
    let resp: CurrencyResponse = client
        .get_json("https://economy.roblox.com/v1/user/currency", cookie)
        .await?;
    
    Ok((resp.robux, false))
}

/// Fetch groups where user has a role (including owner/admin)
pub async fn fetch_user_groups(
    client: &RobloxClient,
    cookie: &str,
    user_id: u64,
) -> Result<Vec<GroupRole>, CoreError> {
    #[derive(Deserialize)]
    struct GroupResponse {
        data: Vec<GroupRoleResponse>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct GroupRoleResponse {
        group: GroupInfo,
        role: RoleInfo,
    }
    #[derive(Deserialize)]
    struct GroupInfo {
        id: u64,
        name: String,
    }
    #[derive(Deserialize)]
    struct RoleInfo {
        name: String,
        rank: u32,
    }
    
    let resp: GroupResponse = client
        .get_json(&format!("https://groups.roblox.com/v1/users/{user_id}/groups/roles"), cookie)
        .await?;
    
    Ok(resp.data.into_iter().map(|r| GroupRole {
        group_id: r.group.id,
        group_name: r.group.name,
        role_name: r.role.name,
        rank: r.role.rank,
    }).collect())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GroupRole {
    pub group_id: u64,
    pub group_name: String,
    pub role_name: String,
    pub rank: u32,
}

/// Fetch group Robux balance.
/// Returns `Ok(Some(GroupCurrency))` on success, `Ok(None)` if the account lacks permission.
pub async fn fetch_group_currency(
    client: &RobloxClient,
    cookie: &str,
    group_id: u64,
) -> Result<Option<GroupCurrency>, CoreError> {
    #[derive(Deserialize)]
    struct GroupRevenue {
        #[serde(rename = "robuxBalance", default)]
        robux_balance: u64,
        #[serde(rename = "robux", default)]
        robux: u64,
        #[serde(rename = "pendingRobuxValue", default)]
        pending_robux: u64,
    }
    
    let url = format!("https://economy.roblox.com/v1/groups/{group_id}/currency");
    
    tracing::debug!("Fetching group currency for group {}: {}", group_id, url);
    let resp: Result<GroupRevenue, _> = client.get_json(&url, cookie).await;
    
    match resp {
        Ok(r) => {
            // Try robuxBalance first, fall back to robux
            let actual_robux = if r.robux_balance > 0 { r.robux_balance } else { r.robux };
            tracing::info!("Group {} has {} Robux (pending: {}, raw: robuxBalance={}, robux={})", 
                group_id, actual_robux, r.pending_robux, r.robux_balance, r.robux);
            Ok(Some(GroupCurrency {
                group_id,
                robux_balance: actual_robux,
                pending_robux: r.pending_robux,
            }))
        }
        Err(e) => {
            tracing::warn!("Failed to fetch currency for group {}: {:?}", group_id, e);
            Ok(None)
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GroupCurrency {
    pub group_id: u64,
    pub robux_balance: u64,
    pub pending_robux: u64,
}

/// Check for a newer release on GitLab. Returns `Some((version, url))` if an
/// update is available, `None` if already on the latest.
pub async fn check_for_updates(current_version: &str) -> Result<Option<(String, String)>, CoreError> {
    let client = reqwest::Client::builder()
        .user_agent("VHRobloxManager-update-check")
        .build()?;

    let release: GitLabRelease = client
        .get("https://gitlab.com/api/v4/projects/centerepic%2Frobloxmanager/releases/permalink/latest")
        .send()
        .await?
        .json()
        .await?;

    let remote = release.tag_name.trim_start_matches('v');
    let local = current_version.trim_start_matches('v');

    if remote != local {
        Ok(Some((remote.to_string(), release._links.self_url)))
    } else {
        Ok(None)
    }
}
