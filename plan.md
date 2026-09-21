# Plan: a Rust implementation for every capability

Goal: everything rsconstruct can do can be done with a Rust-based processor,
either native inside rsconstruct or an external executable written in Rust.
The measure is the `rust` flag on each processor (`is_rust` on the plugin,
shown by `rsconstruct processors list` and `rsconstruct status`, added
2026-09-21): the plan is done when every capability has at least one
processor reporting `rust: true`, and every capability without one is in
the "inherently not Rust" list below with a reason.

**The non-Rust processors stay.** This is about coverage, not replacement:
nothing is retired, deprecated or marked superseded, and no fleet repo is
required to switch. A repo that prefers pylint keeps pylint. The new Rust
processors are additions, sitting beside the existing ones, so that a repo
*can* be built entirely with Rust processors if it chooses to.

This file is the working plan. Each stage has a table with a **Status**
column; update the status and the log at the bottom as items move. Do not
delete rows: a decision not to do something is recorded, not erased.

Status values: `todo`, `in progress`, `done`, `won't do` (with a reason in
Notes), `blocked` (with what on).

## Baseline (2026-09-21)

Measured with the script under *Measuring* below, over the 203 repos under
`~/git` that carry an `rsconstruct.toml`.

| Measure | Value |
|---|---|
| Processors in the registry | 93 |
| Reporting `rust: true` | 28 (20 native + ruff, taplo, rumdl, clippy, cargo, mdbook, pyrefly, rustc) |
| Reporting `rust: false` | 65 |
| Processor declarations across the fleet | 1610 |
| ...served by a Rust processor | 708 (44 %) |
| ...served by a user-supplied command (`explicit`, `generator`, `script`) | 53 |

By usage, Rust already dominates: rumdl (194 repos), taplo (192), ruff
(130), tera (108), zspell (34), ijsonlint (33). The non-Rust load is
concentrated in a handful of processors, which is what orders the stages:

| Non-Rust processor | Repos | Route |
|---|---|---|
| actionlint | 199 | Stage 3: native `iactionlint` |
| mypy | 127 | Stage 1: pyrefly (exists), Stage 2: ty |
| luacheck | 109 | Stage 2: selene |
| shellcheck | 80 | Stage 4 for now (no Rust equivalent), revisit |
| pytest | 62 | Inherent: runs Python |
| sphinx | 51 | Covered: mdbook (Rust) builds documentation; a sphinx repo that wants Rust rewrites its sources for mdbook, which is a per-repo choice, not this plan |
| htmlhint | 29 | Stage 3: native `ihtml` |
| xmllint | 28 | Stage 3: native `ixmllint` |
| yamllint | 27 | Stage 1: iyamllint (exists) |
| cppcheck | 23 | Inherent for now (no Rust C++ analyser) |
| eslint | 21 | Stage 2: oxlint |
| stylelint | 16 | Stage 2: biome, or Stage 3: native `icss` |
| checkstyle | 15 | Inherent: Java |
| hadolint | 15 | Stage 3: native `idockerfile` |
| tidy | 14 | Stage 3: native `ihtml` |

Everything with fewer than ten repos is in the per-stage tables below.

## Principles

- **Native first, wrapper second.** When a Rust crate implements the
  capability (html5ever, roxmltree, minijinja, protox, usvg, lightningcss,
  grass), write a native processor: zero tools to install, one fewer thing
  `tools install` can fail on, and it runs in-process. When the capability is
  a whole Rust program (oxlint, selene, tectonic, uv, oxvg, biome, dprint,
  ty), wrap it as an external processor with a tool-registry entry. Both
  count as Rust; native is preferred when the crate is mature.
- **Additions only.** A new Rust processor lands beside the existing one and
  both stay. Nothing is retired, deprecated or marked superseded; the docs
  page of the existing processor gains one line, "a Rust alternative is X",
  and nothing else changes for anyone using it. Whether a repo switches is
  that repo's decision.
- **A processor is one file** (CLAUDE.md). Each new processor is: the file
  under `src/processors/<category>/`, its `inventory::submit!` entry with
  `is_rust: true`, a docs page under `docs/src/processors/`, a test file
  under `tests/processors/`, and, for a wrapper, a `ToolInfo` entry so
  `tools install` knows how to get the binary. The completeness tests enforce
  the touch-points.
- **Say what the Rust option covers.** Before a row is `done`, run the
  existing tool and the Rust one over the fleet repos that use the existing
  one and diff the findings. Where the Rust option covers less (xmllint's
  XSD validation, hadolint's full rule set), the gap goes into its docs page
  and into the Notes column, so a repo choosing it knows what it gives up.
- **Inherent is a category, not an excuse.** Running Python tests is Python;
  compiling C is a C compiler. Those stay, listed, with the reason. But
  "no Rust equivalent exists today" is a `blocked` status with a date, not a
  `won't do`, because the Rust tool landscape moves.

## Stage 0: measurement

| Item | Status | Notes |
|---|---|---|
| `is_rust` flag on every plugin, shown by `processors list` and `status` | done | 2026-09-21 |
| Fleet usage + coverage script (below) checked in as `scripts/rust_coverage.py` | todo | Prints the two baseline tables; run before and after every stage |
| Coverage number in the log at the bottom, updated per stage | todo | |

## Stage 1: capabilities a Rust processor already covers

No new processor needed; the Rust option exists. What remains per row is
to confirm it really covers the capability (the comparison run from
*Principles*), close any gap found, and add the "Rust alternative" line to
the existing processor's docs page. Ordered by repos using the non-Rust one.

| Capability (non-Rust processor) | Repos | Rust processor | Status | Notes |
|---|---|---|---|---|
| Python type checking (mypy) | 127 | pyrefly | in progress | pyrefly is younger than mypy; run both over the 127 repos and record which mypy checks it lacks. If the gap is large, `ty` (Stage 2) is a second Rust option |
| YAML lint (yamllint) | 27 | iyamllint | in progress | Native; confirm every `.yamllint.yaml` option the fleet sets is honoured |
| Spelling (aspell) | 1 | zspell | in progress | zspell reads Hunspell dictionaries; the one aspell user (veltzer.github.io) checks Hebrew with a compiled aspell dictionary, so the Hebrew Hunspell dictionary must be tried against its allowlist |
| Python formatting check (black) | 0 | ruff | todo | ruff's processor runs `check`; it needs a `format --check` mode (a config field or a `ruff_format` instance) before this row is covered |
| Python lint (pylint) | 0 | ruff | in progress | ruff's `PL` rule set; confirm the pylint rules the fleet's `.pylintrc` enables all have ruff equivalents |
| JSON lint (jsonlint, jq) | 0 | ijsonlint, ijq | done | Native, and the fleet already uses ijsonlint in 33 repos |
| Markdown lint (markdownlint, mdl) | 0 | rumdl | done | Fleet already uses rumdl in 194 repos |
| Templating (jinja2, mako) | 0 | tera | done | tera is native. Jinja2 *syntax* specifically is Stage 3 `ijinja2` |
| Sass compile (sass) | 0 | isass | done | Native |
| PDF merge (pdfunite) | 0 | ipdfunite | done | Native |
| Markdown to HTML (markdown2html) | 0 | imarkdown2html | done | Native |
| TOML lint (taplo) | 192 | itaplo | done | Both are Rust; itaplo is the native one and needs no install |

## Stage 2: wrap existing Rust tools as new external processors

New processor file + `ToolInfo` entry each. Ordered by repos affected.

| Capability (old) | Repos | Rust tool | New processor | Status | Notes |
|---|---|---|---|---|---|
| Lua lint (luacheck) | 109 | selene | `selene` | todo | Largest single win after actionlint. selene needs a `selene.toml` + standard library file per repo; that becomes a fleet-shared file. Compare findings on the 109 repos before switching |
| Python types (mypy) | 127 | ty | `ty` | todo | Alternative to pyrefly if Stage 1 finds gaps. Both are pre-1.0; pick one per the comparison, not both |
| JS/TS lint (eslint, jshint, jslint, standard) | 21 | oxlint | `oxlint` | todo | oxlint reads eslint-style config; the fleet's `.eslint.config.js` needs a one-time translation. biome is the alternative if oxlint's rule coverage falls short |
| CSS/SCSS lint (stylelint) | 16 | biome | `biome` (css) | todo | biome lints CSS but not SCSS; the fleet's SCSS repos need Stage 3 `icss` or stay on stylelint. Decide after counting SCSS vs CSS users |
| Formatting check (prettier) | 0 | biome or dprint | `biome` (format) | todo | No fleet usage; do together with the JS lint row |
| Python deps (pip) | 0 | uv | `uv` creator | todo | uv is already in the tool registry (used by `tools install-deps`); a creator that runs `uv sync` replaces pip. No fleet usage of the pip creator today |
| LaTeX (pdflatex) | 0 | tectonic | `tectonic` | todo | Drop-in for pdflatex on most documents; tectonic bundles its own TeX distribution |
| SVG optimise (svgo) | 0 | oxvg | `oxvg` | todo | Rust port of svgo; check maturity at implementation time |
| Workflow security (part of actionlint) | 199 | zizmor | `zizmor` | todo | Covers the security half of what actionlint does (untrusted inputs, permissions); the syntax half is Stage 3 `iactionlint`. Optional add-on, not a replacement on its own |

## Stage 3: new native processors from Rust crates

Each is a `SimpleChecker`/`SimpleGenerator` over a crate, `is_native: true`,
`is_rust: true`. Ordered by repos affected.

| Capability (old) | Repos | Crate(s) | New processor | Status | Notes |
|---|---|---|---|---|---|
| GitHub workflow lint (actionlint) | 199 | existing `iyamlschema` + vendored SchemaStore `github-workflow.json`; later `regex` for `${{ }}` expression checks | `iactionlint` | todo | First cut is schema validation only, which catches the structural errors actionlint catches (unknown keys, wrong types, missing `runs-on`). Expression and shell-in-`run` checks come after; the docs page lists what is not covered. Zero install: the biggest usability win in the plan |
| HTML lint (htmlhint, tidy, htmllint) | 43 | html5ever, scraper | `ihtml` | todo | Parse errors from html5ever plus a rule set covering what the fleet's `.htmlhintrc` enables (doctype, unique ids, attr quoting, closed tags). tidy's "clean" pass is out of scope; validation only |
| XML well-formedness (xmllint) | 28 | roxmltree or quick-xml | `ixmllint` | todo | Well-formedness only. No Rust XSD validator exists; repos that pass `--schema` today stay on xmllint, and the row stays open for them |
| Dockerfile lint (hadolint) | 15 | dockerfile-parser | `idockerfile` | todo | Implement the DL30xx/DL40xx rules the fleet actually triggers (measured first); hadolint's full set is not the target |
| CSS/SCSS lint (stylelint) | 16 | lightningcss, grass (for SCSS) | `icss` | todo | lightningcss reports parse errors and can flag unknown properties; SCSS goes through grass first. Complements the biome row in Stage 2 |
| SVG lint (svglint) | 2 | usvg | `isvglint` | todo | usvg parse = valid SVG; cheap |
| YAML query (yq) | 0 | serde_yaml + the `ijq` engine | `iyq` | todo | YAML to JSON, then reuse ijq's filter evaluation |
| Jinja2 templates (jinja2) | 0 | minijinja | `ijinja2` | todo | minijinja is Jinja2-compatible; covers the rare template that cannot be ported to tera |
| Protobuf compile (protobuf) | 0 | protox, prost-build | `iprotobuf` | todo | protox is a pure-Rust protoc; output languages limited to what prost generates (Rust) unless protoc plugins are wrapped |
| Shell lint (shellcheck) | 80 | tree-sitter-bash | `ishellcheck` | blocked | No Rust shellcheck exists and a faithful port is a project of its own. Blocked on either a Rust shellcheck appearing or a decision to fund a subset (quoting, `$?` misuse, unset vars). Recorded here so 80 repos are not forgotten |
| C/C++ lint (cpplint) | 1 | regex | `icpplint` | todo | cpplint's rules are regex-level; one repo uses it. Low priority, easy |

## Stage 4: inherently not Rust

These run something that *is* the capability. They keep `rust: false` and
stay in the registry. A row moves out of this table only when a Rust
implementation of the capability itself appears.

| Processor | Repos | Why it stays | Status |
|---|---|---|---|
| pytest, doctest | 62, 0 | Runs Python code; the capability is executing the project's tests | won't do |
| php_lint | 5 | `php -l` is the PHP interpreter checking syntax | won't do |
| make | 2 | Runs the project's Makefiles | won't do |
| cc, cc_single_file, linux_module | 1, 3, 1 | Invoke C/C++ compilers and the kernel build; the compiler is the capability | won't do |
| cppcheck, clang_tidy | 23, 1 | C/C++ static analysis; no Rust analyser of C/C++ exists | blocked (2026-09-21) |
| checkstyle | 15 | Java style checker; no Rust Java linter exists | blocked (2026-09-21) |
| perlcritic, checkpatch | 7, 1 | Perl analysis and a Perl script; no Rust equivalents | blocked (2026-09-21) |
| chromium, drawio, libreoffice | 0 | Drive those applications for rendering; the application is the capability | won't do |
| slidev, marp, mermaid | 0, 1, 1 | Node renderers with no Rust implementation of the format | blocked (2026-09-21) |
| pandoc | 5 | Multi-format conversion; `imarkdown2html` covers Markdown to HTML, nothing in Rust covers the rest | blocked (2026-09-21); note per-repo which conversion is used |
| a2x | 1 | AsciiDoc; no mature Rust AsciiDoc implementation | blocked (2026-09-21) |
| objdump | 0 | binutils; the `object` crate could dump sections but not disassemble faithfully | blocked (2026-09-21) |
| npm, gem | 0 | Package managers of other ecosystems | won't do |
| jekyll, sphinx | 0, 51 | Site and documentation generators for their own source formats. The capability "build documentation" is covered in Rust by mdbook; a sphinx or jekyll repo that wants the Rust path rewrites its sources, which is that repo's decision | covered by mdbook |
| explicit, generator, creator, script | 25, 17, 0, 11 | Run whatever the user configures; the language is the user's | won't do (by design) |

## Stage 5: making the Rust path visible

Nothing here changes any repo's configuration. It makes the Rust option
easy to find and easy to choose.

| Item | Status | Notes |
|---|---|---|
| Docs page of every non-Rust processor names its Rust alternative(s) | todo | One line per page, added as each Stage 1-3 row lands; the page otherwise stays as it is |
| `rsconstruct processors list` shows the Rust alternative | todo | A `Rust alternative` column or a `--rust` filter; whichever reads better in the table. The JSON gains a `rust_alternatives` array |
| `rsconstruct processors recommend` prefers the Rust option where one exists | todo | The recommendation table already maps extensions to processors; where both exist, list the Rust one first and the other as "also" |
| Per-repo switching stays a per-repo decision | won't do | No rsmultigit sweep, no deprecation. A repo owner who wants the Rust path edits its `rsconstruct.toml`; the coverage script's usage column shows the uptake, nothing enforces it |

## How to work an item

1. Measure first: run the existing tool and the Rust candidate over every
   fleet repo that uses the existing one; diff the findings. Write the gaps
   into the row and into the new processor's docs page.
2. For a wrapper: `ToolInfo` in `src/tools.rs` (bare binary name, runtime,
   install methods), then the processor file, `is_rust: true`.
   For a native: the processor file over the crate, `is_native: true`,
   `is_rust: true`; the `native_processors_are_rust` test enforces the pair.
3. Docs page under `docs/src/processors/`, test file under
   `tests/processors/`; `cargo nextest run` must be green with the tool
   installed (tests never skip).
4. `rsconstruct processors defconfig <name>` shows every config field; if
   the new processor needs a config file of its own (selene's `selene.toml`,
   say), a fleet-shared default goes into rsmultigit's shared set so a repo
   that opts in gets a working one.
5. The existing processor's docs page gets its "Rust alternative" line. The
   existing processor is otherwise untouched: not deprecated, not removed,
   and no repo is switched.
6. Update this file: the row's status, and the log below with the new
   coverage number.

## Measuring

Until the script is checked in, this is what produced the baseline. It reads
`rsconstruct --json processors list` and every `rsconstruct.toml` under
`~/git`.

```python
import glob, json, os, re, subprocess

entries = json.loads(subprocess.check_output(["rsconstruct", "--json", "processors", "list"]))
usage = {}
for path in glob.glob(os.path.expanduser("~/git/*/rsconstruct.toml")):
    for m in re.finditer(r"^\[processor\.([a-z0-9_]+)", open(path).read(), re.M):
        usage.setdefault(m.group(1), set()).add(path.split("/")[-2])
rust = {e["name"] for e in entries if e["rust"]}
total = sum(len(v) for v in usage.values())
served = sum(len(v) for k, v in usage.items() if k in rust)
print(f"processors: {len(rust)}/{len(entries)} rust; declarations: {served}/{total} ({100 * served // total} %) served by rust")
for e in sorted(entries, key=lambda e: -len(usage.get(e["name"], ()))):
    if not e["rust"]:
        print(f"{e['name']:16} {len(usage.get(e['name'], ())):4} repos")
```

## Log

| Date | Change | Rust processors | Declarations served by Rust |
|---|---|---|---|
| 2026-09-21 | `is_rust` flag added; baseline measured; plan written | 28 / 93 | 708 / 1610 (44 %) |
