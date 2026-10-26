# Shared Output Directory

Multiple processors can write into the same directory — a website `_site/`, a `dist/`, a `build/` folder. This document explains how rsconstruct keeps each processor's cache correct when they share an output directory, and the exact rules that make it work.

## The scenario

A common case:

- **mkdocs**, run from an explicit stanza, builds a whole site. It produces many files under `_site/` and the stanza declares the directory in `output_dirs`, because mkdocs cannot enumerate its outputs in advance. (A tool that can predict its files is better wired as a [mass generator](output-prediction.md), whose every file is its own product and needs none of what follows.)
- **pandoc**, run from another explicit stanza, converts one specific markdown file into `_site/about.html`. It declares that file in `output_files`.

Both contribute to the same directory. A website IS a single folder by design.

```toml
[processor.explicit.generic.mkdocs]
command     = "mkdocs build --site-dir _site"
inputs      = ["mkdocs.yml"]
output_dirs = ["_site"]

[processor.explicit.generic.pandoc]
command      = "./pandoc-page.sh"
inputs       = ["about.md"]
output_files = ["_site/about.html"]
```

## The problem

Naive implementations break in at least three places:

1. **Over-claiming at cache store time.** If mkdocs's cache entry walks `_site/` and records every file, it will wrongly claim `about.html` as its own. On cache restore, pandoc's file gets restored from mkdocs's cache — with whatever content mkdocs last saw there — even if pandoc hasn't run.
2. **Clobbering at build time.** If mkdocs wipes `_site/` before running (so stale outputs from a previous build don't linger), it will also delete pandoc's `about.html` whenever mkdocs runs after pandoc.
3. **Clobbering at restore time.** If restoring mkdocs's cache wipes `_site/` before writing cached files, it will again destroy pandoc's output.

Each problem leads to silent cache corruption: stale content appears to be fresh, or recently-built files vanish.

## Ownership rule

> **Every declared output path has exactly one owner — the single product that lists it in `outputs`, `output_files`, or produces it as a named product output.**
>
> A directory declared in `output_dirs` is *not* an ownership claim on the whole subtree. The product declaring it owns only the files *it itself produces* that no other product has declared.

This is enforced by a single graph query, `BuildGraph::path_owner(path) -> Option<usize>`, which returns the id of the unique product that declares `path` as one of its outputs (or `None` if nobody does).

Pseudocode:

```text
path_owner(path):
    for each product P in graph:
        if path in P.outputs:
            return P.id
    return None
```

A declared output path has at most one owner by construction — if two products declare the same literal output, that is detected as an output conflict at graph-build time and the build aborts.

## How each of the three hazards is handled

### 1. Over-claiming at cache store time

When the tree descriptor of a product with `output_dirs` is being built in `ObjectStore::store_tree_descriptor`, the walker visits every file under each of those directories. For each file, it asks the graph: "Is this path owned by a different product?"

```text
is_foreign(path) = graph.path_owner(path) is Some(owner) and owner != my_product_id
```

If `is_foreign(path)` is true, the file is **skipped** — it does not appear as a tree entry. The product's cache then contains only files it actually created and that nobody else has laid claim to.

When pandoc writes `_site/about.html` and mkdocs later caches `_site/`, mkdocs's tree will *not* contain `about.html` because `path_owner("_site/about.html") == pandoc.id != mkdocs.id`.

### 2. Clobbering at build time

Before a product's command runs, `remove_stale_outputs` removes stale outputs so the command can rewrite them fresh (important when a cache restore left read-only hardlinks in place).

The rule for a product with `output_dirs`:

- **Do NOT wipe the directories wholesale.**
- Read the previous tree descriptor from the object store.
- Remove only the files recorded in that previous tree.
- Re-create each directory in `output_dirs` (so the command can assume it exists).
- Leave any file not in the previous tree alone — it belongs to somebody else.

Pseudocode:

```text
remove_stale_outputs(product, input_checksum):
    if product has output_dirs:
        previous = object_store.previous_tree_paths(descriptor_key(product, input_checksum))
        for file in previous:
            if file exists: remove it
        for dir in product.output_dirs:
            create dir if missing
    for file in product.outputs:
        if file exists: remove it
```

Because the previous tree only ever contained paths the product owned, this removal cannot touch files owned by other processors.

### 3. Clobbering at restore time

Cache restore for a tree descriptor iterates entries and writes each one in place. It never calls `remove_dir_all` on an `output_dirs` entry. If a file already exists with the correct checksum, the restore skips it (saving I/O).

When mkdocs restores its tree:

- `_site/index.html` and `_site/assets/style.css` are written from the object store.
- `_site/about.html` is NOT in mkdocs's tree, so it is neither written nor removed.
- If pandoc has also restored, pandoc's blob descriptor wrote `_site/about.html` separately.

The two restores compose correctly regardless of order.

## Invariants

The system relies on these invariants; each is enforced in code:

| # | Invariant | Where enforced |
|---|-----------|----------------|
| 1 | Every declared output path has at most one owner. | `add_product` / graph validation (output conflict check) |
| 2 | An `output_dirs` tree descriptor contains only paths not owned by any other product. | `store_tree_descriptor` with `is_foreign` predicate |
| 3 | Pre-run cleanup removes only files the product previously owned. | `remove_stale_outputs` reads `previous_tree_paths` |
| 4 | Cache restore never deletes files it did not cache. | `restore_tree_descriptor` writes in place; no `remove_dir_all` |

When all four hold, processors can freely share an output directory.

## Worked example

Starting from an empty project, both processors are declared as above and both get to run on a fresh build.

### First build

1. **pandoc runs first.**
   - `remove_stale_outputs`: pandoc has no `output_dirs`; removes `_site/about.html` if it exists (it doesn't). No-op.
   - Runs `./pandoc-page.sh`, which creates `_site/about.html`.
   - Caches a blob descriptor for `_site/about.html`.
2. **mkdocs runs next.**
   - `remove_stale_outputs`: mkdocs has `output_dirs`; looks up its previous tree (none — first build). Creates `_site/` to ensure it exists.
   - Runs `mkdocs build`, which writes `_site/index.html`, `_site/assets/style.css`, and may (harmlessly) touch `_site/about.html`.
   - Caches a tree descriptor. The walker skips `_site/about.html` because `path_owner` says pandoc owns it. Tree = `[index.html, assets/style.css]`.

Final state on disk: `index.html`, `assets/style.css`, `about.html`. All three files exist with correct content.

### Incremental build, no changes

- pandoc: input checksum matches; descriptor already exists; skipped.
- mkdocs: input checksum matches; descriptor already exists; skipped.

### Clean outputs + rebuild

1. `rsconstruct clean outputs` deletes `_site/` entirely.
2. Next build:
   - pandoc's input checksum matches its cached descriptor → restore blob → writes `_site/about.html`.
   - mkdocs's input checksum matches its cached descriptor → restore tree → writes only the files in the tree (`index.html`, `assets/style.css`), leaves `about.html` alone.

Final state is the same as after the first build, without either tool having actually run.

### Building only mkdocs (`-p processor.explicit.generic.mkdocs`)

1. pandoc is not in the run set; `_site/about.html` stays wherever it was (absent if cleaned, present otherwise).
2. mkdocs runs or restores its tree.

If `_site/` was clean, `about.html` remains absent — which is correct, because the mkdocs stanza does not claim to produce it. The regression test `output_dirs_tree_does_not_include_foreign_outputs` (`tests/processor/shared_output_dir.rs`) verifies exactly this.

## Non-goals

- **Runtime conflict detection for paths a directory-filling tool actually wrote but didn't declare.** If it happens to write a file that another product declares, the declared owner wins; the `output_dirs` tree simply won't include that file. We do not error on this.
- **Ordering constraints.** rsconstruct does not enforce "the file owner runs before the directory filler" or vice versa. The snapshot/walk is done after each product finishes, and `path_owner` is a static graph query independent of run order.
- **Partial-directory caching like git trees with subtrees.** The tree descriptor is a flat list of `(path, checksum)` entries, which is enough for this use case.

## Quick reference for processor authors

If you are writing a new processor:

- **Generator / mass generator / explicit with `output_files`**: declare every output file. rsconstruct keeps each of your files safe from `output_dirs` stanzas that share the directory.
- **Explicit with `output_dirs`**: declare the shared directory in `output_dirs`. Do NOT assume the directory is empty when your command runs — other processors may have already contributed files to it. Your command should overwrite only what it produces; it should not wipe the directory.
- **Conflict**: never declare the same path as an output in two different products. That is a graph-build-time error regardless of directory sharing.
