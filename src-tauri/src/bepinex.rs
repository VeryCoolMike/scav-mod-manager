use crate::error::AppResult;
use crate::state::AppState;
use serde::Serialize;
use std::path::Path;

/// Pinned fallback if the GitHub releases API is unavailable/rate-limited.
const FALLBACK_VERSION: &str = "5.4.23.2";
const FALLBACK_URL: &str =
    "https://github.com/BepInEx/BepInEx/releases/download/v5.4.23.2/BepInEx_win_x64_5.4.23.2.zip";

#[derive(Debug, Clone, Serialize)]
pub struct BepInExStatus {
    pub installed: bool,
    pub version: Option<String>,
    /// winhttp.dll present and active (mods will load).
    pub enabled: bool,
    /// True when running on Linux, where a one-time Steam launch option is needed.
    pub needs_proton_setup: bool,
    pub proton_launch_option: Option<String>,
}

pub fn status(state: &AppState) -> AppResult<BepInExStatus> {
    let game = state.require_game_path()?;
    let installed = game.join("BepInEx").join("core").exists();
    let version = std::fs::read_to_string(game.join("BepInEx").join(".smm_version"))
        .ok()
        .map(|v| v.trim().to_string());
    let enabled = game.join("winhttp.dll").exists();

    let needs_proton_setup = cfg!(not(target_os = "windows"))
        && state.settings().game_source.as_deref() == Some("steam");

    Ok(BepInExStatus {
        installed,
        version,
        enabled,
        needs_proton_setup,
        proton_launch_option: if needs_proton_setup {
            Some("WINEDLLOVERRIDES=\"winhttp=n,b\" %command%".to_string())
        } else {
            None
        },
    })
}

/// Download and extract BepInEx 5 (x64, Mono) into the game folder.
pub async fn install(state: &AppState) -> AppResult<BepInExStatus> {
    let game = state.require_game_path()?;
    let (url, version) = resolve_download(&state.http).await;

    let tmp = state.paths.cache_dir.join("bepinex.zip");
    crate::mods::download_url(&state.http, &url, &tmp).await?;

    // BepInEx zips extract directly into the game root (winhttp.dll, BepInEx/, doorstop_config.ini).
    crate::mods::extract_zip(&tmp, &game)?;
    let _ = std::fs::remove_file(&tmp);

    std::fs::create_dir_all(game.join("BepInEx").join("plugins"))?;
    std::fs::write(game.join("BepInEx").join(".smm_version"), &version)?;

    // Ensure mods are enabled after a fresh install.
    set_doorstop_enabled(&game, true)?;

    status(state)
}

pub fn uninstall(state: &AppState) -> AppResult<BepInExStatus> {
    let game = state.require_game_path()?;
    for entry in [
        "BepInEx",
        "winhttp.dll",
        "winhttp.dll.disabled",
        "doorstop_config.ini",
        ".doorstop_version",
        "run_bepinex.sh",
        "changelog.txt",
    ] {
        let p = game.join(entry);
        if p.is_dir() {
            let _ = std::fs::remove_dir_all(&p);
        } else if p.exists() {
            let _ = std::fs::remove_file(&p);
        }
    }
    status(state)
}

/// Enable or disable the BepInEx doorstop by renaming winhttp.dll.
/// This is loader-version agnostic and works under Proton/Wine too.
pub fn set_doorstop_enabled(game: &Path, enabled: bool) -> AppResult<()> {
    let active = game.join("winhttp.dll");
    let disabled = game.join("winhttp.dll.disabled");
    if enabled {
        if !active.exists() && disabled.exists() {
            std::fs::rename(&disabled, &active)?;
        }
    } else if active.exists() {
        // Remove a stale disabled copy first so rename doesn't fail on Windows.
        if disabled.exists() {
            let _ = std::fs::remove_file(&disabled);
        }
        std::fs::rename(&active, &disabled)?;
    }
    Ok(())
}

/// Find the best BepInEx 5 (win x64) asset, falling back to a pinned version.
async fn resolve_download(client: &reqwest::Client) -> (String, String) {
    match latest_from_github(client).await {
        Some(pair) => pair,
        None => (FALLBACK_URL.to_string(), FALLBACK_VERSION.to_string()),
    }
}

async fn latest_from_github(client: &reqwest::Client) -> Option<(String, String)> {
    let resp = client
        .get("https://api.github.com/repos/BepInEx/BepInEx/releases/latest")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let json: serde_json::Value = resp.json().await.ok()?;
    let tag = json.get("tag_name")?.as_str()?.trim_start_matches('v').to_string();
    let assets = json.get("assets")?.as_array()?;
    let mut best: Option<String> = None;
    for a in assets {
        let name = a.get("name").and_then(|n| n.as_str()).unwrap_or("").to_lowercase();
        let url = a.get("browser_download_url").and_then(|u| u.as_str()).unwrap_or("");
        let is_x64 = name.contains("x64");
        let is_il2cpp = name.contains("il2cpp");
        let is_unix = name.contains("unix") || name.contains("linux") || name.contains("macos");
        let is_arm = name.contains("arm");
        if is_x64 && !is_il2cpp && !is_unix && !is_arm && name.ends_with(".zip") {
            // Prefer an explicitly "win" asset.
            if name.contains("win") {
                return Some((url.to_string(), tag));
            }
            best.get_or_insert_with(|| url.to_string());
        }
    }
    best.map(|u| (u, tag))
}
