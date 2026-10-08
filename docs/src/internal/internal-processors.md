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

### iyamllint — YAML linting (done)

Replaces: `yamllint` (Python YAML linter)

A full port of yamllint lives in `src/yamllint/`: its config format, its 23
rules and its directives, verified identical to yamllint 1.38.0 over the
fleet's YAML files. Tokens come from `libyaml-safer`, a safe port of libyaml,
whose scanner matches PyYAML's token for token (both are Kirill Simonov's
design), which is what lets the rules be ported line for line.

**Crate:** `libyaml-safer`
**Complexity:** Medium — the rules are mechanical ports; the token-stream
details (marks, comment extraction, implicit block sequences) are where
fidelity is won or lost

### ixmllint — XML well-formedness and XSD validation (done)

Replaces: `xmllint --noout [--schema x.xsd]` (libxml2)

`src/xml/` tokenizes with `xmlparser` (roxmltree's tokenizer) and implements
above it what xmllint checks: tag matching, duplicate attributes, entity and
character references against the internal DTD subset, namespace
declarations and prefixes, document shape; then an XSD validator for the
schema constructs in use (sequence/choice/all content models with
occurrence bounds, mixed and simple content, attributes, facets, lists,
unions, extension, include). Messages, lines and verdicts are libxml2's,
including its quirks: a duplicate attribute is reported where the start tag
ends, "premature end" only when nothing else failed, a relative namespace
URI is warned about only on the default namespace. Checked against xmllint
over the fleet's 6256 XML/SVG files plus hand-made edge cases: same
verdicts, same lines. A schema using an unsupported construct is a build
failure naming it, never a partial check.

**Crate:** `xmlparser`
**Complexity:** Medium — the tokenizer gives the lexical level for free; the
libxml2 behaviours above are what the differential run against xmllint
pinned down

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

### imarkdown2html — Markdown to HTML

Replaces: `markdown2html` (external markdown CLI)

Convert Markdown files to HTML. `pulldown-cmark` is a fast, CommonMark-compliant
Markdown parser written in Rust.

**Crate:** `pulldown-cmark`
**Complexity:** Low — parse and render to HTML string, write to output file

### iyamlschema — YAML Schema Validation

Validates YAML files against JSON schemas referenced by `$schema` URLs.
Fetches and caches schemas via the webcache, validates data against the schema
(including remote `$ref` resolution), and checks property ordering.

**Crate:** `jsonschema`, `ureq`, `serde_yaml_ng`
**Complexity:** Medium — HTTP fetching, schema compilation, recursive ordering checks

### yaml2json — YAML to JSON Conversion

Convert YAML files to pretty-printed JSON.

**Crate:** `serde_yaml_ng`, `serde_json`
**Complexity:** Low — parse YAML, serialize as JSON

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

## Binary Plugin System

As of now, rsconstruct does not have a binary plugin system. This section documents the approach for future consideration.

Rust applications can dynamically load plugins written in Rust via `dlopen`/`dlsym` on shared libraries (`.so` on Linux, `.dylib` on macOS, `.dll` on Windows). The plugin compiles as a `cdylib` crate, exports `extern "C"` functions, and the host loads them at runtime using a crate like `libloading`.

The main constraint is that Rust has no stable ABI. You cannot use Rust traits, generics, or standard library types across the dynamic library boundary. The plugin interface must be C-compatible: `extern "C"` functions returning opaque pointers, with a vtable or function-pointer struct defining the plugin API.

Crates like `abi_stable` attempt to provide a stable ABI layer for Rust-to-Rust dynamic loading, but they add significant complexity.

The current Lua plugin system avoids this problem entirely — Lua has a stable, simple FFI. A binary plugin system would offer better performance but at the cost of a much more complex plugin interface and build process (plugins would need to be compiled separately and matched to the host's ABI).
