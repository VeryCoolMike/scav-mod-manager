use crate::error::{AppError, AppResult};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The Nexus Mods "domain name" for Casualties: Unknown / Scav Prototype.
pub const GAME_DOMAIN: &str = "scavprototype";
/// Known Steam AppIDs for Casualties: Unknown - the full game and its
/// separately-listed free demo each get their own AppID on Steam.
pub const STEAM_APPIDS: &[&str] = &["4576490", "4576510"];
/// The game's main executable file name.
pub const GAME_EXE: &str = "CasualtiesUnknown.exe";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Settings {
    /// Absolute path to the game install folder (contains CasualtiesUnknown.exe).
    pub game_path: Option<String>,
    /// How the game was located: "steam" | "itch" | "manual".
    pub game_source: Option<String>,
    /// Steam AppID to use for `-applaunch` when `game_source` is "steam".
    /// Remembered from detection since the demo and full game differ.
    pub steam_appid: Option<String>,
    /// Personal Nexus Mods API key (from nexusmods.com/users/myaccount?tab=api).
    pub nexus_api_key: Option<String>,
    /// Whether the validated key belongs to a Premium account.
    pub is_premium: bool,
    /// Nexus user name from the last successful validation.
    pub nexus_user: Option<String>,
    /// Currently active profile name.
    pub active_profile: String,
    /// Custom launch command for Linux (e.g. a wrapper script). Optional.
    pub linux_launch: Option<String>,
    /// Whether setup has been completed at least once.
    pub setup_complete: bool,
    /// Temporary account: email from emailnator.
    pub temp_email: Option<String>,
    /// Temporary account: randomly-generated username.
    pub temp_username: Option<String>,
    /// Temporary account: randomly-generated password.
    pub temp_password: Option<String>,
}

impl Settings {
    fn ensure_defaults(&mut self) {
        if self.active_profile.trim().is_empty() {
            self.active_profile = "Default".to_string();
        }
    }
}

/// Resolved on-disk locations used by the app.
#[derive(Debug, Clone)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub profiles_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl AppPaths {
    pub fn resolve() -> AppResult<Self> {
        let proj = ProjectDirs::from("org", "scavprototype", "scav-mod-manager")
            .ok_or_else(|| AppError::msg("could not resolve a home/app-data directory"))?;
        let data_dir = proj.data_dir().to_path_buf();
        let profiles_dir = data_dir.join("profiles");
        let cache_dir = data_dir.join("cache");
        std::fs::create_dir_all(&profiles_dir)?;
        std::fs::create_dir_all(&cache_dir)?;
        Ok(Self {
            data_dir,
            profiles_dir,
            cache_dir,
        })
    }

    pub fn settings_file(&self) -> PathBuf {
        self.data_dir.join("settings.json")
    }

    pub fn profile_dir(&self, name: &str) -> PathBuf {
        self.profiles_dir.join(sanitize(name))
    }
}

/// Shared, thread-safe application state managed by Tauri.
pub struct AppState {
    pub settings: Mutex<Settings>,
    pub paths: AppPaths,
    pub http: reqwest::Client,
}

impl AppState {
    pub fn init() -> AppResult<Self> {
        let paths = AppPaths::resolve()?;
        let settings = load_settings(&paths)?;
        let http = reqwest::Client::builder()
            .user_agent(format!(
                "ScavModManager/{} (+https://github.com/scavprototype)",
                env!("CARGO_PKG_VERSION")
            ))
            .build()?;
        Ok(Self {
            settings: Mutex::new(settings),
            paths,
            http,
        })
    }

    /// Snapshot of the current settings (cloned out of the lock).
    pub fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    /// Replace and persist settings.
    pub fn save_settings(&self, mut new: Settings) -> AppResult<Settings> {
        new.ensure_defaults();
        {
            let mut guard = self.settings.lock().unwrap();
            *guard = new.clone();
        }
        persist_settings(&self.paths, &new)?;
        Ok(new)
    }

    pub fn require_game_path(&self) -> AppResult<PathBuf> {
        let s = self.settings();
        let p = s.game_path.ok_or(AppError::NoGamePath)?;
        Ok(PathBuf::from(p))
    }

    pub fn require_api_key(&self) -> AppResult<String> {
        let s = self.settings();
        s.nexus_api_key
            .filter(|k| !k.trim().is_empty())
            .ok_or(AppError::NoApiKey)
    }
}

fn load_settings(paths: &AppPaths) -> AppResult<Settings> {
    let file = paths.settings_file();
    if !file.exists() {
        let mut s = Settings::default();
        s.ensure_defaults();
        return Ok(s);
    }
    let raw = std::fs::read_to_string(&file)?;
    let mut s: Settings = serde_json::from_str(&raw).unwrap_or_default();
    s.ensure_defaults();
    Ok(s)
}

fn persist_settings(paths: &AppPaths, s: &Settings) -> AppResult<()> {
    let raw = serde_json::to_string_pretty(s)?;
    std::fs::write(paths.settings_file(), raw)?;
    Ok(())
}

/// Make a string safe to use as a directory name.
pub fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.is_empty() {
        "unnamed".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Validate that a folder looks like a Casualties: Unknown install.
pub fn looks_like_game(dir: &Path) -> bool {
    dir.join(GAME_EXE).exists() || dir.join("CasualtiesUnknown_Data").exists()
}
