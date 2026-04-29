use serde::Deserialize;
use uuid::Uuid;

use crate::models::GameSearchResult;
use crate::{CoreError, RobloxClient};

/// Search games by query string using the new omni-search API
pub async fn search_games(
    client: &RobloxClient,
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
    
    let text = client.get_text(&url, "").await?;
    tracing::debug!("[search_games] Response: {}", text.chars().take(500).collect::<String>());
    
    #[derive(Deserialize)]
    struct OmniSearchResponse {
        searchResults: Vec<OmniSearchResult>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct OmniSearchResult {
        place_id: Option<u64>,
        root_place_id: Option<u64>,
        name: Option<String>,
        description: Option<String>,
    }
    
    let resp: OmniSearchResponse = serde_json::from_str(&text).map_err(|e| {
        CoreError::RobloxApi { status: 400, message: format!("parse error: {}", e) }
    })?;
    
    let results: Vec<GameSearchResult> = resp.searchResults
        .into_iter()
        .filter_map(|g| {
            let place_id = g.place_id.or(g.root_place_id)?;
            Some(GameSearchResult {
                place_id,
                name: g.name.unwrap_or_default(),
                description: g.description.unwrap_or_default(),
                root_place_id: g.root_place_id.unwrap_or(place_id),
                thumbnail_url: String::new(),
                universe_id: None,
                playing: 0,
                visits: 0,
                max_players: 0,
                create_vip_servers_allowed: false,
                vip_server_price: 0,
            })
        })
        .take(limit as usize)
        .collect();
    
    tracing::info!("[search_games] Got {} results", results.len());
    Ok(results)
}

/// Get popular/trending games using the explore API
pub async fn get_popular_games(
    client: &RobloxClient,
    limit: u32,
) -> Result<Vec<GameSearchResult>, CoreError> {
    let session_id = Uuid::new_v4().to_string();
    let url = format!(
        "https://apis.roblox.com/explore-api/v1/get-sorts?sessionId={}&device=computer&country=all",
        session_id
    );
    tracing::info!("[get_popular_games] Fetching: {}", url);
    
    let text = client.get_text(&url, "").await?;
    tracing::debug!("[get_popular_games] Response: {}", text.chars().take(500).collect::<String>());
    
    #[derive(Deserialize)]
    struct ExploreResponse {
        #[serde(rename = "Games")]
        games: Option<Vec<ExploreGame>>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ExploreGame {
        place_id: Option<u64>,
        root_place_id: Option<u64>,
        name: Option<String>,
        description: Option<String>,
    }
    
    let resp: ExploreResponse = serde_json::from_str(&text).map_err(|e| {
        CoreError::RobloxApi { status: 400, message: format!("parse error: {}", e) }
    })?;
    
    let games = resp.games.unwrap_or_default();
    let results: Vec<GameSearchResult> = games
        .into_iter()
        .filter_map(|g| {
            let place_id = g.place_id.or(g.root_place_id)?;
            Some(GameSearchResult {
                place_id,
                name: g.name.unwrap_or_default(),
                description: g.description.unwrap_or_default(),
                root_place_id: g.root_place_id.unwrap_or(place_id),
                thumbnail_url: String::new(),
                universe_id: None,
                playing: 0,
                visits: 0,
                max_players: 0,
                create_vip_servers_allowed: false,
                vip_server_price: 0,
            })
        })
        .take(limit as usize)
        .collect();
    
    tracing::info!("[get_popular_games] Got {} results", results.len());
    Ok(results)
}