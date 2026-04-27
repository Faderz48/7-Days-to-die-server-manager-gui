//! Save-game backups. We don't compress (keeps it simple, cross-platform,
//! and 7DTD save folders are already mostly compressed binary). Each
//! backup is a directory under `<backup_dir>/<save_name>/<timestamp>/`,
//! with a `meta.json` describing what was captured.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupEntry {
    pub save_name: String,
    pub timestamp: DateTime<Utc>,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub note: Option<String>,
}

fn meta_file(p: &Path) -> PathBuf { p.join("meta.json") }

fn read_meta(p: &Path) -> Option<BackupEntry> {
    let body = fs::read_to_string(meta_file(p)).ok()?;
    serde_json::from_str(&body).ok()
}

/// List all backups in the manager's backup directory, newest first.
pub fn list(backup_dir: &Path) -> Result<Vec<BackupEntry>> {
    let mut out = Vec::new();
    if !backup_dir.exists() { return Ok(out); }

    for save_dir in fs::read_dir(backup_dir)? {
        let save_dir = save_dir?;
        if !save_dir.file_type()?.is_dir() { continue; }
        for ts_dir in fs::read_dir(save_dir.path())? {
            let ts_dir = ts_dir?;
            if !ts_dir.file_type()?.is_dir() { continue; }
            if let Some(entry) = read_meta(&ts_dir.path()) {
                out.push(entry);
            }
        }
    }
    out.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(out)
}

/// Make a backup of the supplied save folder.
pub fn create(
    saves_dir: &Path,
    save_name: &str,
    backup_dir: &Path,
    note: Option<String>,
) -> Result<BackupEntry> {
    let src = saves_dir.join(save_name);
    if !src.exists() {
        bail!("save folder not found: {}", src.display());
    }
    let now: DateTime<Utc> = Utc::now();
    let stamp = now.format("%Y-%m-%d_%H-%M-%S").to_string();
    let dst = backup_dir.join(save_name).join(&stamp);
    fs::create_dir_all(&dst)?;

    let payload = dst.join("data");
    copy_recursive(&src, &payload).context("copying save folder")?;

    let size = dir_size(&payload).unwrap_or(0);
    let entry = BackupEntry {
        save_name: save_name.into(),
        timestamp: now,
        path: dst.clone(),
        size_bytes: size,
        note,
    };
    fs::write(meta_file(&dst), serde_json::to_string_pretty(&entry)?)?;
    Ok(entry)
}

/// Delete a backup directory.
pub fn delete(backup_path: &Path) -> Result<()> {
    if !backup_path.exists() {
        bail!("backup path missing: {}", backup_path.display());
    }
    if !backup_path.join("meta.json").exists() {
        bail!("refusing to delete: not a backup directory ({})", backup_path.display());
    }
    fs::remove_dir_all(backup_path)?;
    Ok(())
}

/// Restore a backup over the live save folder. Renames the existing one
/// aside as `<name>.replaced-<ts>` instead of deleting it, so a botched
/// restore is recoverable.
pub fn restore(saves_dir: &Path, backup_path: &Path) -> Result<()> {
    let entry = read_meta(backup_path)
        .ok_or_else(|| anyhow!("backup has no meta.json"))?;
    let payload = backup_path.join("data");
    if !payload.exists() {
        bail!("backup payload missing at {}", payload.display());
    }
    let target = saves_dir.join(&entry.save_name);
    if target.exists() {
        let stamp = Utc::now().format("%Y-%m-%d_%H-%M-%S").to_string();
        let aside = saves_dir.join(format!("{}.replaced-{}", entry.save_name, stamp));
        fs::rename(&target, &aside)
            .with_context(|| format!("moving aside existing save to {}", aside.display()))?;
    }
    copy_recursive(&payload, &target).context("restoring backup payload")?;
    Ok(())
}

/// Recursive directory copy. Cross-platform.
fn copy_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_recursive(&from, &to)?;
        } else {
            fs::copy(&from, &to)
                .with_context(|| format!("copying {} -> {}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

fn dir_size(p: &Path) -> Result<u64> {
    let mut total = 0u64;
    for entry in fs::read_dir(p)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            total += dir_size(&entry.path()).unwrap_or(0);
        } else {
            total += entry.metadata()?.len();
        }
    }
    Ok(total)
}
