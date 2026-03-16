# Cross-Processor Dependencies

This chapter discusses the problem of one processor's output being consumed as
input by another processor, and the design options for solving it.

## The Problem

Consider a template that generates a Python file:

```
templates.tera/config.py.tera  →  (template processor)  →  config.py
```

Ideally, ruff should then lint the generated `config.py`. Or a template might
generate a C++ source file that needs to be compiled by `cc_single_file` and
linted by `cppcheck`. Chains can be arbitrarily deep:

```
template  →  generates foo.sh  →  shellcheck lints foo.sh
template  →  generates bar.c   →  cc_single_file compiles bar.c  →  cppcheck lints bar.c
```

Currently this does not work. Each processor discovers its inputs by querying
the `FileIndex`, which is built once at startup by scanning the filesystem.
Files that do not exist yet (because they will be produced by another processor)
are invisible to downstream processors. No product is created for them, and no
dependency edge is formed.

## Why It Breaks

The build pipeline today is:

1. Walk the filesystem once to build `FileIndex`
2. Each processor runs `discover()` against that index
3. `resolve_dependencies()` matches product inputs to product outputs by path
4. Topological sort and execution

Step 3 already handles cross-processor edges correctly: if product A declares
output `foo.py` and product B declares input `foo.py`, a dependency edge from
A to B is created automatically. The problem is that step 2 never creates
product B in the first place, because `foo.py` is not in the `FileIndex`.

## How Other Build Systems Handle This

### Bazel

Bazel uses BUILD files where rules explicitly declare their inputs and outputs.
Dependencies are specified by label references, not by filesystem scanning.
However, Bazel does use `glob()` to discover source files during its loading
phase. The key insight is that during the analysis phase, both source files
(from globs) and generated files (from rule declarations) are visible in a
unified view. A rule's declared outputs are known before any action executes.

### Buck2

Buck2 takes a similar approach with a single unified dependency graph (no
separate phases). Rules call `declare_output()` to create artifact references
and return them via providers. Downstream rules receive these references through
their declared dependencies. For cases where the dependency structure is not
known statically, Buck2 provides `dynamic_output` — a rule can read an artifact
at build time to discover additional dependencies.

### Common Pattern

In both systems, the core principle is the same: **a rule's declared outputs are
visible to the dependency resolver before execution begins**. The dependency
graph is fully resolved at analysis time.

## Proposed Solutions

### A. Multi-Pass Discovery (Iterative Build-Scan Loop)

Run discovery, build what is ready, re-scan the filesystem, discover again.
Repeat until nothing new is found.

- **Pro:** Simple mental model, handles arbitrary chain depth
- **Con:** Slow (re-scans filesystem each pass), hard to detect infinite loops,
  execution is interleaved with discovery

### B. Virtual Files from Declared Outputs (Two-Pass)

After the first discovery pass, collect all declared outputs from the graph and
inject them as "virtual files" visible to processors. Run discovery a second
time so downstream processors can find the generated files.

- **Pro:** No filesystem re-scan, single build execution phase, deterministic
- **Con:** Limited to chains of depth 1 (producer → consumer). A three-step
  chain (template → compile → lint) would require three passes, making the
  fixed two-pass design insufficient.

### C. Fixed-Point Discovery Loop

Generalization of Approach B. Run discovery in a loop: after each pass, collect
newly declared outputs and feed them back as known files for the next pass.
Stop when a full pass adds no new products. Add a maximum iteration limit to
catch cycles.

```
known_files = FileIndex (real files on disk)
loop {
    run discover() for all processors, with known_files visible
    new_outputs = outputs declared in this pass that were not in known_files
    if new_outputs is empty → break
    known_files = known_files + new_outputs
}
resolve_dependencies()
execute()
```

A chain of depth N requires N iterations. Most projects would converge in 1-2
iterations.

- **Pro:** Fully general, handles arbitrary chain depth, no filesystem re-scan,
  deterministic, path-based matching (no reliance on file extensions)
- **Con:** Processors must be able to discover products for files that do not
  exist on disk yet (they only know the path). This works for stub-based
  processors and compilers but might be an issue for processors that inspect
  file contents during discovery.

### D. Explicit Cross-Processor Wiring in Config

Let users declare chains in `rsconstruct.toml`:

```toml
[[pipeline]]
from = "template"
to = "ruff"
```

rsconstruct then knows that template outputs matching ruff's scan configuration should
become ruff inputs.

- **Pro:** Explicit, no magic, user controls what gets chained
- **Con:** More configuration burden, loses the "convention over configuration"
  philosophy

### E. Make `out/` Visible to FileIndex

The simplest mechanical fix: stop excluding `out/` from the `FileIndex`. Since
`.gitignore` contains `/out/`, the `ignore` crate skips it. This could be
overridden in the `WalkBuilder` configuration.

- **Pro:** Minimal code change, works on subsequent builds (files already exist
  from previous build)
- **Con:** Does not work on the first clean build (files do not exist yet).
  Processors would also see stale outputs from deleted processors, and stub
  files from other processors (though extension filtering would exclude most of
  these).

### F. Two-Phase Processor Trait (Declarative Forward Tracing)

Split the `ProductDiscovery` trait so that each processor can declare what
output paths it would produce for a given input path, without performing full
discovery:

```rust
trait ProductDiscovery {
    /// Given an input path, return the output paths this processor would
    /// produce. Called even for files that don't exist on disk yet.
    fn would_produce(&self, input_path: &Path) -> Vec<PathBuf>;

    /// Full discovery (as today)
    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()>;
    // ...
}
```

The build system first runs `discover()` on all processors to get the initial
set of products and their outputs. Then, for each declared output, it calls
`would_produce()` on every other processor to trace the chain forward. This
repeats transitively until no new outputs are produced. Finally, `discover()`
runs once more with the complete set of known paths (real + virtual).

Unlike Approach C, this does not require a loop over full discovery passes.
The chain is traced declaratively by asking each processor "if this file
existed, what would you produce from it?" — a lightweight query that does not
modify the graph.

- **Pro:** Single discovery pass plus lightweight forward tracing. No loop, no
  convergence check, no iteration limit. Each processor defines its output
  naming convention in one place. The full transitive closure of outputs is
  known before the main discovery runs.
- **Con:** Adds a method to the `ProductDiscovery` trait that every processor
  must implement. Some processors have complex output path logic (e.g.,
  `cc_single_file` changes the extension and directory), so `would_produce()`
  must replicate that logic — meaning the output path computation exists in
  two places (in `would_produce()` and in `discover()`). Keeping these in sync
  is a maintenance risk.

### G. Hybrid: Visible `out/` + Fixed-Point Discovery

Combine Approach E (make `out/` visible) with Approach C (fixed-point loop) or
Approach F (forward tracing).
On subsequent builds, existing files in `out/` are already in the index. On
clean builds, the fixed-point loop discovers them from declared outputs.

- **Pro:** Most robust — works for both clean and incremental builds
- **Con:** Combines complexity of two approaches, risk of discovering stale
  outputs

## Recommendation

**Approach C (fixed-point discovery loop)** is the most principled solution. It
is fully general, handles arbitrary chain depth, requires no configuration, and
matches the core insight from Bazel and Buck2: declared outputs should be
visible during dependency resolution before execution begins.

The main implementation requirement is extending the `FileIndex` (or creating a
wrapper) to accept "virtual" entries for paths that are declared as outputs but
do not yet exist on disk. Processors already declare their outputs during
`discover()`, so the information needed to populate these virtual entries is
already available.

## Current Status

Cross-processor dependencies are **not yet implemented**. The dependency graph
machinery (`resolve_dependencies()`, topological sort, executor ordering) is
correct and would handle cross-processor edges properly once downstream products
are discovered. The gap is purely in the discovery phase.
