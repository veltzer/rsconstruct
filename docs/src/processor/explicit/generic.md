# Explicit Processor

## Why "explicit"?

Other processor types discover their inputs by scanning directories for files
matching certain extensions. The explicit processor is different: the user
declares exactly which files are inputs and which are outputs. Nothing is
discovered or inferred.

Names considered:
- **explicit** — chosen. Directly communicates the key difference: everything
  is declared rather than discovered.
- **custom** — too generic. Doesn't say what makes it different from the
  existing `generator` processor (which is also "custom").
- **rule** — precise (Bazel/Make terminology for a build rule with explicit
  inputs/outputs), but carries baggage from other build systems and doesn't
  fit the rsconstruct naming convention (processors, not rules).
- **aggregate** — describes the many-inputs-to-few-outputs pattern, but not
  all uses are aggregations.
- **task** — too generic. Could mean anything.

## Purpose

Runs a user-configured script or command with explicitly declared inputs and
outputs. Unlike scan-based processors (which discover one product per source
file), the explicit processor creates a single product with all declared inputs
feeding into all declared outputs.

This is ideal for build steps that aggregate many files into one or a few
outputs, such as:
- Generating an index page from all HTML files in a directory
- Building a bundle from multiple source files
- Creating a report from multiple data files

## How It Works

The processor resolves all `inputs` (literal paths) and `input_globs` (glob
patterns) into a flat file list. It creates a single product with these files
as inputs and the declared `output_files` and `output_dirs` as outputs.

Rsconstruct uses this information for:
- **Rebuild detection**: if any input changes, the product is rebuilt
- **Dependency ordering**: if an input is an output of another processor,
  that processor runs first (automatic via `resolve_dependencies()`)
- **Caching**: outputs are cached and restored on cache hit

A stanza is exactly one product, however many rounds of fixed-point discovery
it takes for its inputs to appear (see [Cross-Processor
Dependencies](#cross-processor-dependencies)). The product's identity is the
stanza, not its first input or its outputs: a later round that resolves more
inputs updates the product rather than adding a second one — which matters
for a stanza with `output_dirs` only, whose product has no output files to be
recognised by.

### Missing inputs

Every literal `inputs` entry must name a file that exists, or a path some
processor declares as its output. An entry that is neither is a config error
(exit code 2), reported once discovery has settled so that files an upstream
generator only produces during the build are not falsely flagged:

```text
Invalid config:
  [processor.explicit.generic.site] inputs entry 'tags/levels.txt' does not exist and no processor declares it as an output
```

Such entries used to be dropped silently, and the command ran without them. A
glob in `input_globs` that matches nothing is not an error (see [Glob
patterns](#glob-patterns)); a stanza whose inputs resolve to nothing at all
creates no product.

`rsconstruct clean` tolerates a missing input, so the outputs of a build whose
sources have since gone can still be removed.

## Invocation

The command is invoked as:

```text
command [args...] --inputs <input1> <input2> ... [--output-files <file>...] [--output-dirs <dir>...]
```

### Input ordering

Inputs are passed in a deterministic order:
1. `inputs` entries first, in config file order
2. `input_globs` results second, one glob at a time in config file order,
   files within each glob sorted alphabetically

This ordering is stable across builds (assuming the same set of files exists).

Because the resolved list is what the command receives, `inputs` and
`input_globs` are part of the product's cache key (`affects_output`): the same
files listed in another order, or reached through another pattern, are a
config change and rerun the command.

## Configuration

```toml
[processor.explicit.generic.site]
command = "scripts/build_site.py"
args = ["--verbose"]
inputs = [
    "resources/index.html",
    "resources/index.css",
    "resources/index.js",
    "tags/level.txt",
    "tags/category.txt",
    "tags/audiences.txt",
]
input_globs = [
    "docs/courses/**/*.html",
    "docs/tracks/*.html",
]
output_files = [
    "docs/index.html",
]
```

### Fields

| Key | Type | Required | Description |
|---|---|---|---|
| `command` | string | yes | Script or binary to execute |
| `args` | array of strings | no | Extra arguments passed before `--inputs` |
| `inputs` | array of strings | no | Literal input file paths; each must exist or be an upstream processor's declared output |
| `input_globs` | array of strings | no | Glob patterns resolved to input files |
| `output_files` | array of strings | no | Output file paths produced by the command |
| `output_dirs` | array of strings | no | Output directories produced by the command (cached as a tree) |

At least one of `inputs` or `input_globs` must be specified, and at least one
of `output_files` or `output_dirs`.

### Glob patterns

`input_globs` supports standard glob syntax:
- `*` matches any sequence of characters within a path component
- `**` matches any number of path components (recursive)
- `?` matches a single character
- `[abc]` matches one of the listed characters

Glob results that match no files are silently ignored (the set of matching
files may grow as upstream generators produce outputs via the fixed-point
discovery loop).

## Cross-Processor Dependencies

The explicit processor works naturally with the fixed-point discovery loop.
If `input_globs` matches files that are outputs of other processors (e.g.,
pandoc-generated HTML files), rsconstruct automatically:
1. Injects those declared outputs as virtual files during discovery
2. Resolves dependency edges so upstream processors run first
3. Rebuilds the explicit processor when upstream outputs change

This means you do not need to manually order processors or wait for a second
build — everything is handled in a single build invocation.

## Comparison with Other Processor Types

| | Checker | Generator | Explicit |
|---|---|---|---|
| Products | one per input file | one per input file | one total |
| Outputs | none (pass/fail) | one per input | explicitly listed |
| Discovery | src_dirs + src_extensions | src_dirs + src_extensions | declared inputs/globs |
| Use case | lint/validate files | transform files 1:1 | aggregate many → few |

## Clean behavior

This is the Explicit processor — `rsconstruct clean outputs` removes each declared `output_files` entry individually and each declared `output_dirs` entry recursively. After all per-product cleans complete, the orchestrator removes any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
