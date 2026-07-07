use crate::error::{AppError, AppResult};
use crate::state::{looks_like_game, AppState, GAME_EXE, STEAM_APPIDS};
use serde::Serialize;
#[cfg(target_os = "windows")]
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct DetectedGame {
    pub path: String,
    pub source: String,
    /// Set when `source` is "steam" - the AppID to `-applaunch` with.
    pub steam_appid: Option<String>,
    /// Best-effort version string, when one was easy to read (currently only
    /// Steam's build id from the app manifest).
    pub version: Option<String>,
}

/// Auto-detect every plausible game install across Steam, itch, and common
/// download/document locations. Returns all candidates found (deduped by
/// path) so the caller can let the user confirm each one individually rather
/// than silently picking the first match.
pub fn detect_games() -> Vec<DetectedGame> {
    let mut found: Vec<DetectedGame> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    let mut push = |path: PathBuf, source: &str, steam_appid: Option<String>, version: Option<String>| {
        // Resolve symlinks so e.g. ~/.steam/root and ~/.local/share/Steam
        // (which commonly point at the same install) collapse into one entry.
        let canonical = std::fs::canonicalize(&path).unwrap_or(path);
        let path = canonical.to_string_lossy().into_owned();
        if seen.insert(path.clone()) {
            found.push(DetectedGame {
                path,
                source: source.to_string(),
                steam_appid,
                version,
            });
        }
    };

    for (path, appid, version) in detect_steam() {
        push(path, "steam", Some(appid), version);
    }
    for p in candidate_dirs() {
        if looks_like_game(&p) {
            push(p, "manual", None, None);
        }
    }
    for p in scan_common_folders() {
        if looks_like_game(&p) {
            push(p, "manual", None, None);
        }
    }

    found
}

/// Locate all Steam library roots by parsing libraryfolders.vdf.
fn steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for base in steam_base_dirs() {
        let vdf = base.join("steamapps").join("libraryfolders.vdf");
        if let Ok(text) = std::fs::read_to_string(&vdf) {
            for line in text.lines() {
                // Lines look like:  "path"    "/home/user/.local/share/Steam"
                let line = line.trim();
                if let Some(rest) = line.strip_prefix("\"path\"") {
                    if let Some(p) = extract_quoted(rest) {
                        roots.push(PathBuf::from(p));
                    }
                }
            }
        }
        // The base Steam dir is itself a library.
        roots.push(base);
    }
    roots.sort();
    roots.dedup();
    roots
}

/// Default Steam installation directories per platform.
fn steam_base_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = dirs_home() {
        // Linux (native + Flatpak).
        dirs.push(home.join(".local/share/Steam"));
        dirs.push(home.join(".steam/steam"));
        dirs.push(home.join(".steam/root"));
        dirs.push(home.join(".var/app/com.valvesoftware.Steam/data/Steam"));
    }
    #[cfg(target_os = "windows")]
    {
        dirs.push(PathBuf::from("C:\\Program Files (x86)\\Steam"));
        dirs.push(PathBuf::from("C:\\Program Files\\Steam"));
    }
    dirs.into_iter().filter(|d| d.exists()).collect()
}

/// Find every Steam install of the game across all libraries. Returns
/// (path, appid, version) triples - version is Steam's numeric build id,
/// read straight out of the app manifest, when a manifest was found.
fn detect_steam() -> Vec<(PathBuf, String, Option<String>)> {
    let mut out = Vec::new();
    for root in steam_roots() {
        let steamapps = root.join("steamapps");
        for appid in STEAM_APPIDS {
            let manifest = steamapps.join(format!("appmanifest_{appid}.acf"));
            if let Ok(text) = std::fs::read_to_string(&manifest) {
                if let Some(installdir) = acf_value(&text, "installdir") {
                    let candidate = steamapps.join("common").join(&installdir);
                    if looks_like_game(&candidate) {
                        let version = acf_value(&text, "buildid").map(|b| format!("build {b}"));
                        out.push((candidate, appid.to_string(), version));
                    }
                }
            }
        }
        // Fallback: probe common install dir names even without a manifest
        // (covers the full game and its separately-listed Steam demo).
        for name in [
            "Casualties Unknown",
            "CasualtiesUnknown",
            "Casualties Unknown Demo",
            "Scav Prototype",
        ] {
            let candidate = steamapps.join("common").join(name);
            if looks_like_game(&candidate) {
                out.push((candidate, STEAM_APPIDS[0].to_string(), None));
            }
        }
    }
    out
}

/// Other likely locations (itch app, etc.).
fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = dirs_home() {
        // itch.io app default install location.
        dirs.push(home.join(".config/itch/apps/scav-prototype"));
        dirs.push(home.join("Games/scav-prototype"));
    }
    dirs.into_iter().filter(|d| d.exists()).collect()
}

/// Scan the Downloads and Documents folders for a manually-extracted copy of
/// the game, up to two levels deep (covers both a direct extract and an
/// extra wrapper folder from how archive tools unpack zips).
fn scan_common_folders() -> Vec<PathBuf> {
    let Some(user_dirs) = directories::UserDirs::new() else {
        return Vec::new();
    };
    let mut bases = Vec::new();
    if let Some(d) = user_dirs.download_dir() {
        bases.push(d.to_path_buf());
    }
    if let Some(d) = user_dirs.document_dir() {
        bases.push(d.to_path_buf());
    }

    let mut out = Vec::new();
    for base in bases {
        out.push(base.clone());
        let Ok(entries) = std::fs::read_dir(&base) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }
            out.push(p.clone());
            if let Ok(inner) = std::fs::read_dir(&p) {
                for entry in inner.flatten() {
                    let p2 = entry.path();
                    if p2.is_dir() {
                        out.push(p2);
                    }
                }
            }
        }
    }
    out
}

/// Validate a user-picked folder, returning a normalized game path if valid.
pub fn validate_game_dir(dir: &str) -> AppResult<String> {
    let path = PathBuf::from(dir);
    if looks_like_game(&path) {
        return Ok(path.to_string_lossy().into_owned());
    }
    // The user may have picked the parent; look one level down.
    if let Ok(entries) = std::fs::read_dir(&path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() && looks_like_game(&p) {
                return Ok(p.to_string_lossy().into_owned());
            }
        }
    }
    Err(AppError::msg(format!(
        "'{dir}' does not look like a Casualties: Unknown install (missing {GAME_EXE})"
    )))
}

/// Launch the game. `modded` toggles the BepInEx doorstop for this run.
pub fn launch_game(state: &AppState, modded: bool) -> AppResult<()> {
    let game_path = state.require_game_path()?;
    let settings = state.settings();

    // Toggle the doorstop config so a "vanilla" launch runs without mods.
    crate::bepinex::set_doorstop_enabled(&game_path, modded)?;

    use std::process::Command;
    let source = settings.game_source.as_deref().unwrap_or("manual");

    if source == "steam" {
        // Steam applies configured launch options (needed for BepInEx on Proton).
        let steam = which_steam();
        let appid = settings.steam_appid.as_deref().unwrap_or(STEAM_APPIDS[0]);
        Command::new(steam)
            .arg("-applaunch")
            .arg(appid)
            .spawn()
            .map_err(|e| AppError::msg(format!("failed to launch via Steam: {e}")))?;
        return Ok(());
    }

    // Custom Linux wrapper (e.g. a run_bepinex.sh invocation).
    #[cfg(not(target_os = "windows"))]
    if let Some(cmd) = settings.linux_launch.as_ref().filter(|c| !c.trim().is_empty()) {
        Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(&game_path)
            .spawn()
            .map_err(|e| AppError::msg(format!("failed to run launch command: {e}")))?;
        return Ok(());
    }

    let exe = game_path.join(GAME_EXE);
    #[cfg(target_os = "windows")]
    {
        Command::new(&exe)
            .current_dir(&game_path)
            .spawn()
            .map_err(|e| AppError::msg(format!("failed to launch game: {e}")))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Non-Steam on Linux: best effort via the run_bepinex.sh helper or Wine.
        let script = game_path.join("run_bepinex.sh");
        if modded && script.exists() {
            Command::new("sh")
                .arg(script)
                .current_dir(&game_path)
                .spawn()
                .map_err(|e| AppError::msg(format!("failed to run run_bepinex.sh: {e}")))?;
        } else {
            let mut cmd = Command::new("wine");
            cmd.arg(&exe).current_dir(&game_path);
            if modded {
                // Without this, Wine's built-in winhttp.dll takes precedence
                // over BepInEx's doorstop hook and the game runs vanilla.
                cmd.env("WINEDLLOVERRIDES", "winhttp=n,b");
            }
            cmd.spawn().map_err(|e| {
                AppError::msg(format!(
                    "failed to launch via Wine (install wine or use the Steam source): {e}"
                ))
            })?;
        }
    }
    Ok(())
}

fn which_steam() -> String {
    #[cfg(target_os = "windows")]
    {
        for p in [
            "C:\\Program Files (x86)\\Steam\\steam.exe",
            "C:\\Program Files\\Steam\\steam.exe",
        ] {
            if Path::new(p).exists() {
                return p.to_string();
            }
        }
        "steam.exe".to_string()
    }
    #[cfg(not(target_os = "windows"))]
    {
        "steam".to_string()
    }
}

// ---- small parsers ------------------------------------------------------

/// Extract the first double-quoted token from a VDF fragment.
fn extract_quoted(s: &str) -> Option<String> {
    let start = s.find('"')?;
    let rest = &s[start + 1..];
    let end = rest.find('"')?;
    Some(rest[..end].replace("\\\\", "\\"))
}

/// Read a keyed value out of a Steam .acf/.vdf file: `"key"  "value"`.
fn acf_value(text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(&needle) {
            if let Some(v) = extract_quoted(rest) {
                return Some(v);
            }
        }
    }
    None
}

fn dirs_home() -> Option<PathBuf> {
    directories::UserDirs::new().map(|u| u.home_dir().to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_installdir_from_acf() {
        let acf = r#"
"AppState"
{
    "appid"        "4576490"
    "installdir"        "Casualties Unknown"
    "name"        "Casualties: Unknown"
}
"#;
        assert_eq!(acf_value(acf, "installdir").as_deref(), Some("Casualties Unknown"));
        assert_eq!(acf_value(acf, "appid").as_deref(), Some("4576490"));
    }

    #[test]
    fn extracts_library_path_from_vdf_line() {
        let line = "\t\t\"path\"\t\t\"/home/u/.local/share/Steam\"";
        let rest = line.trim().strip_prefix("\"path\"").unwrap();
        assert_eq!(extract_quoted(rest).as_deref(), Some("/home/u/.local/share/Steam"));
    }
}
