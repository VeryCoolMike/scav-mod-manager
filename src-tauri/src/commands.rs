use crate::auto_account::{
    create_temp_account, poll_verification, register_and_authorize, TempAccount, VerificationResult,
};
use crate::bepinex::BepInExStatus;
use crate::error::{AppError, AppResult};
use crate::game::DetectedGame;
use crate::mods::InstalledMod;
use crate::nexus::ValidateResult;
use crate::state::{AppState, Settings};
use crate::updates::UpdateInfo;
use serde_json::Value;
use tauri::Manager;
use tauri::State;

// ---- Settings & game ----------------------------------------------------

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppResult<Settings> {
    Ok(state.settings())
}

#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, settings: Settings) -> AppResult<Settings> {
    state.save_settings(settings)
}

#[tauri::command]
pub fn detect_games() -> AppResult<Vec<DetectedGame>> {
    Ok(crate::game::detect_games())
}

#[tauri::command]
pub fn validate_game_path(dir: String) -> AppResult<String> {
    crate::game::validate_game_dir(&dir)
}

/// Set the game path (validating it) and persist along with its source.
#[tauri::command]
pub fn set_game_path(
    state: State<'_, AppState>,
    dir: String,
    source: Option<String>,
    steam_appid: Option<String>,
) -> AppResult<Settings> {
    let validated = crate::game::validate_game_dir(&dir)?;
    let mut s = state.settings();
    s.game_path = Some(validated);
    s.game_source = source.or(s.game_source).or(Some("manual".into()));
    if steam_appid.is_some() {
        s.steam_appid = steam_appid;
    }
    state.save_settings(s)
}

// ---- BepInEx ------------------------------------------------------------

#[tauri::command]
pub fn bepinex_status(state: State<'_, AppState>) -> AppResult<BepInExStatus> {
    crate::bepinex::status(&state)
}

#[tauri::command]
pub async fn bepinex_install(state: State<'_, AppState>) -> AppResult<BepInExStatus> {
    crate::bepinex::install(&state).await
}

#[tauri::command]
pub fn bepinex_uninstall(state: State<'_, AppState>) -> AppResult<BepInExStatus> {
    crate::bepinex::uninstall(&state)
}

// ---- Nexus --------------------------------------------------------------

/// One-click SSO login: authorize in the browser, key captured automatically.
#[tauri::command]
pub async fn nexus_sso_login(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<ValidateResult> {
    crate::nexus_sso::login(&state, &app).await
}

/// Sign out — forget the stored Nexus key.
#[tauri::command]
pub fn nexus_logout(state: State<'_, AppState>) -> AppResult<Settings> {
    let mut s = state.settings();
    s.nexus_api_key = None;
    s.nexus_user = None;
    s.is_premium = false;
    state.save_settings(s)
}

/// Validate a key and, if valid, persist it plus premium status.
#[tauri::command]
pub async fn nexus_validate(state: State<'_, AppState>, key: String) -> AppResult<ValidateResult> {
    let result = crate::nexus::validate(&state, &key).await?;
    if result.valid {
        let mut s = state.settings();
        s.nexus_api_key = Some(key);
        s.is_premium = result.is_premium;
        s.nexus_user = result.name.clone();
        state.save_settings(s)?;
    }
    Ok(result)
}

#[tauri::command]
pub async fn nexus_browse(state: State<'_, AppState>, list: String) -> AppResult<Value> {
    crate::nexus::browse(&state, &list).await
}

#[tauri::command]
pub async fn nexus_updated(state: State<'_, AppState>, period: String) -> AppResult<Value> {
    crate::nexus::updated(&state, &period).await
}

#[tauri::command]
pub async fn nexus_mod_details(state: State<'_, AppState>, mod_id: u32) -> AppResult<Value> {
    crate::nexus::mod_details(&state, mod_id).await
}

#[tauri::command]
pub async fn nexus_mod_files(state: State<'_, AppState>, mod_id: u32) -> AppResult<Value> {
    crate::nexus::mod_files(&state, mod_id).await
}

#[tauri::command]
pub async fn nexus_endorse(
    state: State<'_, AppState>,
    mod_id: u32,
    endorse: bool,
    version: String,
) -> AppResult<Value> {
    crate::nexus::endorse(&state, mod_id, endorse, &version).await
}

#[tauri::command]
pub async fn nexus_tracked(state: State<'_, AppState>) -> AppResult<Value> {
    crate::nexus::tracked_mods(&state).await
}

// ---- Install ------------------------------------------------------------

/// Install directly from an nxm link the user pasted in.
#[tauri::command]
pub async fn install_nxm(state: State<'_, AppState>, link: String) -> AppResult<InstalledMod> {
    crate::nxm::handle(&state, &link).await
}

/// Premium-only: install a specific file directly via the API (no nxm round-trip).
#[tauri::command]
pub async fn install_mod_file(
    state: State<'_, AppState>,
    mod_id: u32,
    file_id: u64,
) -> AppResult<InstalledMod> {
    if !state.settings().is_premium {
        return Err(AppError::msg(
            "Direct in-app install needs Nexus Premium. Use the website 'Mod Manager Download' \
             button instead (it hands the download to this app).",
        ));
    }
    let links = crate::nexus::download_link(&state, mod_id, file_id, None, None).await?;
    let meta = fetch_meta(&state, mod_id, file_id).await;
    let url = links[0].url.clone();
    crate::mods::install_from_url(
        &state,
        &url,
        crate::mods::ModMeta {
            source: "nexus".to_string(),
            mod_id,
            file_id,
            name: meta.0,
            version: meta.1,
            author: meta.2,
            picture_url: meta.3,
            page_url: Some(format!("https://www.nexusmods.com/scavprototype/mods/{mod_id}")),
        },
    )
    .await
}

// ---- GameBanana (account-free source) -----------------------------------

#[tauri::command]
pub async fn gb_browse(
    state: State<'_, AppState>,
    sort: String,
    page: u32,
) -> AppResult<Vec<crate::gamebanana::GbMod>> {
    crate::gamebanana::browse(&state, &sort, page.max(1)).await
}

#[tauri::command]
pub async fn gb_search(
    state: State<'_, AppState>,
    query: String,
    page: u32,
) -> AppResult<Vec<crate::gamebanana::GbMod>> {
    crate::gamebanana::search(&state, &query, page.max(1)).await
}

#[tauri::command]
pub async fn gb_mod_files(
    state: State<'_, AppState>,
    mod_id: u32,
) -> AppResult<Vec<crate::gamebanana::GbFile>> {
    crate::gamebanana::mod_files(&state, mod_id).await
}

#[tauri::command]
pub async fn gb_install(
    state: State<'_, AppState>,
    mod_id: u32,
    file_id: u64,
) -> AppResult<InstalledMod> {
    crate::gamebanana::install(&state, mod_id, file_id).await
}

// ---- Installed mods -----------------------------------------------------

#[tauri::command]
pub fn list_installed(state: State<'_, AppState>) -> AppResult<Vec<InstalledMod>> {
    crate::mods::installed_list(&state)
}

#[tauri::command]
pub fn set_mod_enabled(state: State<'_, AppState>, key: String, enabled: bool) -> AppResult<()> {
    crate::mods::set_enabled(&state, &key, enabled)
}

#[tauri::command]
pub fn uninstall_mod(state: State<'_, AppState>, key: String) -> AppResult<()> {
    crate::mods::uninstall(&state, &key)
}

#[tauri::command]
pub fn sync_mods(state: State<'_, AppState>) -> AppResult<usize> {
    crate::mods::sync_to_game(&state)
}

// ---- Profiles -----------------------------------------------------------

#[tauri::command]
pub fn list_profiles(state: State<'_, AppState>) -> AppResult<Vec<String>> {
    crate::profiles::list(&state.paths)
}

#[tauri::command]
pub fn create_profile(state: State<'_, AppState>, name: String) -> AppResult<String> {
    crate::profiles::create(&state.paths, &name)
}

#[tauri::command]
pub fn delete_profile(state: State<'_, AppState>, name: String) -> AppResult<Vec<String>> {
    crate::profiles::delete(&state.paths, &name)?;
    // If the active profile was deleted, fall back to Default.
    let mut s = state.settings();
    if s.active_profile == name {
        s.active_profile = "Default".into();
        state.save_settings(s)?;
    }
    crate::profiles::list(&state.paths)
}

#[tauri::command]
pub fn clone_profile(state: State<'_, AppState>, from: String, to: String) -> AppResult<String> {
    crate::profiles::clone_profile(&state.paths, &from, &to)
}

#[tauri::command]
pub fn switch_profile(state: State<'_, AppState>, name: String) -> AppResult<Settings> {
    let mut s = state.settings();
    s.active_profile = name;
    state.save_settings(s)
}

#[tauri::command]
pub fn export_profile_bundle(state: State<'_, AppState>, name: String, dest: String) -> AppResult<()> {
    crate::profiles::export_bundle(&state, &name, std::path::Path::new(&dest))
}

#[tauri::command]
pub fn import_profile_bundle(
    state: State<'_, AppState>,
    zip_path: String,
    new_name: Option<String>,
) -> AppResult<String> {
    crate::profiles::import_bundle(&state, std::path::Path::new(&zip_path), new_name.as_deref())
}

#[tauri::command]
pub fn export_profile_code(state: State<'_, AppState>, name: String) -> AppResult<String> {
    crate::profiles::export_code(&state, &name)
}

#[derive(serde::Serialize)]
pub struct CodeModRef {
    pub source: String,
    pub mod_id: u32,
    pub file_id: u64,
    pub name: String,
}

#[derive(serde::Serialize)]
pub struct ImportCodeStart {
    /// Name of the newly-created profile (already switched to as active).
    pub profile: String,
    /// The mods to install, in order — the frontend drives the actual
    /// per-mod download (premium API / free-tier browser popup / GameBanana)
    /// by calling the existing single-mod install commands for each one.
    pub mods: Vec<CodeModRef>,
}

/// Parse a mod-list code (from `export_profile_code`) into a new profile and
/// the list of mods to install into it.
#[tauri::command]
pub fn import_profile_code(
    state: State<'_, AppState>,
    code: String,
    new_name: Option<String>,
) -> AppResult<ImportCodeStart> {
    let parsed: Value = serde_json::from_str(code.trim())
        .map_err(|_| AppError::msg("That doesn't look like a valid mod-list code."))?;
    let mods_json = parsed
        .get("mods")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if mods_json.is_empty() {
        return Err(AppError::msg("No mods found in that code."));
    }

    let base_name = new_name
        .filter(|n| !n.trim().is_empty())
        .or_else(|| parsed.get("profile").and_then(|v| v.as_str()).map(String::from))
        .unwrap_or_else(|| "Imported".to_string());
    let target = crate::profiles::create_unique(&state.paths, &base_name)?;

    let mut mods = Vec::new();
    for m in &mods_json {
        let mod_id = m.get("mod_id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let file_id = m.get("file_id").and_then(|v| v.as_u64()).unwrap_or(0);
        if mod_id == 0 || file_id == 0 {
            continue;
        }
        let name = m
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("mod")
            .to_string();
        let source = m
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("nexus")
            .to_string();
        mods.push(CodeModRef { source, mod_id, file_id, name });
    }

    // Switch to the new profile so the per-mod installs the frontend is
    // about to trigger land in it.
    let mut s = state.settings();
    s.active_profile = target.clone();
    state.save_settings(s)?;

    Ok(ImportCodeStart { profile: target, mods })
}

// ---- Updates & launch ---------------------------------------------------

#[tauri::command]
pub async fn check_updates(state: State<'_, AppState>) -> AppResult<Vec<UpdateInfo>> {
    crate::updates::check_all(&state).await
}

#[tauri::command]
pub fn launch_game(state: State<'_, AppState>, modded: bool) -> AppResult<()> {
    if modded {
        crate::mods::sync_to_game(&state)?;
    }
    crate::game::launch_game(&state, modded)
}

// ---- shared helper ------------------------------------------------------

async fn fetch_meta(
    state: &AppState,
    mod_id: u32,
    file_id: u64,
) -> (String, String, Option<String>, Option<String>) {
    let mut name = format!("Mod {mod_id}");
    let mut version = "unknown".to_string();
    let mut author = None;
    let mut picture = None;
    if let Ok(details) = crate::nexus::mod_details(state, mod_id).await {
        if let Some(n) = details.get("name").and_then(|v| v.as_str()) {
            name = n.to_string();
        }
        if let Some(v) = details.get("version").and_then(|v| v.as_str()) {
            version = v.to_string();
        }
        author = details.get("author").and_then(|v| v.as_str()).map(String::from);
        picture = details.get("picture_url").and_then(|v| v.as_str()).map(String::from);
    }
    if let Ok(files) = crate::nexus::mod_files(state, mod_id).await {
        if let Some(arr) = files.get("files").and_then(|f| f.as_array()) {
            for f in arr {
                if f.get("file_id").and_then(|x| x.as_u64()) == Some(file_id) {
                    if let Some(v) = f.get("version").and_then(|x| x.as_str()) {
                        version = v.to_string();
                    }
                }
            }
        }
    }
    (name, version, author, picture)
}

// ---- Temporary account (auto-create) ----------------------------------

#[tauri::command]
pub async fn auto_create_account() -> AppResult<TempAccount> {
    create_temp_account().await
}

#[tauri::command]
pub async fn auto_poll_verification(
    email: String,
    timeout_secs: u64,
) -> AppResult<VerificationResult> {
    poll_verification(&email, timeout_secs).await
}

#[tauri::command]
pub async fn auto_full_register(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<ValidateResult> {
    register_and_authorize(&app, &state).await
}

enum AutoDownloadResult {
    /// Caught via the "Mod Manager Download" button (managed download available).
    Nxm(String),
    /// Caught via the "Slow download" button — a plain file landed on disk.
    File(std::path::PathBuf),
}

/// Open the mod's Nexus file page, click whichever download button is
/// actually available, and capture the result — either a nxm:// managed
/// download link, or (when managed download isn't offered for this file) the
/// raw file from the manual/slow download.
#[tauri::command]
pub async fn nexus_auto_download(
    app: tauri::AppHandle,
    mod_id: u32,
    file_id: u64,
) -> AppResult<InstalledMod> {
    use std::sync::{Arc, Mutex};

    let url = format!(
        "https://www.nexusmods.com/{}/mods/{}?tab=files&file_id={}",
        crate::state::GAME_DOMAIN,
        mod_id,
        file_id
    );

    if let Some(old) = app.get_webview_window("nexus_download") {
        let _ = old.close();
    }

    let state = app.state::<AppState>();
    let dest_path = state
        .paths
        .cache_dir
        .join(format!("nexus-slow-{mod_id}-{file_id}.download"));
    std::fs::create_dir_all(&state.paths.cache_dir)?;

    let nxm_url = Arc::new(Mutex::new(None::<String>));
    let nxm_url_clone = nxm_url.clone();

    let downloaded = Arc::new(Mutex::new(None::<std::path::PathBuf>));
    let downloaded_clone = downloaded.clone();
    let dest_for_handler = dest_path.clone();

    let window = tauri::WebviewWindowBuilder::new(
        &app,
        "nexus_download",
        tauri::WebviewUrl::External(url.parse().unwrap()),
    )
    .visible(true)
    .title("Downloading mod…")
    .on_navigation(move |nav_url| {
        let s = nav_url.as_str().to_string();
        if s.starts_with("nxm://") {
            *nxm_url_clone.lock().unwrap() = Some(s);
            false
        } else {
            true
        }
    })
    .on_download(move |_webview, event| {
        match event {
            tauri::webview::DownloadEvent::Requested { url, destination } => {
                eprintln!(
                    "[nexus_auto_download] download requested: url={url} requested_destination={}",
                    dest_for_handler.display()
                );
                *destination = dest_for_handler.clone();
            }
            tauri::webview::DownloadEvent::Finished { url, path, success } => {
                eprintln!(
                    "[nexus_auto_download] download finished: url={url} reported_path={path:?} success={success}"
                );
                if success {
                    // WRY's GTK backend doesn't reliably honor the destination
                    // override (it appears to pass a bare path where WebKit
                    // expects a file:// URI), so trust whatever path it
                    // actually reports rather than assuming our override took
                    // effect. Fall back to our intended path if none is given.
                    let resolved = path
                        .map(|p| {
                            let s = p.to_string_lossy();
                            match s.strip_prefix("file://") {
                                Some(stripped) => std::path::PathBuf::from(stripped),
                                None => p,
                            }
                        })
                        .unwrap_or_else(|| dest_for_handler.clone());
                    eprintln!("[nexus_auto_download] resolved downloaded file to: {}", resolved.display());
                    *downloaded_clone.lock().unwrap() = Some(resolved);
                }
            }
            _ => {}
        }
        true
    })
    .build()
    .map_err(|e| AppError::msg(format!("WebView: {e}")))?;

    // Nexus's file-download widget renders its buttons into a runtime shadow
    // root via bundled JS, and which button is offered (managed vs. slow-only)
    // varies per mod/file. Prefer "Mod Manager Download" when present, else
    // fall back to "Slow download" — search light DOM and nested shadow roots.
    window.eval(r#"
(function(){
function findByText(re){
function search(root){
var els=root.querySelectorAll('button, a');
for(var i=0;i<els.length;i++){
if(re.test((els[i].textContent||'').trim())) return els[i];
}
var all=root.querySelectorAll('*');
for(var i=0;i<all.length;i++){
if(all[i].shadowRoot){
var found=search(all[i].shadowRoot);
if(found) return found;
}
}
return null;
}
return search(document);
}
var attempts=0;
function tryClick(){
attempts++;
var btn=findByText(/mod manager download/i) || findByText(/slow download/i);
if(btn){ btn.click(); return; }
if(attempts<40){ setTimeout(tryClick,1000); }
}
setTimeout(tryClick,1500);
})()"#).ok();

    let resolved = {
        let mut waited = std::time::Duration::ZERO;
        let timeout = std::time::Duration::from_secs(300);
        loop {
            if let Some(u) = nxm_url.lock().unwrap().take() {
                break Some(AutoDownloadResult::Nxm(u));
            }
            if let Some(p) = downloaded.lock().unwrap().take() {
                break Some(AutoDownloadResult::File(p));
            }
            if waited >= timeout {
                break None;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            waited += std::time::Duration::from_millis(500);
        }
    };
    window.close().ok();

    let Some(resolved) = resolved else {
        eprintln!("[nexus_auto_download] timed out waiting for nxm:// or a download");
        return Err(AppError::msg(
            "Timed out waiting for a download to start. Click the mod's download button \
             in the Nexus window yourself, or paste the mod's nxm:// link above.",
        ));
    };

    match resolved {
        AutoDownloadResult::Nxm(link) => {
            eprintln!("[nexus_auto_download] resolved via nxm:// link: {link}");
            crate::nxm::handle(&state, &link).await
        }
        AutoDownloadResult::File(path) => {
            eprintln!(
                "[nexus_auto_download] resolved via slow download: path={} exists={}",
                path.display(),
                path.exists()
            );
            let (name, version, author, picture_url) = fetch_meta(&state, mod_id, file_id).await;
            let meta = crate::mods::ModMeta {
                source: "nexus".to_string(),
                mod_id,
                file_id,
                name,
                version,
                author,
                picture_url,
                page_url: Some(format!(
                    "https://www.nexusmods.com/scavprototype/mods/{mod_id}"
                )),
            };
            let installed = crate::mods::install_local_archive(&state, &path, &meta);
            if let Err(e) = &installed {
                eprintln!("[nexus_auto_download] install_local_archive failed: {e}");
            }
            let _ = std::fs::remove_file(&path);
            installed
        }
    }
}
