//! World import/export. Lets the user download an entire save folder
//! as a `.zip` from the browser, and upload a `.zip` that extracts
//! back into the saves directory.
//!
//! Safety:
//!   - Refuses to overwrite a live save while the server is running
//!     (caller's responsibility — we're a pure helper here).
//!   - On extract, every entry's path is normalized and rejected if it
//!     escapes the destination directory (path traversal attacks via
//!     `..` segments or absolute paths).
//!   - On extract, we refuse if the zip's top-level directory matches
//!     an existing save unless the user passed `overwrite=true`.
//!
//! Streaming:
//!   - Download is sync-streamed to disk via `zip::ZipWriter`. We don't
//!     buffer the whole archive in memory, since worlds can be 500MB+.
//!   - Upload is buffered to a temp file as the multipart body arrives,
//!     then extracted from there.

use std::fs::{self, File};
use std::io::{self, Read, Seek, Write};
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldEntry {
    pub name: String,
    pub size_bytes: u64,
    pub modified_iso: Option<String>,
}

/// Enumerate save folders that the user could download. Each direct
/// subdirectory of `saves_dir` is one save.
pub fn list_saves(saves_dir: &Path) -> Result<Vec<WorldEntry>> {
    if !saves_dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(saves_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = match entry.file_name().to_str() {
            Some(s) => s.to_string(),
            None => continue, // skip non-UTF-8 paths
        };
        // Skip our own "<n>.replaced-<ts>" backup-restore artifacts.
        if name.contains(".replaced-") {
            continue;
        }
        let size = dir_size(&entry.path()).unwrap_or(0);
        let modified_iso = entry.metadata().ok()
            .and_then(|m| m.modified().ok())
            .map(|t| {
                let dt: chrono::DateTime<chrono::Utc> = t.into();
                dt.to_rfc3339()
            });
        out.push(WorldEntry { name, size_bytes: size, modified_iso });
    }
    // Newest first.
    out.sort_by(|a, b| b.modified_iso.cmp(&a.modified_iso));
    Ok(out)
}

/// Pack `saves_dir/<name>` into a zip file at `dest`. The zip's top-level
/// directory is `<name>/` so the extract round-trips cleanly.
///
/// Use `Deflate` compression — saves a lot for the text-y .ttp/.dat files,
/// negligible cost for the already-compressed binary chunks.
pub fn pack_save<W: Write + Seek>(
    saves_dir: &Path,
    name: &str,
    sink: W,
) -> Result<u64> {
    let src = saves_dir.join(name);
    if !src.exists() || !src.is_dir() {
        bail!("save folder not found: {}", src.display());
    }
    let mut zip = ZipWriter::new(sink);
    let opts: FileOptions = FileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let mut total: u64 = 0;
    walk(&src, &src, &mut zip, &opts, &mut total, name)?;
    zip.finish()?;
    Ok(total)
}

fn walk<W: Write + Seek>(
    base: &Path,
    cur: &Path,
    zip: &mut ZipWriter<W>,
    opts: &FileOptions,
    total: &mut u64,
    top_name: &str,
) -> Result<()> {
    for entry in fs::read_dir(cur)? {
        let entry = entry?;
        let path = entry.path();
        // Path inside the archive: "<top_name>/<rel_to_base>"
        let rel = path.strip_prefix(base).unwrap_or(&path);
        let arc_name = format!("{}/{}", top_name, rel.to_string_lossy().replace('\\', "/"));

        if entry.file_type()?.is_dir() {
            zip.add_directory(format!("{arc_name}/"), *opts)?;
            walk(base, &path, zip, opts, total, top_name)?;
        } else {
            zip.start_file(arc_name, *opts)?;
            let mut f = File::open(&path)
                .with_context(|| format!("opening {}", path.display()))?;
            let copied = io::copy(&mut f, zip)?;
            *total += copied;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    pub save_name: String,
    pub bytes_extracted: u64,
    pub file_count: u64,
}

/// Extract the zip at `archive_path` into `saves_dir`. We figure out the
/// "save name" from the zip's top-level directory (or `forced_name` if
/// supplied). Returns metadata about what was written.
///
/// `overwrite=false` (the default) refuses if the destination already
/// exists. The caller can either rename it aside via the backup module
/// or pass `overwrite=true`.
pub fn import_zip(
    archive_path: &Path,
    saves_dir: &Path,
    forced_name: Option<&str>,
    overwrite: bool,
) -> Result<ImportResult> {
    let f = File::open(archive_path)
        .with_context(|| format!("opening uploaded archive {}", archive_path.display()))?;
    let mut zip = ZipArchive::new(f).context("zip is malformed or not a zip file")?;

    // ── Decide the save name ──────────────────────────────────────────
    // Priority: explicit override > zip's common top-level dir > error.
    let detected = detect_top_dir(&mut zip)?;
    let save_name = forced_name
        .map(sanitize_name)
        .or(detected)
        .ok_or_else(|| anyhow!(
            "cannot determine save name — zip has no common top-level folder. \
             Try uploading a zip whose contents are nested under a single folder, \
             or pass an explicit name."
        ))?;
    if save_name.is_empty() {
        bail!("save name is empty after sanitization");
    }

    let dest_root = saves_dir.join(&save_name);
    if dest_root.exists() && !overwrite {
        bail!(
            "a save named '{}' already exists at {}. Pass overwrite=true to replace it.",
            save_name, dest_root.display(),
        );
    }
    fs::create_dir_all(&dest_root)?;

    // ── Extract entries ───────────────────────────────────────────────
    // We strip the zip's own top-level dir if it matches the save name,
    // since we're already writing into that named directory.
    let strip_prefix = detect_top_dir(&mut zip)?
        .filter(|d| d == &save_name);

    let mut bytes: u64 = 0;
    let mut count: u64 = 0;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        let raw = file.mangled_name();

        // Skip the bare top-level directory entry itself.
        let rel_path = match &strip_prefix {
            Some(p) => match raw.strip_prefix(p) {
                Ok(s) if s.as_os_str().is_empty() => continue,
                Ok(s) => s.to_path_buf(),
                Err(_) => raw, // entry not under expected prefix; keep as-is
            },
            None => raw,
        };

        // Reject path traversal (..) and absolute paths.
        let safe = sanitize_relative_path(&rel_path)
            .ok_or_else(|| anyhow!(
                "zip entry has unsafe path (escapes destination): {}",
                file.name(),
            ))?;
        let out_path = dest_root.join(&safe);

        if file.is_dir() {
            fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut out = File::create(&out_path)
                .with_context(|| format!("creating {}", out_path.display()))?;
            bytes += io::copy(&mut file, &mut out)?;
            count += 1;
        }
    }

    Ok(ImportResult { save_name, bytes_extracted: bytes, file_count: count })
}

/// Find the directory all entries in the zip share, if any. Returns
/// `Some("Navezgane")` for a zip like `Navezgane/foo`, `Navezgane/bar/baz`,
/// or `None` if entries don't share a common first segment (mixed top-level).
fn detect_top_dir<R: Read + Seek>(zip: &mut ZipArchive<R>) -> Result<Option<String>> {
    let mut common: Option<String> = None;
    let mut had_any = false;
    for i in 0..zip.len() {
        let entry = zip.by_index(i)?;
        let path = entry.mangled_name();
        let first = match path.components().next() {
            Some(Component::Normal(s)) => s.to_string_lossy().to_string(),
            _ => return Ok(None), // suspicious — bail to "no common dir"
        };
        had_any = true;
        match &common {
            None => common = Some(first),
            Some(c) if c == &first => {}
            Some(_) => return Ok(None), // mixed top-levels
        }
    }
    Ok(if had_any { common } else { None })
}

/// Strip unsafe components from a path inside an archive. Refuses if
/// any component is `..`, an absolute root, or a Windows drive prefix.
fn sanitize_relative_path(p: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(s) => out.push(s),
            Component::CurDir   => {} // skip "./"
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => return None,
        }
    }
    Some(out)
}

/// Strip filesystem-unsafe characters from a save name. Same rules
/// across Windows/macOS/Linux for portability.
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

/// Delete a save folder. Refuses if the path doesn't look like one of
/// our saves (must be a direct subdirectory of `saves_dir`). Returns
/// the bytes freed.
pub fn delete_save(saves_dir: &Path, name: &str) -> Result<u64> {
    // Sanity-check: name must not contain path separators, .., etc.
    // Otherwise a malicious request could remove arbitrary directories.
    if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
        bail!("invalid save name: '{name}'");
    }
    let target = saves_dir.join(name);
    if !target.exists() {
        bail!("save folder not found: {}", target.display());
    }
    // Resolve symlinks and confirm the target is still inside saves_dir.
    // Stops `name = "../something"` from escaping even if the basic name
    // check missed an edge case.
    let canon_target = target.canonicalize()
        .with_context(|| format!("canonicalizing {}", target.display()))?;
    let canon_saves = saves_dir.canonicalize()
        .with_context(|| format!("canonicalizing {}", saves_dir.display()))?;
    if !canon_target.starts_with(&canon_saves) {
        bail!("refusing to delete: {} is not inside {}",
              canon_target.display(), canon_saves.display());
    }

    let size = dir_size(&canon_target).unwrap_or(0);
    fs::remove_dir_all(&canon_target)
        .with_context(|| format!("removing {}", canon_target.display()))?;
    Ok(size)
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
    fn rejects_parent_dir() {
        assert!(sanitize_relative_path(Path::new("../etc/passwd")).is_none());
        assert!(sanitize_relative_path(Path::new("foo/../../bar")).is_none());
    }
    #[test]
    fn rejects_absolute() {
        assert!(sanitize_relative_path(Path::new("/etc/passwd")).is_none());
    }
    #[test]
    fn accepts_normal() {
        assert_eq!(
            sanitize_relative_path(Path::new("Navezgane/region/world.dat")).unwrap(),
            PathBuf::from("Navezgane/region/world.dat"),
        );
    }
    #[test]
    fn skips_curdir() {
        assert_eq!(
            sanitize_relative_path(Path::new("./Navezgane/foo")).unwrap(),
            PathBuf::from("Navezgane/foo"),
        );
    }
    #[test]
    fn sanitize_strips_bad_chars() {
        assert_eq!(sanitize_name("My/World*?"), "My_World__");
    }
}
