use crate::error::{AppError, AppResult};
use crate::state::{AppState, GAME_DOMAIN};
use serde::Serialize;
use serde_json::Value;

const API_BASE: &str = "https://api.nexusmods.com";

#[derive(Debug, Clone, Serialize)]
pub struct ValidateResult {
    pub valid: bool,
    pub user_id: Option<u64>,
    pub name: Option<String>,
    pub is_premium: bool,
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadLink {
    pub url: String,
    pub short_name: Option<String>,
}

/// Build an authenticated GET request to the Nexus API.
fn get(state: &AppState, key: &str, path: &str) -> reqwest::RequestBuilder {
    state
        .http
        .get(format!("{API_BASE}{path}"))
        .header("apikey", key)
        .header("Accept", "application/json")
        .header("Application-Name", "ScavModManager")
        .header("Application-Version", env!("CARGO_PKG_VERSION"))
}

/// Turn a non-2xx response into a structured error and check rate limits.
async fn read_json(resp: reqwest::Response) -> AppResult<Value> {
    let status = resp.status();
    if status.is_success() {
        Ok(resp.json().await?)
    } else if status.as_u16() == 429 {
        Err(AppError::msg(
            "Nexus rate limit reached (429). Wait a bit and try again.",
        ))
    } else {
        let body = resp.text().await.unwrap_or_default();
        Err(AppError::Nexus {
            status: status.as_u16(),
            body: body.chars().take(300).collect(),
        })
    }
}

/// Validate an API key and report premium status.
pub async fn validate(state: &AppState, key: &str) -> AppResult<ValidateResult> {
    let resp = get(state, key, "/v1/users/validate.json").send().await?;
    if resp.status().as_u16() == 401 {
        return Ok(ValidateResult {
            valid: false,
            user_id: None,
            name: None,
            is_premium: false,
            email: None,
        });
    }
    let v = read_json(resp).await?;
    Ok(ValidateResult {
        valid: true,
        user_id: v.get("user_id").and_then(|x| x.as_u64()),
        name: v.get("name").and_then(|x| x.as_str()).map(String::from),
        is_premium: v.get("is_premium").and_then(|x| x.as_bool()).unwrap_or(false),
        email: v.get("email").and_then(|x| x.as_str()).map(String::from),
    })
}

/// One of the browse lists: latest_added | latest_updated | trending.
pub async fn browse(state: &AppState, list: &str) -> AppResult<Value> {
    let key = state.require_api_key()?;
    let list = match list {
        "latest_added" | "latest_updated" | "trending" => list,
        _ => "latest_added",
    };
    let resp = get(state, &key, &format!("/v1/games/{GAME_DOMAIN}/mods/{list}.json"))
        .send()
        .await?;
    read_json(resp).await
}

/// Mods updated within a period: 1d | 1w | 1m.
pub async fn updated(state: &AppState, period: &str) -> AppResult<Value> {
    let key = state.require_api_key()?;
    let period = match period {
        "1d" | "1w" | "1m" => period,
        _ => "1w",
    };
    let resp = get(
        state,
        &key,
        &format!("/v1/games/{GAME_DOMAIN}/mods/updated.json?period={period}"),
    )
    .send()
    .await?;
    read_json(resp).await
}

pub async fn mod_details(state: &AppState, mod_id: u32) -> AppResult<Value> {
    let key = state.require_api_key()?;
    let resp = get(state, &key, &format!("/v1/games/{GAME_DOMAIN}/mods/{mod_id}.json"))
        .send()
        .await?;
    read_json(resp).await
}

pub async fn mod_files(state: &AppState, mod_id: u32) -> AppResult<Value> {
    let key = state.require_api_key()?;
    let resp = get(
        state,
        &key,
        &format!("/v1/games/{GAME_DOMAIN}/mods/{mod_id}/files.json"),
    )
    .send()
    .await?;
    read_json(resp).await
}

/// Resolve a CDN download link for a file.
/// Free accounts must pass `key`/`expires` obtained from an nxm:// link.
pub async fn download_link(
    state: &AppState,
    mod_id: u32,
    file_id: u64,
    nxm_key: Option<&str>,
    expires: Option<&str>,
) -> AppResult<Vec<DownloadLink>> {
    let key = state.require_api_key()?;
    let mut path = format!("/v1/games/{GAME_DOMAIN}/mods/{mod_id}/files/{file_id}/download_link.json");
    if let (Some(k), Some(e)) = (nxm_key, expires) {
        path.push_str(&format!("?key={k}&expires={e}"));
    }
    let resp = get(state, &key, &path).send().await?;
    if resp.status().as_u16() == 403 && nxm_key.is_none() {
        return Err(AppError::msg(
            "Nexus returned 403: free accounts can only get download links via the \
             'Mod Manager Download' button on the website (nxm:// link). Use that button, \
             or upgrade to Premium for in-app downloads.",
        ));
    }
    let v = read_json(resp).await?;
    let arr = v.as_array().cloned().unwrap_or_default();
    let links = arr
        .iter()
        .filter_map(|item| {
            Some(DownloadLink {
                url: item.get("URI")?.as_str()?.to_string(),
                short_name: item.get("short_name").and_then(|s| s.as_str()).map(String::from),
            })
        })
        .collect::<Vec<_>>();
    if links.is_empty() {
        return Err(AppError::msg("Nexus returned no download links for this file"));
    }
    Ok(links)
}

/// Look up a mod by the MD5 hash of one of its files.
#[allow(dead_code)]
pub async fn md5_search(state: &AppState, md5: &str) -> AppResult<Value> {
    let key = state.require_api_key()?;
    let resp = get(
        state,
        &key,
        &format!("/v1/games/{GAME_DOMAIN}/mods/md5_search/{md5}.json"),
    )
    .send()
    .await?;
    read_json(resp).await
}

/// Endorse (or abstain from) a mod.
pub async fn endorse(state: &AppState, mod_id: u32, endorse: bool, version: &str) -> AppResult<Value> {
    let key = state.require_api_key()?;
    let action = if endorse { "endorse" } else { "abstain" };
    let resp = state
        .http
        .post(format!(
            "{API_BASE}/v1/games/{GAME_DOMAIN}/mods/{mod_id}/{action}.json"
        ))
        .header("apikey", &key)
        .header("Accept", "application/json")
        .header("Application-Name", "ScavModManager")
        .header("Application-Version", env!("CARGO_PKG_VERSION"))
        .json(&serde_json::json!({ "version": version }))
        .send()
        .await?;
    read_json(resp).await
}

pub async fn tracked_mods(state: &AppState) -> AppResult<Value> {
    let key = state.require_api_key()?;
    let resp = get(state, &key, "/v1/user/tracked_mods.json").send().await?;
    read_json(resp).await
}
