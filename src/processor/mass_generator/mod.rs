//! Mass generators: tools that write many files in one run and can say in
//! advance which files those will be. Each planned file becomes a product of
//! its own; the tool runs at most once per build.
//!
//! `generic` gets the plan from a `predict_command` the tool provides; `zola`
//! computes it natively from the site's sources. What they share — turning a
//! plan into products, running the tool once, checking each product's file —
//! lives here.

mod generic;
mod zola;

use anyhow::Result;
use parking_lot::Mutex;
use serde::Deserialize;
use std::path::PathBuf;

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
        let hash =
            crate::checksum::hash_parts(&[config_hash, &planned.path.display().to_string()]);
        graph.add_product(inputs, vec![planned.path.clone()], instance_name, Some(hash))?;
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
