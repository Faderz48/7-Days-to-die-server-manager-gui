//! Persisted app-level settings — paths to the game install, custom
//! presets, etc. Stored as TOML in the user's config directory so they
//! survive restarts.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::paths;

/// Where we store the manager's own settings file.
fn settings_file() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("sdtd-server-manager")
        .join("settings.toml")
}

/// App-level settings (NOT the in-game server settings — those live in
/// the game's `serverconfig.xml`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// Folder containing the dedicated server executable.
    pub server_install_dir: Option<PathBuf>,

    /// Path to the `serverconfig.xml` we read & write.
    /// Defaults to `<install_dir>/serverconfig.xml`.
    pub server_config_path: Option<PathBuf>,

    /// Where the game stores save data (so we can list maps).
    pub saves_dir: Option<PathBuf>,

    /// Where the game stores generated random worlds.
    pub generated_worlds_dir: Option<PathBuf>,

    /// Where the manager keeps backups. Defaults to a folder beside the
    /// settings file.
    pub backup_dir: Option<PathBuf>,

    /// Named XML config presets the user has saved.
    #[serde(default)]
    pub presets: Vec<Preset>,

    /// Scheduled tasks (daily HH:MM).
    #[serde(default)]
    pub scheduled_tasks: Vec<crate::scheduler::ScheduledTask>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    /// XML body of the preset.
    pub xml: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        let install = paths::guess_server_install_dir();
        let config = install.as_ref().map(|p| p.join("serverconfig.xml"));
        let backup_dir = dirs::config_dir()
            .map(|p| p.join("sdtd-server-manager").join("backups"));
        Self {
            server_install_dir: install,
            server_config_path: config,
            saves_dir: paths::guess_saves_dir(),
            generated_worlds_dir: paths::guess_generated_worlds_dir(),
            backup_dir,
            presets: Vec::new(),
            scheduled_tasks: Vec::new(),
        }
    }
}

impl AppSettings {
    pub fn load_or_default() -> Result<Self> {
        let path = settings_file();
        if !path.exists() {
            let s = Self::default();
            s.save()?;
            return Ok(s);
        }
        let body = std::fs::read_to_string(&path)
            .with_context(|| format!("reading settings file {}", path.display()))?;
        let mut s: Self = toml::from_str(&body)
            .with_context(|| format!("parsing settings file {}", path.display()))?;

        // Fall back to guessed paths for any field the user didn't set.
        if s.server_install_dir.is_none() {
            s.server_install_dir = paths::guess_server_install_dir();
        }
        if s.server_config_path.is_none() {
            s.server_config_path = s
                .server_install_dir
                .as_ref()
                .map(|d| d.join("serverconfig.xml"));
        }
        if s.saves_dir.is_none() {
            s.saves_dir = paths::guess_saves_dir();
        }
        if s.generated_worlds_dir.is_none() {
            s.generated_worlds_dir = paths::guess_generated_worlds_dir();
        }
        if s.backup_dir.is_none() {
            s.backup_dir = dirs::config_dir()
                .map(|p| p.join("sdtd-server-manager").join("backups"));
        }
        Ok(s)
    }

    pub fn save(&self) -> Result<()> {
        let path = settings_file();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = toml::to_string_pretty(self)?;
        std::fs::write(&path, body)
            .with_context(|| format!("writing settings file {}", path.display()))?;
        Ok(())
    }

    /// Resolve the server executable inside the install dir.
    pub fn resolve_executable(&self) -> Option<PathBuf> {
        self.server_install_dir
            .as_ref()
            .map(|d| d.join(paths::server_executable_name()))
    }

    /// Resolve the path of the serverconfig.xml we're managing.
    pub fn resolve_config_path(&self) -> Option<PathBuf> {
        self.server_config_path.clone().or_else(|| {
            self.server_install_dir
                .as_ref()
                .map(|d| d.join("serverconfig.xml"))
        })
    }

    pub fn settings_file_path() -> PathBuf {
        settings_file()
    }
}
