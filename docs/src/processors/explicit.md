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
as inputs and the `outputs` list as outputs.

Rsconstruct uses this information for:
- **Rebuild detection**: if any input changes, the product is rebuilt
- **Dependency ordering**: if an input is an output of another processor,
  that processor runs first (automatic via `resolve_dependencies()`)
- **Caching**: outputs are cached and restored on cache hit

## Invocation

The command is invoked as:

```
command [args...] --inputs <input1> <input2> ... --outputs <output1> <output2> ...
```

### Input ordering

Inputs are passed in a deterministic order:
1. `inputs` entries first, in config file order
2. `input_globs` results second, one glob at a time in config file order,
   files within each glob sorted alphabetically

This ordering is stable across builds (assuming the same set of files exists).

## Configuration

```toml
[processor.explicit.site]
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
outputs = [
    "docs/index.html",
]
```

### Fields

| Key | Type | Required | Description |
|---|---|---|---|
| `command` | string | yes | Script or binary to execute |
| `args` | array of strings | no | Extra arguments passed before `--inputs` |
| `inputs` | array of strings | no | Literal input file paths |
| `input_globs` | array of strings | no | Glob patterns resolved to input files |
| `outputs` | array of strings | yes | Output file paths produced by the command |

At least one of `inputs` or `input_globs` must be specified.

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
| Discovery | scan dirs + extensions | scan dirs + extensions | declared inputs/globs |
| Use case | lint/validate files | transform files 1:1 | aggregate many → few |
