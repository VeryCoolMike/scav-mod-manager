use crate::error::{AppError, AppResult};
use crate::profiles::{self, ProfileMod};
use crate::state::AppState;
use futures_util::StreamExt;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

/// A mod as tracked in a profile (returned to the frontend).
pub type InstalledMod = ProfileMod;

/// Top-level folder names that map into a BepInEx layout.
const SPECIAL_DIRS: [&str; 5] = ["bepinex", "plugins", "patchers", "config", "core"];

// ---- HTTP download ------------------------------------------------------

/// Stream a URL to a file on disk.
pub async fn download_url(client: &reqwest::Client, url: &str, dest: &Path) -> AppResult<()> {
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(AppError::msg(format!(
            "download failed ({}) for {url}",
            resp.status()
        )));
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::File::create(dest)?;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk)?;
    }
    file.flush()?;
    Ok(())
}

// ---- Archive extraction -------------------------------------------------

/// Extract a zip/7z/rar archive into `dest_root`.
pub fn extract_archive(archive: &Path, dest_root: &Path) -> AppResult<()> {
    let magic = read_magic(archive)?;
    if magic.starts_with(b"PK") {
        extract_zip(archive, dest_root)
    } else if magic.starts_with(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]) {
        extract_7z(archive, dest_root)
    } else if magic.starts_with(b"Rar!") {
        extract_rar(archive, dest_root)
    } else {
        // Fall back to extension.
        match archive.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()) {
            Some(ref e) if e == "zip" => extract_zip(archive, dest_root),
            Some(ref e) if e == "7z" => extract_7z(archive, dest_root),
            Some(ref e) if e == "rar" => extract_rar(archive, dest_root),
            _ => Err(AppError::msg("unrecognized archive format")),
        }
    }
}

pub fn extract_zip(archive: &Path, dest_root: &Path) -> AppResult<()> {
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file)?;
    std::fs::create_dir_all(dest_root)?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let Some(rel) = entry.enclosed_name() else {
            continue; // reject path traversal
        };
        let out = dest_root.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut f = std::fs::File::create(&out)?;
            std::io::copy(&mut entry, &mut f)?;
        }
    }
    Ok(())
}

pub fn extract_7z(archive: &Path, dest_root: &Path) -> AppResult<()> {
    std::fs::create_dir_all(dest_root)?;
    sevenz_rust::decompress_file(archive, dest_root)
        .map_err(|e| AppError::msg(format!("7z extraction failed: {e}")))
}

/// Extract a RAR archive using the bundled unrar decoder (statically linked
/// via `unrar-ng` — no external 7z/unrar binary needs to be installed).
pub fn extract_rar(archive: &Path, dest_root: &Path) -> AppResult<()> {
    std::fs::create_dir_all(dest_root)?;
    let opened = unrar_ng::Archive::new(archive)
        .open_for_processing()
        .map_err(|e| AppError::msg(format!("failed to open RAR archive: {e}")))?;
    opened
        .extract_all(dest_root)
        .map_err(|e| AppError::msg(format!("RAR extraction failed: {e}")))
}

fn read_magic(path: &Path) -> AppResult<[u8; 8]> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut buf = [0u8; 8];
    let n = f.read(&mut buf)?;
    if n < 4 {
        return Err(AppError::msg("archive is too small / empty"));
    }
    Ok(buf)
}

// ---- Install ------------------------------------------------------------

/// Everything needed to record an installed mod, independent of source.
#[derive(Debug, Clone)]
pub struct ModMeta {
    pub source: String,
    pub mod_id: u32,
    pub file_id: u64,
    pub name: String,
    pub version: String,
    pub author: Option<String>,
    pub picture_url: Option<String>,
    pub page_url: Option<String>,
}

impl ModMeta {
    /// Stable per-source key, also used as the on-disk folder name.
    pub fn key(&self) -> String {
        format!("{}-{}-{}", self.source, self.mod_id, self.file_id)
    }
}

/// Download an archive from `url` and install it into the active profile.
pub async fn install_from_url(state: &AppState, url: &str, meta: ModMeta) -> AppResult<InstalledMod> {
    let tmp = state.paths.cache_dir.join(format!("{}.archive", meta.key()));
    download_url(&state.http, url, &tmp).await?;
    let result = install_local_archive(state, &tmp, &meta);
    let _ = std::fs::remove_file(&tmp);
    result
}

/// Install a mod from an already-downloaded archive on disk.
pub fn install_local_archive(
    state: &AppState,
    archive: &Path,
    meta: &ModMeta,
) -> AppResult<InstalledMod> {
    let profile_name = state.settings().active_profile;
    let key = meta.key();

    // Extract to a staging dir, then normalize into the profile mod folder.
    let staging = state.paths.cache_dir.join(format!("staging-{key}"));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    extract_archive(archive, &staging)?;

    let mods_dir = profiles::mods_dir(&state.paths, &profile_name);
    let dest = mods_dir.join(&key);
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::create_dir_all(&dest)?;
    normalize(&staging, &dest, &meta.name)?;
    let _ = std::fs::remove_dir_all(&staging);

    // Record in the profile (replace any prior version of the same mod).
    let mut profile = profiles::load(&state.paths, &profile_name)?;
    let entry = ProfileMod {
        key: key.clone(),
        source: meta.source.clone(),
        mod_id: meta.mod_id,
        file_id: meta.file_id,
        name: meta.name.clone(),
        version: meta.version.clone(),
        author: meta.author.clone(),
        picture_url: meta.picture_url.clone(),
        page_url: meta.page_url.clone(),
        enabled: true,
    };
    // Remove older files of the same mod (a re-install / update).
    profile.mods.retain(|m| {
        if m.source == meta.source && m.mod_id == meta.mod_id && m.key != key {
            let _ = std::fs::remove_dir_all(mods_dir.join(&m.key));
            false
        } else {
            m.key != key
        }
    });
    profile.mods.push(entry.clone());
    profiles::save(&state.paths, &profile)?;

    Ok(entry)
}

/// Does `dir` directly contain a BepInEx-style folder (bepinex/plugins/patchers/config/core)?
fn has_bepinex_layout(entries: &[(String, bool)]) -> bool {
    entries.iter().any(|(n, is_dir)| {
        *is_dir
            && (n.eq_ignore_ascii_case("bepinex") || SPECIAL_DIRS.contains(&n.to_lowercase().as_str()))
    })
}

/// Some mods wrap their real BepInEx layout inside an inner folder alongside
/// loose docs (e.g. `CHANGELOG.txt`, `mod/BepInEx/plugins/...`), so it isn't
/// visible at the top level `unwrap_single_folder` already checked. Search a
/// few levels deep for the first folder that directly contains a BepInEx-style
/// layout.
fn find_bepinex_root(dir: &Path, depth: u32) -> Option<PathBuf> {
    let entries = list_entries(dir).ok()?;
    if has_bepinex_layout(&entries) {
        return Some(dir.to_path_buf());
    }
    if depth == 0 {
        return None;
    }
    for (name, is_dir) in &entries {
        if *is_dir {
            if let Some(found) = find_bepinex_root(&dir.join(name), depth - 1) {
                return Some(found);
            }
        }
    }
    None
}

/// Recursively search (bounded depth) for a directory literally named `name`.
fn find_named_folder(dir: &Path, name: &str, depth: u32) -> Option<PathBuf> {
    let entries = list_entries(dir).ok()?;
    for (n, is_dir) in &entries {
        if *is_dir && n.eq_ignore_ascii_case(name) {
            return Some(dir.join(n));
        }
    }
    if depth == 0 {
        return None;
    }
    for (n, is_dir) in &entries {
        if *is_dir {
            if let Some(found) = find_named_folder(&dir.join(n), name, depth - 1) {
                return Some(found);
            }
        }
    }
    None
}

/// Normalize an extracted mod tree into a game-root-relative layout under `dest`.
fn normalize(staging: &Path, dest: &Path, mod_name: &str) -> AppResult<()> {
    let unwrapped = unwrap_single_folder(staging.to_path_buf());

    // A folder literally named "mod" is a packaging convention meaning "copy
    // everything in here straight into the game's install root" - files
    // included (e.g. steam_appid.txt), not just a BepInEx tree. This is
    // distinct from a plain BepInEx-only archive, so it takes priority.
    if let Some(mod_root) = find_named_folder(&unwrapped, "mod", 3) {
        copy_dir_merge(&mod_root, dest)?;
        return Ok(());
    }

    let root = find_bepinex_root(&unwrapped, 4).unwrap_or_else(|| unwrapped.clone());
    let entries = list_entries(&root)?;

    if has_bepinex_layout(&entries) {
        for (name, is_dir) in &entries {
            if !is_dir {
                continue; // skip loose readmes / loader dlls at the top
            }
            let lower = name.to_lowercase();
            let target = match lower.as_str() {
                "bepinex" => dest.join("BepInEx"),
                "plugins" => dest.join("BepInEx").join("plugins"),
                "patchers" => dest.join("BepInEx").join("patchers"),
                "config" => dest.join("BepInEx").join("config"),
                "core" => dest.join("BepInEx").join("core"),
                _ => continue,
            };
            copy_dir_merge(&root.join(name), &target)?;
        }
    } else {
        // Loose DLLs / assets: drop everything under BepInEx/plugins/<mod_name>.
        let target = dest
            .join("BepInEx")
            .join("plugins")
            .join(crate::state::sanitize(mod_name));
        copy_dir_merge(&unwrapped, &target)?;
    }
    Ok(())
}

/// Descend through single-folder wrappers (e.g. archive/ModName/BepInEx/...).
fn unwrap_single_folder(mut dir: PathBuf) -> PathBuf {
    loop {
        let Ok(entries) = list_entries(&dir) else {
            return dir;
        };
        if entries.len() == 1 && entries[0].1 {
            let name = &entries[0].0;
            if SPECIAL_DIRS.contains(&name.to_lowercase().as_str()) {
                return dir; // don't descend into a meaningful folder
            }
            dir = dir.join(name);
        } else {
            return dir;
        }
    }
}

fn list_entries(dir: &Path) -> AppResult<Vec<(String, bool)>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = entry.path().is_dir();
        out.push((name, is_dir));
    }
    Ok(out)
}

fn copy_dir_merge(src: &Path, dst: &Path) -> AppResult<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_merge(&from, &to)?;
        } else {
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

// ---- Profile mod operations --------------------------------------------

pub fn installed_list(state: &AppState) -> AppResult<Vec<InstalledMod>> {
    let profile = profiles::load(&state.paths, &state.settings().active_profile)?;
    Ok(profile.mods)
}

pub fn set_enabled(state: &AppState, key: &str, enabled: bool) -> AppResult<()> {
    let name = state.settings().active_profile;
    let mut profile = profiles::load(&state.paths, &name)?;
    let mut found = false;
    for m in profile.mods.iter_mut() {
        if m.key == key {
            m.enabled = enabled;
            found = true;
        }
    }
    if !found {
        return Err(AppError::msg(format!("mod '{key}' not found in profile")));
    }
    profiles::save(&state.paths, &profile)?;
    Ok(())
}

pub fn uninstall(state: &AppState, key: &str) -> AppResult<()> {
    let name = state.settings().active_profile;
    let mut profile = profiles::load(&state.paths, &name)?;
    profile.mods.retain(|m| m.key != key);
    profiles::save(&state.paths, &profile)?;
    let dir = profiles::mods_dir(&state.paths, &name).join(key);
    let _ = std::fs::remove_dir_all(dir);
    Ok(())
}

// ---- Sync enabled mods into the game -----------------------------------

/// A file written into the game folder by a mod sync.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SyncedFile {
    rel: String,
    /// True if this write overwrote a pre-existing game file, in which case
    /// the original is backed up under the profile's `backups/` folder and
    /// must be restored (not deleted) when un-syncing.
    overwrote: bool,
}

/// Copy all enabled mods of the active profile into the game folder, cleaning
/// up (and restoring any overwritten files from) a previous run. Returns the
/// number of files written.
pub fn sync_to_game(state: &AppState) -> AppResult<usize> {
    let game = state.require_game_path()?;
    let name = state.settings().active_profile;
    let profile = profiles::load(&state.paths, &name)?;
    let meta_dir = profiles::profile_meta(&state.paths, &name);
    let manifest_path = meta_dir.join(".synced.json");
    let backups_dir = meta_dir.join("backups");

    // Undo the previous sync: restore any game file a mod overwrote, delete
    // any file a mod newly added.
    if let Ok(raw) = std::fs::read_to_string(&manifest_path) {
        if let Ok(prev) = serde_json::from_str::<Vec<SyncedFile>>(&raw) {
            for entry in prev.iter().rev() {
                let p = game.join(&entry.rel);
                if entry.overwrote {
                    let backup = backups_dir.join(&entry.rel);
                    if backup.is_file() {
                        if let Some(parent) = p.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        std::fs::copy(&backup, &p)?;
                        let _ = std::fs::remove_file(&backup);
                    }
                } else if p.is_file() {
                    let _ = std::fs::remove_file(&p);
                }
            }
            // Best-effort cleanup of now-empty directories.
            for entry in prev.iter().rev() {
                if let Some(parent) = Path::new(&entry.rel).parent() {
                    let _ = std::fs::remove_dir(game.join(parent));
                    let _ = std::fs::remove_dir(backups_dir.join(parent));
                }
            }
        }
    }

    // Copy enabled mods, backing up any pre-existing game file before a mod
    // overwrites it, and recording relative paths.
    let mods_dir = profiles::mods_dir(&state.paths, &name);
    let mut written: Vec<SyncedFile> = Vec::new();
    for m in profile.mods.iter().filter(|m| m.enabled) {
        let src = mods_dir.join(&m.key);
        collect_and_copy(&src, &src, &game, &backups_dir, &mut written)?;
    }

    std::fs::create_dir_all(&meta_dir)?;
    std::fs::write(&manifest_path, serde_json::to_string_pretty(&written)?)?;
    Ok(written.len())
}

fn collect_and_copy(
    base: &Path,
    src: &Path,
    game: &Path,
    backups_dir: &Path,
    written: &mut Vec<SyncedFile>,
) -> AppResult<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        if from.is_dir() {
            collect_and_copy(base, &from, game, backups_dir, written)?;
        } else {
            let rel = from.strip_prefix(base).unwrap();
            if is_protected(rel) {
                continue; // never overwrite the loader itself
            }
            let to = game.join(rel);
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            // Back up a pre-existing game file before overwriting it, so it
            // can be restored when the mod is disabled/uninstalled. Skip if
            // a backup already exists (e.g. two mods touch the same path in
            // this same sync) so we never overwrite the true original.
            let overwrote = to.is_file();
            if overwrote {
                let backup = backups_dir.join(rel);
                if !backup.is_file() {
                    if let Some(parent) = backup.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::copy(&to, &backup)?;
                }
            }
            std::fs::copy(&from, &to)?;
            written.push(SyncedFile { rel: rel_to_string(rel), overwrote });
        }
    }
    Ok(())
}

/// Loader files that a mod must never overwrite/remove.
fn is_protected(rel: &Path) -> bool {
    let s = rel_to_string(rel).to_lowercase();
    s == "winhttp.dll"
        || s == "winhttp.dll.disabled"
        || s == "doorstop_config.ini"
        || s == ".doorstop_version"
        || s.starts_with("bepinex/core/")
}

fn rel_to_string(rel: &Path) -> String {
    rel.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("smm-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }

    fn test_state(root: &Path, game_path: &Path) -> AppState {
        let data_dir = root.join("data");
        let paths = crate::state::AppPaths {
            profiles_dir: data_dir.join("profiles"),
            cache_dir: data_dir.join("cache"),
            data_dir,
        };
        std::fs::create_dir_all(&paths.profiles_dir).unwrap();
        std::fs::create_dir_all(&paths.cache_dir).unwrap();
        let mut settings = crate::state::Settings::default();
        settings.active_profile = "Default".to_string();
        settings.game_path = Some(game_path.to_string_lossy().into_owned());
        AppState {
            settings: std::sync::Mutex::new(settings),
            paths,
            http: reqwest::Client::new(),
        }
    }

    #[test]
    fn sync_backs_up_and_restores_a_game_file_a_mod_overwrites() {
        let root = tmp("sync");
        let game = root.join("game");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("data.bin"), b"vanilla").unwrap();

        let state = test_state(&root, &game);
        let mods_dir = profiles::mods_dir(&state.paths, "Default");
        touch(&mods_dir.join("test-1/data.bin"));
        std::fs::write(mods_dir.join("test-1/data.bin"), b"modded").unwrap();

        let mut profile = profiles::load(&state.paths, "Default").unwrap();
        profile.mods.push(ProfileMod {
            key: "test-1".into(),
            source: "nexus".into(),
            mod_id: 1,
            file_id: 1,
            name: "Test".into(),
            version: "1.0".into(),
            author: None,
            picture_url: None,
            page_url: None,
            enabled: true,
        });
        profiles::save(&state.paths, &profile).unwrap();

        sync_to_game(&state).unwrap();
        assert_eq!(std::fs::read(game.join("data.bin")).unwrap(), b"modded");

        // Disabling the mod and re-syncing must restore the real game file,
        // not just delete it.
        let mut profile = profiles::load(&state.paths, "Default").unwrap();
        profile.mods[0].enabled = false;
        profiles::save(&state.paths, &profile).unwrap();

        sync_to_game(&state).unwrap();
        assert_eq!(std::fs::read(game.join("data.bin")).unwrap(), b"vanilla");
    }

    #[test]
    fn normalize_bepinex_wrapped_in_folder() {
        let staging = tmp("bep");
        touch(&staging.join("MyMod/BepInEx/plugins/Foo.dll"));
        let dest = tmp("bep-dest");
        normalize(&staging, &dest, "MyMod").unwrap();
        assert!(dest.join("BepInEx/plugins/Foo.dll").exists());
    }

    #[test]
    fn normalize_mod_folder_copies_straight_to_game_root() {
        // Real-world layout (Nexus mod "Casualties Together"): loose docs at
        // the top level alongside a "mod" folder. A literal "mod" folder
        // means "copy this whole folder into the game's install root" -
        // including loose files like steam_appid.txt, not just the nested
        // BepInEx/plugins + BepInEx/patchers tree.
        let staging = tmp("nested");
        touch(&staging.join("CHANGELOG.txt"));
        touch(&staging.join("INSTALLATION.txt"));
        touch(&staging.join("mod/steam_appid.txt"));
        touch(&staging.join("mod/BepInEx/plugins/KrokMP/KrokoshaCasualtiesMP.dll"));
        touch(&staging.join("mod/BepInEx/patchers/KrokMP/autoupdater_patcher.dll"));
        let dest = tmp("nested-dest");
        normalize(&staging, &dest, "Casualties Together").unwrap();
        assert!(dest.join("steam_appid.txt").exists());
        assert!(dest.join("BepInEx/plugins/KrokMP/KrokoshaCasualtiesMP.dll").exists());
        assert!(dest.join("BepInEx/patchers/KrokMP/autoupdater_patcher.dll").exists());
        // Loose docs alongside the "mod" folder are not part of the payload.
        assert!(!dest.join("CHANGELOG.txt").exists());
    }

    #[test]
    fn normalize_bare_plugins_folder() {
        let staging = tmp("plug");
        touch(&staging.join("plugins/Bar.dll"));
        let dest = tmp("plug-dest");
        normalize(&staging, &dest, "BarMod").unwrap();
        assert!(dest.join("BepInEx/plugins/Bar.dll").exists());
    }

    #[test]
    fn normalize_loose_dll_goes_under_mod_name() {
        let staging = tmp("loose");
        touch(&staging.join("Loose.dll"));
        let dest = tmp("loose-dest");
        normalize(&staging, &dest, "Loose Mod").unwrap();
        assert!(dest.join("BepInEx/plugins/Loose Mod/Loose.dll").exists());
    }

    #[test]
    fn loader_files_are_protected() {
        assert!(is_protected(Path::new("winhttp.dll")));
        assert!(is_protected(Path::new("BepInEx/core/BepInEx.dll")));
        assert!(!is_protected(Path::new("BepInEx/plugins/Mod.dll")));
    }

    fn find_dll(dir: &Path) -> bool {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let p = entry.path();
            if p.is_dir() {
                if find_dll(&p) {
                    return true;
                }
            } else if p.extension().and_then(|e| e.to_str()) == Some("dll") {
                return true;
            }
        }
        false
    }

    // Real account-free download from GameBanana → extract → normalize.
    // Run with: cargo test -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn gamebanana_download_extract_normalize() {
        let client = reqwest::Client::builder().build().unwrap();
        let archive = tmp("gb-dl").join("mod.zip");
        // "Custom Structures" standalone .dll file (mod 684563, file 1724143).
        download_url(&client, "https://gamebanana.com/dl/1724143", &archive)
            .await
            .expect("anonymous download should succeed");
        assert!(std::fs::metadata(&archive).unwrap().len() > 1000);

        let staging = tmp("gb-staging");
        extract_archive(&archive, &staging).expect("extract");
        let dest = tmp("gb-dest");
        normalize(&staging, &dest, "Custom Structures").expect("normalize");

        let plugins = dest.join("BepInEx").join("plugins");
        assert!(plugins.exists(), "expected BepInEx/plugins to be created");
        assert!(find_dll(&plugins), "expected a plugin .dll under BepInEx/plugins");
    }
}
