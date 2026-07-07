use crate::error::{AppError, AppResult};
use crate::state::{sanitize, AppPaths, AppState};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

/// A mod tracked within a profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileMod {
    /// "<source>-<mod_id>-<file_id>" — also the on-disk folder name.
    pub key: String,
    /// Where the mod came from: "gamebanana" | "nexus".
    #[serde(default = "default_source")]
    pub source: String,
    pub mod_id: u32,
    pub file_id: u64,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub picture_url: Option<String>,
    /// Link to the mod's web page (for the "open in browser" action).
    #[serde(default)]
    pub page_url: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_source() -> String {
    "nexus".to_string()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Profile {
    pub name: String,
    #[serde(default)]
    pub mods: Vec<ProfileMod>,
}

/// Root folder for a profile's metadata (profile.json, .synced.json).
pub fn profile_meta(paths: &AppPaths, name: &str) -> PathBuf {
    paths.profile_dir(name)
}

/// Folder holding a profile's installed mod trees.
pub fn mods_dir(paths: &AppPaths, name: &str) -> PathBuf {
    paths.profile_dir(name).join("mods")
}

fn profile_file(paths: &AppPaths, name: &str) -> PathBuf {
    paths.profile_dir(name).join("profile.json")
}

pub fn load(paths: &AppPaths, name: &str) -> AppResult<Profile> {
    let file = profile_file(paths, name);
    if !file.exists() {
        return Ok(Profile {
            name: name.to_string(),
            mods: Vec::new(),
        });
    }
    let raw = std::fs::read_to_string(&file)?;
    let mut p: Profile = serde_json::from_str(&raw)?;
    if p.name.is_empty() {
        p.name = name.to_string();
    }
    Ok(p)
}

pub fn save(paths: &AppPaths, profile: &Profile) -> AppResult<()> {
    let dir = paths.profile_dir(&profile.name);
    std::fs::create_dir_all(dir.join("mods"))?;
    std::fs::write(
        profile_file(paths, &profile.name),
        serde_json::to_string_pretty(profile)?,
    )?;
    Ok(())
}

pub fn list(paths: &AppPaths) -> AppResult<Vec<String>> {
    let mut names = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&paths.profiles_dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() && entry.path().join("profile.json").exists() {
                names.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    if names.is_empty() {
        // Materialize the default profile.
        save(
            paths,
            &Profile {
                name: "Default".into(),
                mods: Vec::new(),
            },
        )?;
        names.push("Default".into());
    }
    names.sort();
    Ok(names)
}

pub fn create(paths: &AppPaths, name: &str) -> AppResult<String> {
    let clean = sanitize(name);
    if profile_file(paths, &clean).exists() {
        return Err(AppError::msg(format!("profile '{clean}' already exists")));
    }
    save(
        paths,
        &Profile {
            name: clean.clone(),
            mods: Vec::new(),
        },
    )?;
    Ok(clean)
}

/// Create a new empty profile, appending "(2)", "(3)", ... to `base_name` if
/// needed to avoid clashing with an existing profile.
pub fn create_unique(paths: &AppPaths, base_name: &str) -> AppResult<String> {
    let clean = sanitize(base_name);
    let unique = uniquify(paths, &clean);
    create(paths, &unique)
}

pub fn delete(paths: &AppPaths, name: &str) -> AppResult<()> {
    if name == "Default" {
        return Err(AppError::msg("the Default profile cannot be deleted"));
    }
    let dir = paths.profile_dir(name);
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}

pub fn clone_profile(paths: &AppPaths, from: &str, to: &str) -> AppResult<String> {
    let clean = sanitize(to);
    if profile_file(paths, &clean).exists() {
        return Err(AppError::msg(format!("profile '{clean}' already exists")));
    }
    let src = paths.profile_dir(from);
    let dst = paths.profile_dir(&clean);
    copy_tree(&src, &dst)?;
    // Rewrite the name inside the copied profile.json.
    let mut p = load(paths, &clean)?;
    p.name = clean.clone();
    save(paths, &p)?;
    Ok(clean)
}

// ---- Import / export ----------------------------------------------------

/// Export a profile as a portable zip bundle (includes the actual mod files).
pub fn export_bundle(state: &AppState, name: &str, dest_zip: &Path) -> AppResult<()> {
    let dir = state.paths.profile_dir(name);
    if !dir.exists() {
        return Err(AppError::msg(format!("profile '{name}' does not exist")));
    }
    let file = std::fs::File::create(dest_zip)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip_dir(&mut zip, &dir, &dir, &opts)?;
    zip.finish()?;
    Ok(())
}

/// Import a profile bundle produced by `export_bundle`.
pub fn import_bundle(state: &AppState, zip_path: &Path, new_name: Option<&str>) -> AppResult<String> {
    let tmp = state.paths.cache_dir.join("import-staging");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;
    crate::mods::extract_zip(zip_path, &tmp)?;

    let mut profile = load(&state.paths, "__staging__").unwrap_or_default();
    if let Ok(raw) = std::fs::read_to_string(tmp.join("profile.json")) {
        profile = serde_json::from_str(&raw)?;
    }
    let target = sanitize(new_name.unwrap_or(&profile.name));
    let target = uniquify(&state.paths, &target);
    let dst = state.paths.profile_dir(&target);
    copy_tree(&tmp, &dst)?;
    let _ = std::fs::remove_dir_all(&tmp);

    let mut p = load(&state.paths, &target)?;
    p.name = target.clone();
    save(&state.paths, &p)?;
    Ok(target)
}

/// A lightweight, shareable code listing Nexus mod/file refs (no files).
pub fn export_code(state: &AppState, name: &str) -> AppResult<String> {
    let profile = load(&state.paths, name)?;
    let refs: Vec<serde_json::Value> = profile
        .mods
        .iter()
        .map(|m| {
            serde_json::json!({
                "source": m.source,
                "mod_id": m.mod_id,
                "file_id": m.file_id,
                "name": m.name,
                "version": m.version,
                "enabled": m.enabled,
            })
        })
        .collect();
    let payload = serde_json::json!({
        "format": "scav-mod-manager/1",
        "profile": profile.name,
        "mods": refs,
    });
    Ok(serde_json::to_string_pretty(&payload)?)
}

// ---- helpers ------------------------------------------------------------

fn uniquify(paths: &AppPaths, base: &str) -> String {
    if !paths.profile_dir(base).exists() {
        return base.to_string();
    }
    for i in 2..1000 {
        let candidate = format!("{base} ({i})");
        if !paths.profile_dir(&candidate).exists() {
            return candidate;
        }
    }
    format!("{base}-imported")
}

fn copy_tree(src: &Path, dst: &Path) -> AppResult<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

fn zip_dir<W: std::io::Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    base: &Path,
    dir: &Path,
    opts: &zip::write::FileOptions<'_, ()>,
) -> AppResult<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let rel = path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
        if path.is_dir() {
            zip.add_directory(format!("{rel}/"), *opts)?;
            zip_dir(zip, base, &path, opts)?;
        } else {
            zip.start_file(rel, *opts)?;
            let bytes = std::fs::read(&path)?;
            zip.write_all(&bytes)?;
        }
    }
    Ok(())
}
