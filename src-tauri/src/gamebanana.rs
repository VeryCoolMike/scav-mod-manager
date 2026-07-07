use crate::error::{AppError, AppResult};
use crate::mods::ModMeta;
use crate::state::AppState;
use serde::Serialize;
use serde_json::Value;

/// GameBanana numeric game id for Casualties: Unknown.
pub const GB_GAME_ID: u32 = 24260;
const API: &str = "https://gamebanana.com/apiv11";

/// A mod as shown in browse/search results — no account required to fetch.
#[derive(Debug, Clone, Serialize)]
pub struct GbMod {
    pub mod_id: u32,
    pub name: String,
    pub author: Option<String>,
    pub image_url: Option<String>,
    pub page_url: Option<String>,
    pub version: Option<String>,
    pub summary: Option<String>,
    pub likes: i64,
    pub ts_modified: i64,
    pub has_files: bool,
}

/// A downloadable file attached to a mod.
#[derive(Debug, Clone, Serialize)]
pub struct GbFile {
    pub file_id: u64,
    pub filename: String,
    pub download_url: String,
    pub size: u64,
    pub version: Option<String>,
    pub description: Option<String>,
    pub ts_added: i64,
    pub av_clean: bool,
}

async fn get_json(state: &AppState, url: &str) -> AppResult<Value> {
    let resp = state.http.get(url).header("Accept", "application/json").send().await?;
    if !resp.status().is_success() {
        return Err(AppError::msg(format!(
            "GameBanana request failed ({}) for {url}",
            resp.status()
        )));
    }
    Ok(resp.json().await?)
}

/// Browse the game's mods. `sort` is "new" (newest) or "default" (featured).
pub async fn browse(state: &AppState, sort: &str, page: u32) -> AppResult<Vec<GbMod>> {
    let sort = match sort {
        "new" | "default" | "updated" => sort,
        _ => "new",
    };
    let url = format!("{API}/Game/{GB_GAME_ID}/Subfeed?_nPage={page}&_sSort={sort}");
    let v = get_json(state, &url).await?;
    Ok(records_to_mods(&v))
}

/// Keyword-search the game's mods (GameBanana has a real search endpoint).
pub async fn search(state: &AppState, query: &str, page: u32) -> AppResult<Vec<GbMod>> {
    let q = urlencoding(query);
    let url = format!(
        "{API}/Util/Search/Results?_sSearchString={q}&_idGameRow={GB_GAME_ID}&_sModelName=Mod&_nPage={page}"
    );
    let v = get_json(state, &url).await?;
    Ok(records_to_mods(&v))
}

/// List the downloadable files for a mod.
pub async fn mod_files(state: &AppState, mod_id: u32) -> AppResult<Vec<GbFile>> {
    let url = format!("{API}/Mod/{mod_id}?_csvProperties=_aFiles");
    let v = get_json(state, &url).await?;
    Ok(parse_files(v.get("_aFiles")))
}

/// Full mod info used for install metadata.
async fn fetch_full(state: &AppState, mod_id: u32) -> AppResult<(GbMod, Vec<GbFile>)> {
    let url = format!(
        "{API}/Mod/{mod_id}?_csvProperties=_sName,_sProfileUrl,_nLikeCount,_aSubmitter,_aFiles,_aPreviewMedia,_sVersion,_sDescription,_tsDateModified"
    );
    let v = get_json(state, &url).await?;
    let files = parse_files(v.get("_aFiles"));
    let m = GbMod {
        mod_id,
        name: v.get("_sName").and_then(|x| x.as_str()).unwrap_or("Mod").to_string(),
        author: v.pointer("/_aSubmitter/_sName").and_then(|x| x.as_str()).map(String::from),
        image_url: first_image(&v),
        page_url: v.get("_sProfileUrl").and_then(|x| x.as_str()).map(String::from),
        version: v.get("_sVersion").and_then(|x| x.as_str()).map(String::from),
        summary: v.get("_sDescription").and_then(|x| x.as_str()).map(String::from),
        likes: v.get("_nLikeCount").and_then(|x| x.as_i64()).unwrap_or(0),
        ts_modified: v.get("_tsDateModified").and_then(|x| x.as_i64()).unwrap_or(0),
        has_files: !files.is_empty(),
    };
    Ok((m, files))
}

/// Install a specific file of a mod — fully account-free.
pub async fn install(
    state: &AppState,
    mod_id: u32,
    file_id: u64,
) -> AppResult<crate::mods::InstalledMod> {
    let (m, files) = fetch_full(state, mod_id).await?;
    let file = files.iter().find(|f| f.file_id == file_id);
    // `/dl/<file_id>` resolves anonymously to the archive (302 → CDN).
    let url = file
        .map(|f| f.download_url.clone())
        .unwrap_or_else(|| format!("https://gamebanana.com/dl/{file_id}"));
    let version = file
        .and_then(|f| f.version.clone())
        .or(m.version.clone())
        .unwrap_or_else(|| "1.0".to_string());

    let meta = ModMeta {
        source: "gamebanana".to_string(),
        mod_id,
        file_id,
        name: m.name.clone(),
        version,
        author: m.author.clone(),
        picture_url: m.image_url.clone(),
        page_url: m.page_url.clone(),
    };
    crate::mods::install_from_url(state, &url, meta).await
}

// ---- parsing helpers ----------------------------------------------------

fn records_to_mods(v: &Value) -> Vec<GbMod> {
    let empty = Vec::new();
    let records = v.get("_aRecords").and_then(|r| r.as_array()).unwrap_or(&empty);
    records
        .iter()
        .filter(|r| r.get("_sModelName").and_then(|m| m.as_str()) == Some("Mod"))
        .map(|r| GbMod {
            mod_id: r.get("_idRow").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
            name: r.get("_sName").and_then(|x| x.as_str()).unwrap_or("Mod").to_string(),
            author: r.pointer("/_aSubmitter/_sName").and_then(|x| x.as_str()).map(String::from),
            image_url: first_image(r),
            page_url: r.get("_sProfileUrl").and_then(|x| x.as_str()).map(String::from),
            version: r.get("_sVersion").and_then(|x| x.as_str()).map(String::from),
            summary: r.get("_sDescription").and_then(|x| x.as_str()).map(String::from),
            likes: r.get("_nLikeCount").and_then(|x| x.as_i64()).unwrap_or(0),
            ts_modified: r.get("_tsDateModified").and_then(|x| x.as_i64()).unwrap_or(0),
            has_files: r.get("_bHasFiles").and_then(|x| x.as_bool()).unwrap_or(true),
        })
        .collect()
}

fn parse_files(files: Option<&Value>) -> Vec<GbFile> {
    let Some(arr) = files.and_then(|f| f.as_array()) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|f| {
            let file_id = f.get("_idRow").and_then(|x| x.as_u64())?;
            Some(GbFile {
                file_id,
                filename: f.get("_sFile").and_then(|x| x.as_str()).unwrap_or("file.zip").to_string(),
                download_url: f
                    .get("_sDownloadUrl")
                    .and_then(|x| x.as_str())
                    .map(String::from)
                    .unwrap_or_else(|| format!("https://gamebanana.com/dl/{file_id}")),
                size: f.get("_nFilesize").and_then(|x| x.as_u64()).unwrap_or(0),
                version: f.get("_sVersion").and_then(|x| x.as_str()).filter(|s| !s.is_empty()).map(String::from),
                description: f.get("_sDescription").and_then(|x| x.as_str()).map(String::from),
                ts_added: f.get("_tsDateAdded").and_then(|x| x.as_i64()).unwrap_or(0),
                av_clean: f.get("_sAvResult").and_then(|x| x.as_str()) != Some("suspicious"),
            })
        })
        .collect()
}

/// Build a thumbnail URL from a record/detail's preview media.
fn first_image(v: &Value) -> Option<String> {
    let imgs = v.pointer("/_aPreviewMedia/_aImages")?.as_array()?;
    let img = imgs.first()?;
    let base = img.get("_sBaseUrl")?.as_str()?;
    let file = img
        .get("_sFile530")
        .and_then(|x| x.as_str())
        .or_else(|| img.get("_sFile220").and_then(|x| x.as_str()))
        .or_else(|| img.get("_sFile").and_then(|x| x.as_str()))?;
    Some(format!("{base}/{file}"))
}

/// Minimal percent-encoding for a search query.
fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
