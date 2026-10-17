//! Mass generators: tools that write many files in one run and can say in
//! advance which files those will be. Each planned file becomes a product of
//! its own; the tool runs at most once per build.
//!
//! `generic` gets the plan from a `predict_command` the tool provides; `zola`
//! computes it natively from the site's sources; `sphinx` asks Sphinx itself.
//! What they share — turning a plan into products, running the tool once,
//! caching a plan, building into staging and checking it against the plan —
//! lives here.

mod cargo;
mod cc;
pub mod gem;
mod generic;
mod jekyll;
mod linux_module;
mod mdbook;
mod npm;
mod pip;
mod sphinx;
mod zola;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::graph::{BuildGraph, Product};

/// One file the tool will write, and the input files whose content can
/// change it. Also the shape of a manifest entry `predict_command` prints.
#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlannedOutput {
    pub path: PathBuf,
    pub sources: Vec<PathBuf>,
}

/// Whether this build has already run the tool, and how it went. Every
/// product of an instance asks before executing, so the tool runs at most
/// once per build and a failure is reported once per product instead of
/// re-running a build that just failed.
enum ToolRun {
    NotRun,
    Succeeded,
    Failed(String),
}

/// The once-per-build guard around a mass generator's tool run.
pub struct OnceTool {
    state: Mutex<ToolRun>,
}

impl OnceTool {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(ToolRun::NotRun),
        }
    }

    /// Run `run` if this build has not yet; replay the earlier outcome if it
    /// has. Serialized on the guard, so concurrent products of one instance
    /// cannot start the tool twice. `tool` names it in the replayed error.
    pub fn run(&self, tool: &str, run: impl FnOnce() -> Result<()>) -> Result<()> {
        let mut state = self.state.lock();
        match &*state {
            ToolRun::Succeeded => return Ok(()),
            ToolRun::Failed(msg) => {
                anyhow::bail!("{tool} already failed earlier in this build: {msg}")
            }
            ToolRun::NotRun => {}
        }
        let result = run();
        *state = match &result {
            Ok(()) => ToolRun::Succeeded,
            Err(e) => ToolRun::Failed(format!("{e:#}")),
        };
        result
    }
}

/// Add one product per planned file. `extra` (the config's `dep_inputs`) is
/// added to every product's inputs.
pub fn add_planned_products(
    graph: &mut BuildGraph,
    plan: &[PlannedOutput],
    instance_name: &str,
    config_hash: &str,
    extra: &[PathBuf],
) -> Result<()> {
    for planned in plan {
        let mut inputs = planned.sources.clone();
        for input in extra {
            if !inputs.contains(input) {
                inputs.push(input.clone());
            }
        }
        // A product's cache key is processor + config hash + input
        // checksum; the output path is not part of it, because for every
        // other processor the output path follows from the input. Here many
        // outputs can share one input set — a tag's index page, its
        // `page/1/` redirect and its feed all depend on exactly the same
        // posts — and without the path in the key the second would be
        // "restored" from the first's blob, with the first's content.
        let hash = crate::checksum::hash_parts(&[config_hash, &planned.path.display().to_string()]);
        graph.add_product(
            inputs,
            vec![planned.path.clone()],
            instance_name,
            Some(hash),
        )?;
    }
    Ok(())
}

/// After the tool ran: the product's planned file must exist. The plan check
/// already reported a missing file in strict mode; in loose mode it was a
/// warning, and this product still has nothing to cache.
pub fn require_planned_output(product: &Product, tool: &str) -> Result<()> {
    let output = product.primary_output();
    if !output.is_file() {
        anyhow::bail!(
            "{tool} did not produce the planned output {}",
            output.display()
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The plan cache: one file per instance under `.rsconstruct/`, holding the
// plan exactly as the planner produced it and the key it is valid for.
// ---------------------------------------------------------------------------

/// Where mass generators keep their private state: cached plans, staging
/// directories, planner scratch space.
const STATE_DIR: &str = ".rsconstruct/mass_generator";

/// A file under the state directory named after the instance.
pub(super) fn state_path(instance_name: &str, suffix: &str) -> PathBuf {
    Path::new(STATE_DIR).join(format!("{instance_name}{suffix}"))
}

pub(super) fn plan_cache_path(instance_name: &str) -> PathBuf {
    state_path(instance_name, ".json")
}

/// A cached plan: the planner's output verbatim, and the key it is valid for.
#[derive(Serialize, Deserialize)]
struct PlanCache {
    key: String,
    manifest: String,
}

/// The cached plan if the cache file exists and was written for `key`;
/// `None` for a missing file or another key (both mean "plan again"). A file
/// that exists but does not parse is an error: it is rsconstruct's own state,
/// so garbage there is a bug or tampering, not a reason to quietly re-plan.
pub(super) fn read_plan_cache(path: &Path, key: &str) -> Result<Option<String>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(anyhow::Error::from(e)
                .context(format!("Failed to read plan cache {}", path.display())));
        }
    };
    let cache: PlanCache = serde_json::from_str(&text).with_context(|| {
        format!(
            "Plan cache {} is corrupt; delete it and rerun",
            path.display()
        )
    })?;
    Ok((cache.key == key).then_some(cache.manifest))
}

pub(super) fn write_plan_cache(path: &Path, key: &str, manifest: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let cache = PlanCache {
        key: key.to_string(),
        manifest: manifest.to_string(),
    };
    let text = serde_json::to_string(&cache).context("Failed to serialize the plan cache")?;
    std::fs::write(path, text)
        .with_context(|| format!("Failed to write plan cache {}", path.display()))
}

// ---------------------------------------------------------------------------
// Staging: the tool builds into a private directory, what it wrote is
// compared with the plan, and the planned files are moved into place.
// ---------------------------------------------------------------------------

/// An empty, freshly created directory under the state directory for this
/// instance. Whatever a previous run left there is removed first.
pub(super) fn fresh_state_dir(instance_name: &str, suffix: &str) -> Result<PathBuf> {
    let dir = state_path(instance_name, suffix);
    remove_dir_if_present(&dir)?;
    std::fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    Ok(dir)
}

pub(super) fn remove_dir_if_present(dir: &Path) -> Result<()> {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => {
            Err(anyhow::Error::from(e).context(format!("Failed to remove {}", dir.display())))
        }
    }
}

/// Every regular file under `dir`, following symlinks. A missing directory
/// is empty.
pub(super) fn files_under(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current)
            .with_context(|| format!("Failed to read directory {}", current.display()))?;
        for entry in entries {
            let entry = entry.with_context(|| format!("Failed to read {}", current.display()))?;
            let path = entry.path();
            let meta = std::fs::metadata(&path)
                .with_context(|| format!("Failed to stat {}", path.display()))?;
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file() {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// `path` below `base`, with `/` separators.
pub(super) fn relative_slash_path(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// After the tool built into `staging`: compare what it wrote with the
/// plan (`planned`, paths relative to the output directory), exactly and
/// both ways, then move every planned file into `output_dir` and remove the
/// staging directory. A mismatch fails the build — or, in loose mode, warns
/// — and leaves staging in place for inspection.
pub(super) fn install_staged(
    staging: &Path,
    planned: &BTreeSet<String>,
    output_dir: &Path,
    instance_name: &str,
    tool: &str,
    loose: bool,
) -> Result<()> {
    let written: BTreeSet<String> = files_under(staging)?
        .into_iter()
        .map(|p| relative_slash_path(staging, &p))
        .collect();
    let mut mismatches: Vec<String> = planned
        .difference(&written)
        .map(|p| format!("missing: {p} (planned but not written by {tool})"))
        .collect();
    mismatches.extend(
        written
            .difference(planned)
            .map(|p| format!("unexpected: {p} (written by {tool} but not planned)")),
    );
    if !mismatches.is_empty() {
        let msg = format!(
            "[{instance_name}] {tool}'s output does not match the plan (staging kept at {}):\n  {}",
            staging.display(),
            mismatches.join("\n  ")
        );
        if loose {
            crate::output::warn(&format!("{msg}\n  (loose_manifest = true: continuing)"));
        } else {
            anyhow::bail!("{msg}");
        }
    }

    // Move every planned file the tool wrote into place. A planned file that
    // is already there may be a read-only hardlink from a cache restore, so
    // it is unlinked rather than overwritten.
    for rel in planned.intersection(&written) {
        let from = staging.join(rel);
        let to = output_dir.join(rel);
        match std::fs::remove_file(&to) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(anyhow::Error::from(e)
                    .context(format!("Failed to remove stale output {}", to.display())));
            }
        }
        crate::processor::ensure_output_dir(&to)?;
        std::fs::rename(&from, &to)
            .with_context(|| format!("Failed to move {} to {}", from.display(), to.display()))?;
    }
    remove_dir_if_present(staging)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_cache_round_trips_and_rejects_other_keys_and_garbage() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("sub/site.json");
        assert_eq!(
            read_plan_cache(&path, "k1").unwrap(),
            None,
            "missing file is a miss"
        );
        write_plan_cache(&path, "k1", "{\"version\":1,\"outputs\":[]}").unwrap();
        assert_eq!(
            read_plan_cache(&path, "k1").unwrap().as_deref(),
            Some("{\"version\":1,\"outputs\":[]}")
        );
        assert_eq!(
            read_plan_cache(&path, "k2").unwrap(),
            None,
            "other key is a miss"
        );
        std::fs::write(&path, "not json").unwrap();
        let err = read_plan_cache(&path, "k1").unwrap_err();
        assert!(
            format!("{err:#}").contains("is corrupt; delete it"),
            "{err:#}"
        );
    }

    #[test]
    fn install_staged_moves_planned_files_and_reports_both_mismatch_kinds() {
        let tmp = tempfile::TempDir::new().unwrap();
        let staging = tmp.path().join("staging");
        let out = tmp.path().join("out");
        std::fs::create_dir_all(staging.join("sub")).unwrap();
        std::fs::write(staging.join("a.html"), "a").unwrap();
        std::fs::write(staging.join("sub/b.html"), "b").unwrap();
        std::fs::write(staging.join("stray.html"), "s").unwrap();
        let planned: BTreeSet<String> = ["a.html", "sub/b.html", "never.html"]
            .map(String::from)
            .into();

        let err = install_staged(&staging, &planned, &out, "inst", "tool", false).unwrap_err();
        let text = format!("{err:#}");
        assert!(
            text.contains("missing: never.html (planned but not written by tool)"),
            "{text}"
        );
        assert!(
            text.contains("unexpected: stray.html (written by tool but not planned)"),
            "{text}"
        );
        assert!(staging.is_dir(), "strict mode keeps staging for inspection");
        assert!(!out.exists(), "strict mode moves nothing");

        install_staged(&staging, &planned, &out, "inst", "tool", true).unwrap();
        assert_eq!(std::fs::read_to_string(out.join("a.html")).unwrap(), "a");
        assert_eq!(
            std::fs::read_to_string(out.join("sub/b.html")).unwrap(),
            "b"
        );
        assert!(
            !out.join("stray.html").exists(),
            "an unplanned file is never installed"
        );
        assert!(!staging.exists(), "staging is removed once installed");
    }
}
