use crate::error::AppResult;
use crate::state::AppState;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub key: String,
    pub mod_id: u32,
    pub current_file_id: u64,
    pub current_version: String,
    pub latest_file_id: Option<u64>,
    pub latest_version: Option<String>,
    pub update_available: bool,
}

/// Check every installed mod in the active profile for a newer file on Nexus.
pub async fn check_all(state: &AppState) -> AppResult<Vec<UpdateInfo>> {
    let mods = crate::mods::installed_list(state)?;
    let mut out = Vec::new();
    for m in mods {
        let info = check_one(state, &m).await.unwrap_or(UpdateInfo {
            key: m.key.clone(),
            mod_id: m.mod_id,
            current_file_id: m.file_id,
            current_version: m.version.clone(),
            latest_file_id: None,
            latest_version: None,
            update_available: false,
        });
        out.push(info);
    }
    Ok(out)
}

async fn check_one(state: &AppState, m: &crate::mods::InstalledMod) -> AppResult<UpdateInfo> {
    if m.source == "gamebanana" {
        return check_gamebanana(state, m).await;
    }
    let files = crate::nexus::mod_files(state, m.mod_id).await?;
    let empty = Vec::new();
    let arr = files.get("files").and_then(|f| f.as_array()).unwrap_or(&empty);

    // Consider primary / MAIN / UPDATE category files as upgrade candidates.
    let mut best: Option<(u64, String, i64)> = None;
    for f in arr {
        let file_id = f.get("file_id").and_then(|x| x.as_u64());
        let category = f
            .get("category_name")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        let is_primary = f.get("is_primary").and_then(|x| x.as_bool()).unwrap_or(false);
        let ts = f.get("uploaded_timestamp").and_then(|x| x.as_i64()).unwrap_or(0);
        let version = f
            .get("version")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();

        let eligible = is_primary || matches!(category, "MAIN" | "UPDATE");
        if let Some(fid) = file_id {
            if eligible {
                match &best {
                    Some((_, _, bts)) if *bts >= ts => {}
                    _ => best = Some((fid, version, ts)),
                }
            }
        }
    }

    let (latest_file_id, latest_version) = match best {
        Some((fid, ver, _)) => (Some(fid), Some(ver)),
        None => (None, None),
    };
    let update_available = latest_file_id.map(|fid| fid > m.file_id).unwrap_or(false);

    Ok(UpdateInfo {
        key: m.key.clone(),
        mod_id: m.mod_id,
        current_file_id: m.file_id,
        current_version: m.version.clone(),
        latest_file_id,
        latest_version,
        update_available,
    })
}

/// Update check for a GameBanana mod: newest file by id wins.
async fn check_gamebanana(state: &AppState, m: &crate::mods::InstalledMod) -> AppResult<UpdateInfo> {
    let files = crate::gamebanana::mod_files(state, m.mod_id).await?;
    let newest = files.iter().max_by_key(|f| f.file_id);
    let latest_file_id = newest.map(|f| f.file_id);
    let latest_version = newest.and_then(|f| f.version.clone());
    let update_available = latest_file_id.map(|fid| fid > m.file_id).unwrap_or(false);
    Ok(UpdateInfo {
        key: m.key.clone(),
        mod_id: m.mod_id,
        current_file_id: m.file_id,
        current_version: m.version.clone(),
        latest_file_id,
        latest_version,
        update_available,
    })
}
