//! Removing outputs a build no longer produces (`[build] remove_orphan_outputs`).
//!
//! A build only knows the products it has now. When a product disappears
//! (its source was deleted, its processor disabled, its output moved), the
//! files it wrote stay on disk and nothing looks at them again. With this on,
//! every full build records the output files it is responsible for, with the
//! size and modification time each had at the end of the build, in
//! [`RECORD_FILE`]. The next full build compares: a recorded path that no
//! product declares any more is an orphan, and it is removed if it still has
//! the recorded size and modification time and git does not track it. A
//! file changed since (edited by hand, or replaced by a hand-written source)
//! or tracked by git (a generator stopped but its last output was kept as a
//! source) is kept, reported once, and dropped from the record. Directories
//! the removals leave empty go too.
//!
//! The caller says which files are current: the declared outputs, plus,
//! for an explicit stanza's `output_dirs`, the files of the tree it last
//! built or restored. Only relative paths inside the project are tracked, so
//! nothing outside it can be removed. Paths are relative to `root`, the
//! project root (the build passes `""`, the current directory). See
//! docs/src/internal/orphan-outputs.md.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Where the outputs of the last full build are recorded, under the root.
pub const RECORD_FILE: &str = ".rsconstruct/outputs.json";

/// What a recorded output looked like at the end of the build that wrote it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Stamp {
    size: u64,
    mtime_ns: u64,
}

/// The size and modification time of the regular file at `path`, or `None`
/// when there is none (missing, a directory, a symlink).
fn stamp(path: &Path) -> Option<Stamp> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    Some(Stamp {
        size: meta.len(),
        mtime_ns: u64::try_from(mtime.as_nanos()).ok()?,
    })
}

/// Whether `path` stays inside the project: relative, with no `..`.
fn inside_project(path: &Path) -> bool {
    path.components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

/// The output files the products in `outputs` declare, as tracked here.
pub fn tracked<'a>(outputs: impl IntoIterator<Item = &'a PathBuf>) -> BTreeSet<PathBuf> {
    outputs
        .into_iter()
        .filter(|p| inside_project(p))
        .cloned()
        .collect()
}

fn load(record: &Path) -> Result<Option<BTreeMap<PathBuf, Stamp>>> {
    let text = match fs::read_to_string(record) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(e).with_context(|| format!("Failed to read {}", record.display()));
        }
    };
    serde_json::from_str(&text).map(Some).with_context(|| {
        format!(
            "Failed to parse {}; delete it and the next build starts a new record",
            record.display()
        )
    })
}

/// What [`remove_orphans`] did.
#[derive(Debug, Default)]
pub struct Removal {
    /// Orphans removed.
    pub removed: Vec<PathBuf>,
    /// Orphans kept because they changed after the build that wrote them.
    pub changed: Vec<PathBuf>,
    /// Orphans kept because git tracks them: a generator stopped, but its
    /// last output was kept as a source.
    pub tracked: Vec<PathBuf>,
}

/// Remove the outputs recorded under `root` that are not in `current`,
/// still look as recorded, and are not tracked by git, then the directories
/// that leaves empty. `git_tracked` lists the files git tracks; it is only
/// called when there is something to remove. No record yet (the first
/// build with this on) removes nothing.
pub fn remove_orphans(
    root: &Path,
    current: &BTreeSet<PathBuf>,
    git_tracked: impl FnOnce() -> Result<BTreeSet<PathBuf>>,
) -> Result<Removal> {
    let mut removal = Removal::default();
    let Some(previous) = load(&root.join(RECORD_FILE))? else {
        return Ok(removal);
    };
    let mut candidates: Vec<PathBuf> = Vec::new();
    for (path, recorded) in previous {
        if current.contains(&path) || !inside_project(&path) {
            continue;
        }
        let full = root.join(&path);
        match stamp(&full) {
            // Already gone: nothing to do.
            None if fs::symlink_metadata(&full).is_err() => {}
            Some(now) if now == recorded => candidates.push(path),
            // Changed since, or no longer a regular file.
            _ => removal.changed.push(path),
        }
    }
    if candidates.is_empty() {
        return Ok(removal);
    }
    let tracked = git_tracked()?;
    for path in candidates {
        if tracked.contains(&path) {
            removal.tracked.push(path);
            continue;
        }
        let full = root.join(&path);
        fs::remove_file(&full).with_context(|| {
            format!(
                "Failed to remove output no longer produced: {}",
                full.display()
            )
        })?;
        remove_empty_parents(root, &path)?;
        removal.removed.push(path);
    }
    Ok(removal)
}

/// The files git tracks, relative to the current directory (the project
/// root); none when the project is not inside a git work tree.
pub fn git_tracked(ctx: &crate::build_context::BuildContext) -> Result<BTreeSet<PathBuf>> {
    let mut probe = std::process::Command::new("git");
    probe.args(["rev-parse", "--is-inside-work-tree"]);
    let inside = crate::processor::run_command_capture(ctx, &probe)
        .context("Failed to run git rev-parse")?;
    if !inside.status.success() {
        return Ok(BTreeSet::new());
    }
    let mut cmd = std::process::Command::new("git");
    cmd.args(["ls-files", "-z", "--cached"]);
    let output =
        crate::processor::run_command_capture(ctx, &cmd).context("Failed to run git ls-files")?;
    if !output.status.success() {
        anyhow::bail!(
            "git ls-files failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect())
}

/// Remove the parent directories of `path` that are now empty, up to the
/// project root.
fn remove_empty_parents(root: &Path, path: &Path) -> Result<()> {
    let mut dir = path.parent();
    while let Some(d) = dir.filter(|d| !d.as_os_str().is_empty()) {
        let full = root.join(d);
        match fs::remove_dir(&full) {
            Ok(()) => dir = d.parent(),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::DirectoryNotEmpty | std::io::ErrorKind::NotFound
                ) =>
            {
                break;
            }
            Err(e) => {
                return Err(e).with_context(|| {
                    format!("Failed to remove empty directory {}", full.display())
                });
            }
        }
    }
    Ok(())
}

/// Record the outputs in `current` that exist now under `root`, for the
/// next full build.
pub fn write_record(root: &Path, current: &BTreeSet<PathBuf>) -> Result<()> {
    let stamps: BTreeMap<&PathBuf, Stamp> = current
        .iter()
        .filter_map(|path| stamp(&root.join(path)).map(|s| (path, s)))
        .collect();
    let text = serde_json::to_string(&stamps).expect(crate::errors::JSON_SERIALIZE);
    let record = root.join(RECORD_FILE);
    if let Some(dir) = record.parent() {
        fs::create_dir_all(dir)
            .with_context(|| format!("Failed to create directory {}", dir.display()))?;
    }
    fs::write(&record, text).with_context(|| format!("Failed to write {}", record.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(paths: &[&str]) -> BTreeSet<PathBuf> {
        paths.iter().map(PathBuf::from).collect()
    }

    /// An output no product declares any more is removed with the
    /// directories that leaves empty; one still declared stays.
    #[test]
    fn removes_outputs_no_longer_produced() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("out/a/b")).unwrap();
        fs::write(root.join("out/a/b/gone.html"), "x").unwrap();
        fs::write(root.join("out/kept.html"), "y").unwrap();
        write_record(root, &set(&["out/a/b/gone.html", "out/kept.html"])).unwrap();

        let removal =
            remove_orphans(root, &set(&["out/kept.html"]), || Ok(BTreeSet::new())).unwrap();
        assert_eq!(removal.removed, vec![PathBuf::from("out/a/b/gone.html")]);
        assert!(removal.changed.is_empty() && removal.tracked.is_empty());
        assert!(
            !root.join("out/a").exists(),
            "empty directories are removed"
        );
        assert!(root.join("out/kept.html").exists());
    }

    /// An orphan changed since the build that wrote it is kept and reported.
    #[test]
    fn keeps_orphans_changed_since() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("out")).unwrap();
        fs::write(root.join("out/edited.html"), "x").unwrap();
        write_record(root, &set(&["out/edited.html"])).unwrap();
        fs::write(root.join("out/edited.html"), "edited by hand").unwrap();

        let removal = remove_orphans(root, &set(&[]), || Ok(BTreeSet::new())).unwrap();
        assert_eq!(removal.removed, Vec::<PathBuf>::new());
        assert_eq!(removal.changed, vec![PathBuf::from("out/edited.html")]);
        assert!(root.join("out/edited.html").exists());
    }

    /// An orphan git tracks is kept: its generator stopped, but the file
    /// was kept as a source.
    #[test]
    fn keeps_orphans_git_tracks() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("README.md"), "x").unwrap();
        write_record(root, &set(&["README.md"])).unwrap();

        let removal = remove_orphans(root, &set(&[]), || Ok(set(&["README.md"]))).unwrap();
        assert_eq!(removal.removed, Vec::<PathBuf>::new());
        assert_eq!(removal.tracked, vec![PathBuf::from("README.md")]);
        assert!(root.join("README.md").exists());
    }

    /// With nothing to remove, git is not asked.
    #[test]
    fn git_is_asked_only_when_something_would_be_removed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("a.txt"), "x").unwrap();
        write_record(root, &set(&["a.txt"])).unwrap();
        let removal = remove_orphans(root, &set(&["a.txt"]), || {
            panic!("git must not be asked when nothing is orphaned")
        })
        .unwrap();
        assert_eq!(removal.removed, Vec::<PathBuf>::new());
    }

    /// The first build with the record on removes nothing, and an orphan
    /// already gone is not reported.
    #[test]
    fn no_record_and_missing_orphans_remove_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let removal = remove_orphans(root, &set(&[]), || Ok(BTreeSet::new())).unwrap();
        assert!(removal.removed.is_empty() && removal.changed.is_empty());

        fs::write(root.join("a.txt"), "x").unwrap();
        write_record(root, &set(&["a.txt"])).unwrap();
        fs::remove_file(root.join("a.txt")).unwrap();
        let removal = remove_orphans(root, &set(&[]), || Ok(BTreeSet::new())).unwrap();
        assert!(removal.removed.is_empty() && removal.changed.is_empty());
    }

    /// Paths outside the project are never tracked.
    #[test]
    fn only_paths_inside_the_project_are_tracked() {
        let outputs = [
            PathBuf::from("out/a"),
            PathBuf::from("/etc/passwd"),
            PathBuf::from("../other/x"),
        ];
        assert_eq!(tracked(&outputs), set(&["out/a"]));
    }
}
