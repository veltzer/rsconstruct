# Internal Processors

Processors that can be reimplemented in pure Rust, eliminating external tool dependencies.
Internal processors are faster (no subprocess overhead), require no installation,
and work on any platform with rsconstruct.

The naming convention is to prefix with `i` (for internal), e.g., `ipdfunite` replaces `pdfunite`.
Both the original and internal variants coexist — users choose which to use.

## Implemented

### ipdfunite

Replaces: `pdfunite` (external `pdfunite` binary from poppler-utils)

Merges PDFs from subdirectories into course bundles using `lopdf` in-process.
Same config as `pdfunite` minus the `pdfunite_bin` field. Batch-capable.

**Crate:** `lopdf`

## Candidates

### ijq / ijsonlint — JSON validation

Replaces: `jq` (checks JSON parses) and `jsonlint` (Python JSON linter)

Both tools ultimately just validate that files are well-formed JSON.
`serde_json` is already a dependency — parse each file and report errors.

**Crate:** `serde_json` (already in deps)
**Complexity:** Low — parse file, report error with line/column

### iyamllint — YAML validation

Replaces: `yamllint` (Python YAML linter)

Validate that YAML files parse correctly. `yamllint` also checks style rules
(line length, indentation, etc.) which would need to be reimplemented if desired,
but basic validity checking is trivial.

**Crate:** `serde_yaml`
**Complexity:** Low for validation only, medium if style rules are needed

### itaplo — TOML validation

Replaces: `taplo` (TOML formatter/linter)

Validate that TOML files parse correctly. The `toml` crate is already a dependency.
`taplo` also reformats — a pure validation-only internal processor covers the common case.

**Crate:** `toml` (already in deps)
**Complexity:** Low

### ijson_schema — JSON Schema validation

Replaces: `json_schema` (Python `jsonschema`)

Validate JSON files against JSON Schema definitions. The `jsonschema` Rust crate
supports JSON Schema draft 2020-12, draft 7, and draft 4.

**Crate:** `jsonschema`
**Complexity:** Medium — need to load schema files and validate against them

### imarkdown — Markdown to HTML

Replaces: `markdown` (external markdown CLI)

Convert Markdown files to HTML. `pulldown-cmark` is a fast, CommonMark-compliant
Markdown parser written in Rust.

**Crate:** `pulldown-cmark`
**Complexity:** Low — parse and render to HTML string, write to output file

### isass — Sass/SCSS to CSS

Replaces: `sass` (Dart Sass CLI)

Compile Sass/SCSS files to CSS. The `grass` crate is a pure-Rust Sass compiler
with good compatibility.

**Crate:** `grass`
**Complexity:** Low — compile input file, write CSS output

## Not Suitable for Internal Implementation

These processors wrap tools with complex, evolving behavior that would be
impractical to reimplement:

- **ruff, pylint, mypy, pyrefly** — Python linters/type checkers with deep language understanding
- **eslint, jshint, stylelint** — JavaScript/CSS linters with plugin ecosystems
- **clippy, cargo** — Rust toolchain components
- **marp** — Presentation framework (spawns Chromium)
- **sphinx, mdbook, jekyll** — Full documentation/site generators
- **shellcheck** — Shell script analyzer with extensive rule set
- **aspell** — Spell checker with language dictionaries
- **chromium, libreoffice, drawio** — GUI applications used for rendering
- **protobuf** — Protocol buffer compiler
- **pdflatex** — LaTeX to PDF (entire TeX distribution)
