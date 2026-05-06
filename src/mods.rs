//! Mod (modlet) management. The 7 Days to Die server loads anything in
//! the `Mods/` subdirectory of the install at startup, so installing a
//! mod amounts to dropping its folder in there.
//!
//! What this module handles:
//!   - Listing what's currently installed
//!   - Accepting a `.zip` upload and extracting it under `Mods/` with
//!     path-traversal protection
//!   - Auto-detecting the layout: most mods ship as either
//!       (a) `MyMod/ModInfo.xml + ...`             — pre-wrapped
//!       (b) `MyMod-1.2.3/MyMod/ModInfo.xml + ...` — wrapped twice
//!       (c) loose files at the zip root          — needs to be wrapped
//!     We unwrap layer (b) when we can detect it, and synthesize a
//!     wrapper folder for (c).
//!   - Deleting an installed mod
//!
//! Caller's responsibility: stopping the server before installing or
//! deleting (the running server pins the loaded files on Windows; on
//! Linux it just causes weird mid-game crashes).

use std::fs::{self, File};
use std::io::{self, Read, Seek};
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use zip::ZipArchive;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModEntry {
    pub name: String,
    pub size_bytes: u64,
    /// Whether the directory contains a recognizable `ModInfo.xml`
    /// (or `modinfo.xml`). Lets the UI flag broken installs.
    pub has_modinfo: bool,
    pub modified_iso: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallResult {
    pub mod_name: String,
    pub bytes_extracted: u64,
    pub file_count: u64,
    /// The shape we matched on at install time, useful for logs.
    pub layout: String,
}

/// Resolve the `Mods/` directory inside the configured server install.
/// Creates it if it doesn't exist (a fresh server install won't have
/// `Mods/` until something gets installed).
pub fn resolve_mods_dir(install_dir: &Path) -> Result<PathBuf> {
    let mods = install_dir.join("Mods");
    if !mods.exists() {
        fs::create_dir_all(&mods)
            .with_context(|| format!("creating {}", mods.display()))?;
    }
    Ok(mods)
}

/// List everything in `Mods/`. Each direct subdirectory is a mod.
pub fn list_mods(mods_dir: &Path) -> Result<Vec<ModEntry>> {
    if !mods_dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(mods_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = match entry.file_name().to_str() {
            Some(s) => s.to_string(),
            None => continue,
        };
        let path = entry.path();
        let has_modinfo = path.join("ModInfo.xml").exists()
                       || path.join("modinfo.xml").exists();
        let size = dir_size(&path).unwrap_or(0);
        let modified_iso = entry.metadata().ok()
            .and_then(|m| m.modified().ok())
            .map(|t| {
                let dt: chrono::DateTime<chrono::Utc> = t.into();
                dt.to_rfc3339()
            });
        out.push(ModEntry { name, size_bytes: size, has_modinfo, modified_iso });
    }
    out.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    Ok(out)
}

/// Extract a mod zip into `mods_dir`. The trickiest part is figuring
/// out where in the zip the actual mod folder lives. See module doc.
pub fn install_zip(
    archive_path: &Path,
    mods_dir: &Path,
    forced_name: Option<&str>,
    overwrite: bool,
) -> Result<InstallResult> {
    let f = File::open(archive_path)
        .with_context(|| format!("opening uploaded archive {}", archive_path.display()))?;
    let mut zip = ZipArchive::new(f).context("zip is malformed or not a zip file")?;

    // ── Decide the layout & target name ───────────────────────────────
    let layout = detect_layout(&mut zip)?;
    let mod_name = forced_name
        .map(sanitize_name)
        .filter(|s| !s.is_empty())
        .or_else(|| layout.suggested_name())
        .or_else(|| {
            // Last-ditch: use the zip's filename stem.
            archive_path.file_stem()
                .and_then(|s| s.to_str())
                .map(sanitize_name)
        })
        .ok_or_else(|| anyhow!("could not determine mod name from zip"))?;

    let dest = mods_dir.join(&mod_name);
    if dest.exists() && !overwrite {
        bail!(
            "mod '{}' already exists at {}. Pass overwrite=true to replace it.",
            mod_name, dest.display(),
        );
    }
    if dest.exists() {
        // Remove existing version so leftover files from the old version
        // don't pollute the new install.
        fs::remove_dir_all(&dest)
            .with_context(|| format!("removing existing {}", dest.display()))?;
    }
    fs::create_dir_all(&dest)?;

    // ── Extract ───────────────────────────────────────────────────────
    let strip_prefix = layout.strip_prefix();
    let mut bytes: u64 = 0;
    let mut count: u64 = 0;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        let raw = file.mangled_name();

        // Skip directory headers entirely; we recreate dirs from the
        // file paths as we go. This sidesteps an annoying edge case
        // where the zip records an empty top-level dir entry.
        if file.is_dir() { continue; }

        let rel_path = match &strip_prefix {
            Some(p) => match raw.strip_prefix(p) {
                Ok(s) if s.as_os_str().is_empty() => continue,
                Ok(s) => s.to_path_buf(),
                Err(_) => raw,
            },
            None => raw,
        };

        let safe = sanitize_relative_path(&rel_path)
            .ok_or_else(|| anyhow!(
                "zip entry has unsafe path (escapes destination): {}",
                file.name(),
            ))?;
        let out_path = dest.join(&safe);
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = File::create(&out_path)
            .with_context(|| format!("creating {}", out_path.display()))?;
        bytes += io::copy(&mut file, &mut out)?;
        count += 1;
    }

    Ok(InstallResult {
        mod_name,
        bytes_extracted: bytes,
        file_count: count,
        layout: layout.label().to_string(),
    })
}

/// Delete an installed mod by name. Same path-safety rules as worlds.
pub fn delete_mod(mods_dir: &Path, name: &str) -> Result<u64> {
    if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
        bail!("invalid mod name: '{name}'");
    }
    let target = mods_dir.join(name);
    if !target.exists() {
        bail!("mod folder not found: {}", target.display());
    }
    let canon_target = target.canonicalize()
        .with_context(|| format!("canonicalizing {}", target.display()))?;
    let canon_root = mods_dir.canonicalize()
        .with_context(|| format!("canonicalizing {}", mods_dir.display()))?;
    if !canon_target.starts_with(&canon_root) {
        bail!("refusing to delete: {} is not inside {}",
              canon_target.display(), canon_root.display());
    }
    let size = dir_size(&canon_target).unwrap_or(0);
    fs::remove_dir_all(&canon_target)
        .with_context(|| format!("removing {}", canon_target.display()))?;
    Ok(size)
}

// ─── layout detection ──────────────────────────────────────────────────

#[derive(Debug)]
enum Layout {
    /// `ModInfo.xml` is at the zip root (no wrapper).
    /// → install as `<mod_name>/...`, no prefix to strip.
    Loose { suggested: Option<String> },
    /// One top-level dir, and `ModInfo.xml` is directly inside it.
    /// → install as `<top>/...`, strip `<top>/` prefix.
    Wrapped { top: String },
    /// Two wrapping dirs (e.g. `MyMod-v1.2/MyMod/ModInfo.xml`), the inner
    /// one contains the actual ModInfo.xml.
    /// → install as `<inner>/...`, strip `<outer>/<inner>/` prefix.
    DoubleWrapped { outer: String, inner: String },
    /// We can't find a ModInfo.xml. Treat the zip's top dir (or the
    /// archive name) as the mod name and unpack everything under it.
    Unknown { suggested: Option<String> },
}

impl Layout {
    fn suggested_name(&self) -> Option<String> {
        match self {
            Layout::Loose     { suggested }       => suggested.clone(),
            Layout::Wrapped   { top }             => Some(top.clone()),
            Layout::DoubleWrapped { inner, .. }   => Some(inner.clone()),
            Layout::Unknown   { suggested }       => suggested.clone(),
        }
    }
    fn strip_prefix(&self) -> Option<PathBuf> {
        match self {
            Layout::Loose { .. }                       => None,
            Layout::Wrapped { top }                    => Some(PathBuf::from(top)),
            Layout::DoubleWrapped { outer, inner }     => Some(PathBuf::from(outer).join(inner)),
            Layout::Unknown { .. }                     => None,
        }
    }
    fn label(&self) -> &'static str {
        match self {
            Layout::Loose         { .. } => "loose (no wrapper)",
            Layout::Wrapped       { .. } => "wrapped",
            Layout::DoubleWrapped { .. } => "double-wrapped",
            Layout::Unknown       { .. } => "unknown layout",
        }
    }
}

fn detect_layout<R: Read + Seek>(zip: &mut ZipArchive<R>) -> Result<Layout> {
    // First pass: collect all entry paths and look for ModInfo.xml.
    let mut entries: Vec<PathBuf> = Vec::with_capacity(zip.len());
    let mut modinfo_paths: Vec<PathBuf> = Vec::new();
    for i in 0..zip.len() {
        let entry = zip.by_index(i)?;
        let p = entry.mangled_name();
        if entry.is_file() {
            let name_lower = p.file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_ascii_lowercase());
            if name_lower.as_deref() == Some("modinfo.xml") {
                modinfo_paths.push(p.clone());
            }
        }
        entries.push(p);
    }

    // Compute the common top-level directory across all entries (if any).
    let common_top = common_top_dir(&entries);

    // Case A: ModInfo.xml exists.
    if !modinfo_paths.is_empty() {
        // Take the shortest path — the most "outer" location.
        modinfo_paths.sort_by_key(|p| p.components().count());
        let mi = &modinfo_paths[0];
        let depth = mi.components().count();
        match depth {
            1 => Ok(Layout::Loose { suggested: common_top.clone() }),
            2 => {
                let top = mi.components().next()
                    .and_then(|c| match c {
                        Component::Normal(s) => s.to_str().map(|s| s.to_string()),
                        _ => None,
                    })
                    .ok_or_else(|| anyhow!("could not determine wrapper folder"))?;
                Ok(Layout::Wrapped { top })
            }
            _ => {
                // Three or more deep: treat the two outermost as wrappers.
                let mut comps = mi.components();
                let outer = match comps.next() {
                    Some(Component::Normal(s)) => s.to_str().map(|s| s.to_string()),
                    _ => None,
                }.ok_or_else(|| anyhow!("could not determine outer wrapper folder"))?;
                let inner = match comps.next() {
                    Some(Component::Normal(s)) => s.to_str().map(|s| s.to_string()),
                    _ => None,
                }.ok_or_else(|| anyhow!("could not determine inner wrapper folder"))?;
                Ok(Layout::DoubleWrapped { outer, inner })
            }
        }
    } else {
        // Case B: No ModInfo.xml at all. Best we can do is install
        // whatever's there under a folder named after the common top.
        Ok(Layout::Unknown { suggested: common_top })
    }
}

/// Find the common top-level directory across a set of paths, if all
/// paths agree on one. Otherwise None.
fn common_top_dir(paths: &[PathBuf]) -> Option<String> {
    let mut common: Option<String> = None;
    for p in paths {
        let first = p.components().next();
        let name = match first {
            Some(Component::Normal(s)) => s.to_str().map(|s| s.to_string()),
            _ => None,
        }?;
        match &common {
            None => common = Some(name),
            Some(c) if c == &name => {}
            Some(_) => return None,
        }
    }
    common
}

// ─── shared helpers (small enough not to be worth factoring out) ────────

fn sanitize_relative_path(p: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(s) => out.push(s),
            Component::CurDir   => {}
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => return None,
        }
    }
    Some(out)
}

fn sanitize_name(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_rejects_path_traversal() {
        // dir_size etc. don't matter — the name validation should catch
        // these before we ever touch the filesystem.
        let tmp = std::env::temp_dir();
        for bad in ["..", "../foo", "foo/bar", "a\\b", "."] {
            assert!(delete_mod(&tmp, bad).is_err(), "should reject '{bad}'");
        }
    }
}
