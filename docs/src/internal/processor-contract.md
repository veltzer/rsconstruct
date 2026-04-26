# Processor Contract

Rules that all processors must follow.

## Fail hard, never degrade gracefully

When something fails, it must fail the entire build. Do not try-and-fallback,
do not silently substitute defaults for missing resources, do not swallow errors.
If a processor is configured to use a file and that file does not exist, that is
an error. The user must fix their configuration or their project, not the code.

Optional features must be opt-in via explicit configuration (default off).
When the user enables a feature, all resources it requires must exist.

## No work without source files

An enabled processor must not fail the build if no source files match its
file patterns. Zero matching files means zero products discovered; the
processor simply does nothing. This is not an error — it is the normal
state for a freshly initialized project.

## Single responsibility

Each processor handles one type of transformation or check.
A processor discovers its own products and knows how to execute, clean, and
report on them.

## Deterministic discovery

`discover()` receives an `instance_name` parameter identifying the processor
instance (e.g., `"ruff"` or `"script.lint_a"` for multi-instance processors).
Use this name when calling `graph.add_product()` — do not use hardcoded
processor type constants.

`discover()` must return the same products given the same filesystem state.
File discovery, processor iteration, and topological sort must all produce
sorted, deterministic output so builds are reproducible.

## Incremental correctness

Products must declare all their inputs. If any declared input changes,
the product is rebuilt. If no inputs change, the cached result is reused.
Processors must not rely on undeclared side inputs for correctness
(support files read at execution time but excluded from the input list
are acceptable only when changes to those files can never cause a
previously-passing product to fail).

## Execution isolation

A processor's `execute()` must only write to the declared output paths
(or, for creators, to the expected output directory).
It must not modify source files, other products' outputs, or global state.

## Cleaning

A processor's `clean(product, verbose)` is responsible only for removing
what *that* product produced. Concretely:

- **Checkers** override nothing — the default `clean()` returns 0 (no outputs).
- **Generators** must remove every file in `product.outputs` and only those
  files. The `clean_outputs()` helper does exactly this with `fs::remove_file`.
  Generators must not recurse into directories.
- **Creators** must remove every directory in `product.output_dirs`
  recursively (the contents are unenumerated by design — the external build
  tool produces an unknown set of files inside). The `clean_output_dir()`
  helper handles this. Creators must not delete anything outside their
  declared `output_dirs`.
- **Explicit** processors run both — files for `output_files`, recursive
  removal for any user-declared `output_dirs`.
- **Lua plugins** may override `clean(product)`; the default falls back to
  file-only removal.

Recursive directory removal is reserved for Creators (and the user-declared
`output_dirs` of Explicit). All other processor types remove individually
declared files and nothing else.

After every product's `clean()` has run, the executor performs an
**empty-directory sweep**: it collects every parent directory of every
removed output (and every parent of every removed `output_dir`), sorts
deepest-first, and calls `fs::remove_dir` on each one. `remove_dir` is
non-recursive, so it only succeeds on already-empty directories — anything
still containing files is left in place. The sweep climbs upward through
parents until it hits a non-empty directory or the project root.

The sweep is part of the orchestrator, not the per-processor `clean()`.
Processors should not pre-empt it by removing parent directories themselves
— let the sweep handle directory cleanup so the rule "files I produced,
nothing else" stays clean. Users can disable the sweep with
`rsconstruct clean outputs --no-empty-dirs`.

## Output directory caching (creators)

Creators that set `output_dir` on their products get automatic
directory-level caching. After successful execution, the executor walks
the output directory, stores every file as a content-addressed object,
and records a manifest with paths, checksums, and Unix permissions.
On restore, the entire directory is recreated from cache.

The `cache_output_dir` config option (default `true`) controls this.
When disabled, creators fall back to stamp-file or empty-output
caching (no directory restore on `rsconstruct clean && rsconstruct build`).

Creators that use output_dir caching must implement `clean()` to
remove the output directory so it can be restored from cache.

## Error reporting

On failure, `execute()` returns an `Err` with a clear message including
the relevant file path and the nature of the problem. The executor
decides whether to abort or continue based on `--keep-going`.

## Batch execution and partial failure

Batch-capable processors implement `supports_batch()` and `execute_batch()`.
The `execute_batch()` method receives multiple products and must return one
`Result` per product, in the same order as the input.

**External tool processors** that invoke a single subprocess for the entire
batch typically use `execute_generator_batch()`, which maps a single exit code
to all-success or all-failure. If the tool fails, all products in the batch
are marked failed — there is no way to determine which outputs are valid.

**Internal processors** (e.g., `imarkdown2html`, `isass`, `ipdfunite`) that process
files in-process should return per-file results so that partial failure is
handled correctly — only the actually-failed products are rebuilt on the next run.

**Chunk sizing:** In fail-fast mode (default), the executor uses `chunk_size=1`
even for batch-capable processors, so each product is cached individually. This
gives the best incremental recovery. Larger chunks are used only with
`--keep-going` or explicit `--batch-size`.
