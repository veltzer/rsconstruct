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

### iactionlint — GitHub Actions workflow linting (done)

Replaces: `actionlint` (Go), except its `shellcheck` and `pyflakes` rules,
which run external tools over `run:` scripts.

`src/actionlint/` is a file-by-file port of actionlint v1.7.12: the YAML
tree with go-yaml's tag resolution and alias handling, the workflow parser
with its `syntax-check` messages, the `${{ }}` lexer, parser, type system
and semantics checker (including untrusted-input tracking), and the fifteen
rules. The popular-actions data set actionlint ships is embedded as JSON,
generated from `popular_actions.go` by
`scripts/gen-actionlint-popular-actions.py`. Verified against actionlint's
own test corpus (`testdata/err`, `examples`, `ok`, `projects`): all 740
reported problems identical in position and wording; the fleet's 343
workflow files are clean under both tools.

**Crate:** `libyaml-safer` (events with marks, like go-yaml's), `regex`,
`serde_json`
**Complexity:** High — the expression type checker and the position rules
(go-yaml's node positions, text/scanner columns inside expressions) are
where fidelity is won or lost

### idockerfile — Dockerfile linting (done)

Replaces: `hadolint` (hadolint 2.15.1, Haskell), except its ShellCheck
findings (`SC` codes), which come from a separate program.

`src/dockerfile/` ports hadolint's own half: the Dockerfile parser
(language-docker 16.0.0's grammar, combinator by combinator: escaped line
breaks, heredocs, `--mount` arguments, exec-form arrays, pragmas), the view
of `RUN` scripts the rules query (commands, arguments, flags, pipes; a shell
reader written for those queries, with the quirks of hadolint's walk over
ShellCheck's tree copied: every command is yielded twice, an unparsable
script yields nothing), the `# hadolint ignore=` / `stage ignore=` / `global
ignore=` pragmas, `.hadolint.yaml` (ignored, override, trustedRegistries,
label-schema, strict-labels, failure-threshold), all 71 `DL` rules with
their messages, and hadolint's exit rule (fail at or above the threshold).
Verified against hadolint over the fleet's 154 Dockerfiles with and without
the fleet's config and 2000 mutants: identical `DL` findings. Known
divergence: hadolint 2.15.1's parser rejects a heredoc followed directly by
the next instruction (fixed upstream); idockerfile accepts it.

**Crate:** `serde_yaml_ng` (the configuration file)
**Complexity:** High — 71 rules is the easy part; the shell view and the
parser's whitespace rules are where fidelity is won or lost

### itidy — HTML checking (done)

Replaces: `tidy -errors -q [-config x] [--option value]` (HTML Tidy 5.8.0, C)

`src/tidy/` is a port of tidy's checking half: the UTF-8 stream reader
(streamio.c, with its tab expansion and 64-entry column ring for pushed-back
characters), the lexer (lexer.c: tokens, entities, attributes, doctype
declarations, the inline stack), the element parsers (parser.c, one routine
per content model), the attribute and element checks (attrs.c, tags.c), the
default clean-up passes (clean.c: style elements, nested emphasis, the meta
charset, the doctype, HTML5-removed and proprietary elements) and the
message table (message.c, language_en.h). The tag, attribute and entity
tables are generated from tidy's sources by `scripts/gen-itidy-tables.py`.
The pretty-printer is not ported. Every tidy option is parsed and validated;
options that only shape the output are ignored and the few whose effect is
not reproduced (`clean`, `gdoc`, `bare`, `word-2000`, `logical-emphasis`,
`input-xml`, `accessibility-check`, `mute`, non-UTF-8 encodings) are
refused. Verified against tidy over the fleet's 599 HTML files and 6000
mutated copies, under the fleet's option sets: identical report lines in
the same order. The quirks that had to be copied: an empty attribute value
is treated as absent (`tmbstrndup` returns NULL for length 0), an ESC
character does not count as a column, and whether an emptied text node was
blank is decided from a stale byte past its end.

**Crate:** none (serde_json for the generated tables)
**Complexity:** High — tidy's parser repairs as it reads, so every report's
position depends on the exact token stream; the differential run is the
only way to know the port is right

### isvglint — SVG linting (done)

Replaces: `svglint` (svglint 4.2.1, Node), except its `custom` rules,
which are JavaScript functions.

`src/svglint/` ports the three built-in rules and what they run on: the
lenient parser (htmlparser2 3.10.1 in XML mode without entity decoding,
with its chained node start positions, over UTF-16 code units so columns
agree), a CSS selector engine with css-select's XML-mode semantics (the
seven attribute operators, four combinators, `:not`/`:is`/`:has`, the
`-child`/`-of-type` families, `:empty`, `:contains`), fast-xml-parser
5.10.1's validator for `valid`, and the `elm` and `attr` rules with their
messages. Rules come from TOML (`valid`, an `elm` table, `attr` tables)
instead of `.svglintrc.js`; selectors or rule values svglint would only
warn about are configuration errors. Verified against svglint over the
fleet's 5776 SVG files and 3000 mutants under the default rules and two
rule sets covering every selector and attribute feature: identical
findings. Positions are printed 1-based (svglint prints them 0-based and
miscounts after a leading newline).

**Crate:** `regex` (for `{ regex = ... }` attribute values)
**Complexity:** Medium — three small libraries to port faithfully rather
than one large one; the lenient parser's position chaining and the
validator's blind spots (self-closing roots, declarations after a tag) are
what the differential run pinned down

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
