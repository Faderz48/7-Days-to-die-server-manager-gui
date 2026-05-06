//! Cross-platform helpers for guessing where the 7DTD dedicated server
//! installation and save folders live. Best-effort defaults — the user
//! can always override them in the UI.

use std::path::PathBuf;

/// Try to find the most likely path to the dedicated server executable
/// for the current OS. Returns `None` if no candidate exists on disk.
pub fn guess_server_install_dir() -> Option<PathBuf> {
    let candidates: Vec<PathBuf> = if cfg!(target_os = "windows") {
        vec![
            PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\7 Days to Die Dedicated Server"),
            PathBuf::from(r"C:\Program Files\Steam\steamapps\common\7 Days to Die Dedicated Server"),
            PathBuf::from(r"C:\steamcmd\steamapps\common\7 Days to Die Dedicated Server"),
        ]
    } else if cfg!(target_os = "macos") {
        vec![]
    } else {
        // Linux — covers vanilla Steam, Flatpak Steam (com.valvesoftware.Steam),
        // and snap Steam, plus a couple of common system-wide install paths.
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        vec![
            home.join("Steam/steamapps/common/7 Days to Die Dedicated Server"),
            home.join(".steam/steam/steamapps/common/7 Days to Die Dedicated Server"),
            home.join(".local/share/Steam/steamapps/common/7 Days to Die Dedicated Server"),
            home.join(".var/app/com.valvesoftware.Steam/data/Steam/steamapps/common/7 Days to Die Dedicated Server"),
            home.join("snap/steam/common/.local/share/Steam/steamapps/common/7 Days to Die Dedicated Server"),
            PathBuf::from("/srv/7dtd"),
            PathBuf::from("/opt/7dtd-server"),
        ]
    };

    candidates.into_iter().find(|p| p.exists())
}

/// Name of the executable / launch script for the current OS.
pub fn server_executable_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "7DaysToDieServer.exe"
    } else {
        "startserver.sh"
    }
}

/// Where 7DTD stores generated worlds & save data, by convention.
pub fn guess_saves_dir() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        // %APPDATA%\7DaysToDie\Saves
        dirs::data_dir().map(|p| p.join("7DaysToDie").join("Saves"))
    } else {
        // ~/.local/share/7DaysToDie/Saves
        dirs::data_local_dir().map(|p| p.join("7DaysToDie").join("Saves"))
    }
}

/// Where 7DTD stores generated random world maps.
pub fn guess_generated_worlds_dir() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        dirs::data_dir().map(|p| p.join("7DaysToDie").join("GeneratedWorlds"))
    } else {
        dirs::data_local_dir().map(|p| p.join("7DaysToDie").join("GeneratedWorlds"))
    }
}
