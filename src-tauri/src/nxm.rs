use crate::error::{AppError, AppResult};
use crate::mods::{self, InstalledMod};
use crate::state::AppState;
use url::Url;

/// Parsed components of an `nxm://` deep link.
///
/// Format: `nxm://scavprototype/mods/<mod_id>/files/<file_id>?key=<k>&expires=<e>&user_id=<u>`
#[derive(Debug, Clone)]
pub struct NxmLink {
    pub domain: String,
    pub mod_id: u32,
    pub file_id: u64,
    pub key: Option<String>,
    pub expires: Option<String>,
}

pub fn parse(link: &str) -> AppResult<NxmLink> {
    let url = Url::parse(link)?;
    if url.scheme() != "nxm" {
        return Err(AppError::msg(format!("not an nxm link: {link}")));
    }
    let domain = url
        .host_str()
        .ok_or_else(|| AppError::msg("nxm link missing game domain"))?
        .to_string();

    let segments: Vec<&str> = url
        .path_segments()
        .map(|s| s.collect())
        .unwrap_or_default();
    // Expect: ["mods", "<id>", "files", "<id>"]
    let mod_id = segments
        .iter()
        .position(|s| *s == "mods")
        .and_then(|i| segments.get(i + 1))
        .and_then(|s| s.parse::<u32>().ok())
        .ok_or_else(|| AppError::msg("nxm link missing mod id"))?;
    let file_id = segments
        .iter()
        .position(|s| *s == "files")
        .and_then(|i| segments.get(i + 1))
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or_else(|| AppError::msg("nxm link missing file id"))?;

    let mut key = None;
    let mut expires = None;
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "key" => key = Some(v.into_owned()),
            "expires" => expires = Some(v.into_owned()),
            _ => {}
        }
    }

    Ok(NxmLink {
        domain,
        mod_id,
        file_id,
        key,
        expires,
    })
}

/// Handle an nxm link end to end: resolve download, fetch archive, install into
/// the active profile.
pub async fn handle(state: &AppState, link: &str) -> AppResult<InstalledMod> {
    let parsed = parse(link)?;
    if parsed.domain != crate::state::GAME_DOMAIN {
        return Err(AppError::msg(format!(
            "this link is for '{}', not Casualties: Unknown",
            parsed.domain
        )));
    }

    let links = crate::nexus::download_link(
        state,
        parsed.mod_id,
        parsed.file_id,
        parsed.key.as_deref(),
        parsed.expires.as_deref(),
    )
    .await?;
    let url = links[0].url.clone();

    // Pull metadata for a nicer display name/version.
    let (name, version, author, picture) = fetch_meta(state, parsed.mod_id, parsed.file_id).await;

    let meta = mods::ModMeta {
        source: "nexus".to_string(),
        mod_id: parsed.mod_id,
        file_id: parsed.file_id,
        name,
        version,
        author,
        picture_url: picture,
        page_url: Some(format!(
            "https://www.nexusmods.com/scavprototype/mods/{}",
            parsed.mod_id
        )),
    };
    mods::install_from_url(state, &url, meta).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_nxm_link() {
        let link = "nxm://scavprototype/mods/130/files/456?key=abc123&expires=1700000000&user_id=42";
        let p = parse(link).unwrap();
        assert_eq!(p.domain, "scavprototype");
        assert_eq!(p.mod_id, 130);
        assert_eq!(p.file_id, 456);
        assert_eq!(p.key.as_deref(), Some("abc123"));
        assert_eq!(p.expires.as_deref(), Some("1700000000"));
    }

    #[test]
    fn parses_link_without_key() {
        let p = parse("nxm://scavprototype/mods/7/files/12").unwrap();
        assert_eq!(p.mod_id, 7);
        assert_eq!(p.file_id, 12);
        assert!(p.key.is_none());
    }

    #[test]
    fn rejects_non_nxm() {
        assert!(parse("https://example.com/mods/1/files/2").is_err());
    }
}

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
        picture = details
            .get("picture_url")
            .and_then(|v| v.as_str())
            .map(String::from);
    }
    // Prefer the specific file's version when available.
    if let Ok(files) = crate::nexus::mod_files(state, mod_id).await {
        if let Some(arr) = files.get("files").and_then(|f| f.as_array()) {
            for f in arr {
                if f.get("file_id").and_then(|x| x.as_u64()) == Some(file_id) {
                    if let Some(v) = f.get("version").and_then(|x| x.as_str()) {
                        version = v.to_string();
                    }
                    if let Some(n) = f.get("name").and_then(|x| x.as_str()) {
                        if name.starts_with("Mod ") {
                            name = n.to_string();
                        }
                    }
                }
            }
        }
    }

    (name, version, author, picture)
}
