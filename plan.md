# Plan: a Rust implementation for every capability

Goal: everything rsconstruct can do can be done with a Rust-based processor,
either builtin inside rsconstruct or an external executable written in Rust.
The measure is the `rust` flag on each processor (`is_rust` on the plugin,
shown by `rsconstruct processor list` and `rsconstruct status`, added
2026-09-21): the plan is done when every capability has at least one
processor reporting `rust: true`, and every capability without one is in
the "inherently not Rust" list below with a reason.

**The non-Rust processors stay.** This is about coverage, not replacement:
nothing is retired, deprecated or marked superseded, and no fleet repo is
required to switch. A repo that prefers pylint keeps pylint. The new Rust
processors are additions, sitting beside the existing ones, so that a repo
*can* be built entirely with Rust processors if it chooses to.

This file is the working plan. Each stage has a table with a **Status**
column; update the status and the log at the bottom as items move. A row
that is done is deleted (the log records the coverage it bought); a decision
not to do something stays, so it is recorded rather than forgotten.

Status values: `todo`, `in progress`, `won't do` (with a reason in Notes),
`blocked` (with what on).

## Baseline (2026-09-21)

Measured with the script under *Measuring* below, over the 203 repos under
`~/git` that carry an `rsconstruct.toml`.

| Measure | Value |
|---|---|
| Processors in the registry | 93 |
| Reporting `rust: true` | 28 (20 builtin + ruff, taplo, rumdl, clippy, cargo, mdbook, pyrefly, rustc) |
| Reporting `rust: false` | 65 |
| Processor declarations across the fleet | 1610 |
| ...served by a Rust processor | 708 (44 %) |
| ...served by a user-supplied command (`explicit`, `generator`, `script`) | 53 |

By usage, Rust already dominates: rumdl (194 repos), taplo (192), ruff
(130), tera (108), zspell (34), ijsonlint (33). The non-Rust load is
concentrated in a handful of processors, which is what orders the stages:

| Non-Rust processor | Repos | Route |
|---|---|---|
| actionlint | 199 | Stage 3: builtin `iactionlint` (done, fleet switched 2026-10-09) |
| mypy | 127 | Stage 1: pyrefly (done, fleet switched; no repo runs mypy) |
| luacheck | 109 | Stage 3: builtin `iluacheck` (done, fleet switched 2026-10-09) |
| shellcheck | 80 | Stage 3: builtin `ishellcheck` (done, fleet switched 2026-10-11) |
| pytest | 62 | Inherent: runs Python |
| sphinx | 51 | Covered: mdbook (Rust) builds documentation; a sphinx repo that wants Rust rewrites its sources for mdbook, which is a per-repo choice, not this plan |
| htmlhint | 0 | Was 29 at the first count; the fleet's HTML checking is all `tidy` now (0 htmlhint, 0 htmllint) |
| xmllint | 28 | Stage 3: builtin `ixmllint` (done, fleet switched 2026-10-08) |
| yamllint | 27 | Stage 1: iyamllint (done, fleet switched 2026-10-08) |
| cppcheck | 23 | Inherent for now (no Rust C++ analyser) |
| eslint | 21 | Stage 2: oxlint (done, fleet switched; no repo runs eslint) |
| stylelint | 16 | Stage 2: biome (done, fleet switched; no repo runs stylelint) |
| checkstyle | 15 | Stage 3: builtin `icheckstyle` (done, fleet switched 2026-10-11) |
| hadolint | 15 | Stage 3: builtin `idockerfile` (done, fleet switched 2026-10-09) |
| tidy | 29 | Stage 3: builtin `itidy` (done, fleet switched 2026-10-09) |
| perlcritic | 7 | Stage 3: builtin `iperlcritic` (done, fleet switched 2026-10-11) |

Everything with fewer than ten repos is in the per-stage tables below.

## Principles

- **Mind the original's license.** A port written by translating another
  tool's source is a derivative of it. Six engines are ports of GPL, LGPL or
  Artistic-licensed code while rsconstruct is MIT; see
  `docs/src/internal/licensing.md` (recorded 2026-10-11, undecided). New
  engines are not ported from GPL or LGPL source; where the original is
  copyleft, write from its documented and observed behaviour instead.
- **Builtin first, wrapper second.** When a Rust crate implements the
  capability (html5ever, roxmltree, minijinja, protox, usvg, lightningcss,
  grass), write a builtin processor: zero tools to install, one fewer thing
  `tool install` can fail on, and it runs in-process. When the capability is
  a whole Rust program (oxlint, selene, tectonic, uv, oxvg, biome, dprint,
  ty), wrap it as an external processor with a tool-registry entry. Both
  count as Rust; builtin is preferred when the crate is mature.
- **Additions only.** A new Rust processor lands beside the existing one and
  both stay. Nothing is retired, deprecated or marked superseded; the docs
  page of the existing processor gains one line, "a Rust alternative is X",
  and nothing else changes for anyone using it. Whether a repo switches is
  that repo's decision.
- **A processor is one file** (CLAUDE.md). Each new processor is: the file
  under `src/processor/<category>/`, its `inventory::submit!` entry with
  `is_rust: true`, a docs page under `docs/src/processor/<type>/`, a test file
  under `tests/processor/`, and, for a wrapper, a `ToolInfo` entry so
  `tool install` knows how to get the binary. The completeness tests enforce
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
| Fleet usage + coverage script (below) checked in as `scripts/rust_coverage.py` | done | Prints the two baseline tables (with each non-Rust processor's Rust alternatives); run before and after every stage |
| Coverage number in the log at the bottom, updated per stage | done | Measured 2026-10-11 with the script |

## Stage 1: capabilities a Rust processor already covers

No new processor needed; the Rust option exists. What remains per row is
to confirm it really covers the capability (the comparison run from
*Principles*), close any gap found, and add the "Rust alternative" line to
the existing processor's docs page. Ordered by repos using the non-Rust one.

| Capability (non-Rust processor) | Repos | Rust processor | Status | Notes |
|---|---|---|---|---|
| Python type checking (mypy) | 127 | pyrefly | done | The fleet runs pyrefly in all 127 repos and mypy in none; the unused `mypy` dev dependency was dropped from the 7 repos still declaring it (2026-10-11). demos-lang-python keeps mypy as a dependency: it is the subject of an example there |
| YAML lint (yamllint) | 27 | iyamllint | done | Builtin. All 23 yamllint rules with every option, `.yamllint.yaml` read as yamllint reads it (`extends`, levels, `ignore`, directives), verified identical to yamllint 1.38.0 over the fleet's 362 YAML files (`src/engines/yamllint/`). Not supported: the `locale` key (key-ordering collates by code point) and the user-level `~/.config/yamllint/config`. Switching a repo is `yamllint` → `iyamllint` in rsconstruct.toml; the fleet switched on 2026-10-08 (26 repos, `yamllint` dropped from their pyproject dev groups) |
| Spelling (aspell) | 1 | zspell | in progress | zspell reads Hunspell dictionaries; the one aspell user (veltzer.github.io) checks Hebrew with a compiled aspell dictionary, so the Hebrew Hunspell dictionary must be tried against its allowlist |
| Python lint (pylint) | 0 | ruff | in progress | ruff's `PL` rule set; confirm the pylint rules the fleet's `.pylintrc` enables all have ruff equivalents |

## Stage 2: wrap existing Rust tools as new external processors

New processor file + `ToolInfo` entry each. Ordered by repos affected.

| Capability (old) | Repos | Rust tool | New processor | Status | Notes |
|---|---|---|---|---|---|
| Lua lint (luacheck) | 109 | none (mlua for `.luacheckrc`) | `iluacheck` | done | `src/engines/luacheck/`: a port of luacheck 1.2.0 (lexer, parser, linearization, local resolution, every detector, inline options, options/standards/filter with cross-file implicit globals, `.luacheckrc` run in the embedded Lua; built-in standards generated by `scripts/gen-iluacheck-tables.lua`). Verified against luacheck over 7101 Lua files on a development machine, luacheck's own sources, the fleet's files and 3000 mutants, under defaults, ten option sets and three configurations: identical (about 2.4 million findings). One product per stanza, since implicit globals span files. The earlier `selene` wrapper stays beside it: selene parses Lua 5.1 only and needs a different configuration, so a shared `selene.toml` passed 104 of the 109 repos. The fleet switched on 2026-10-09 with release 0.9.121 (109 repos, none kept luacheck; dots' `args = ["--globals", "vim", "--"]` became `globals = ["vim"]`) |
| Python types (mypy) | 127 | ty | `ty` | won't do | The fallback in case pyrefly fell short; the fleet moved to pyrefly instead |
| Python deps (pip) | 0 | uv | `uv` | todo | uv is already in the tool registry (used by `tool install-deps`); a processor that runs `uv sync` replaces pip, shaped as `processor.mass_generator.pip` is. No fleet usage of pip today |
| LaTeX (pdflatex) | 0 | tectonic | `tectonic` | todo | Drop-in for pdflatex on most documents; tectonic bundles its own TeX distribution |
| SVG optimise (svgo) | 0 | oxvg | `oxvg` | todo | Rust port of svgo; check maturity at implementation time |
| Workflow security (part of actionlint) | 199 | zizmor | `zizmor` | todo | Covers the security half of what actionlint does (untrusted inputs, permissions); the syntax half is Stage 3 `iactionlint`. Optional add-on, not a replacement on its own |

## Stage 3: new builtin processors from Rust crates

Each is a `SimpleChecker`/`SimpleGenerator` over a crate, `is_builtin: true`,
`is_rust: true`. Ordered by repos affected.

| Capability (old) | Repos | Crate(s) | New processor | Status | Notes |
|---|---|---|---|---|---|
| GitHub workflow lint (actionlint) | 199 | libyaml-safer, regex, serde_json | `iactionlint` | done | `src/engines/actionlint/`: a file-by-file port of actionlint v1.7.12 (syntax-check, expression type checking, all fifteen rules, the popular-actions data set embedded). Not ported: the shellcheck and pyflakes rules, which run external tools. Verified against actionlint's own test corpus: all 740 problems identical. Switching a repo is `actionlint` → `iactionlint`; a repo that wants shellcheck over `run:` scripts stays on actionlint. The fleet switched on 2026-10-09 (199 repos, none kept actionlint; the shared `.github/actionlint.yaml` still lists `ubuntu-26.04`, which iactionlint, like actionlint 1.7.12, does not know) |
| HTML checking (tidy) | 29 | none (serde_json for the generated tables) | `itidy` | done | `src/engines/tidy/`: a port of HTML Tidy 5.8.0's checking half (stream reader, lexer, element parsers, attribute and element checks, default clean-up passes, message table; tables generated from tidy's sources by `scripts/gen-itidy-tables.py`). The pretty-printer is not ported. Every tidy option is accepted and validated; the ones whose effect is not reproduced (`clean`, `gdoc`, `bare`, `word-2000`, `logical-emphasis`, `input-xml`, `accessibility-check`, `mute`, non-UTF-8 encodings) are refused by name. Verified against `tidy -errors -q` over the fleet's 599 HTML files and 6000 mutants under the fleet's option sets: identical report lines. The original `ihtml` idea (html5ever + an htmlhint rule set) was dropped when a recount showed the fleet's 29 HTML-checking repos all use tidy and none use htmlhint or htmllint: a faithful tidy port is what lets them switch without new findings. Switching a repo is `tidy` → `itidy`, `-config x` → `config_file = "x"`, `--name value` → `options = ["name: value"]`. The fleet switched on 2026-10-09 (29 repos, none kept tidy) |
| XML well-formedness and XSD validation (xmllint) | 28 | xmlparser | `ixmllint` | done | `src/engines/xmllint/`: well-formedness, namespaces and an XSD validator covering the constructs the fleet uses (unsupported ones are a build failure naming them). Verified against xmllint over the fleet's 6256 XML/SVG files and the java-keynote schema: same verdicts, lines and messages. Switching a repo is `xmllint` → `ixmllint` (`args = ["--schema", x]` → `schema = x`); DTD and RelaxNG validation stay on xmllint. The fleet switched on 2026-10-08 (27 repos; java-keynote keeps its schema via `schema = "xsd/keynote.xsd"`) |
| Dockerfile lint (hadolint) | 15 | serde_yaml_ng | `idockerfile` | done | `src/engines/hadolint/`: a port of hadolint 2.15.1's own half: language-docker's Dockerfile grammar, the command view of `RUN` scripts its rules query (a shell reader written for those queries; hadolint's walk yields every command twice and an unparsable script yields nothing, both copied), the ignore pragmas, `.hadolint.yaml`, all 71 `DL` rules and the exit rule. Not ported: ShellCheck (`SC` codes), a separate program. The "rules the fleet triggers" idea was dropped: measured first (15 of the 71 fire on the fleet), but the full set costs little more once the parser and shell view exist, and parity is what lets a repo switch without new findings. Verified against hadolint over the fleet's 154 Dockerfiles (with and without the fleet's config) and 2000 mutants: identical `DL` findings. Known divergence: hadolint 2.15.1 rejects a heredoc followed directly by the next instruction (a parser bug fixed upstream); idockerfile accepts it. The fleet switched on 2026-10-09 with release 0.9.120 (15 repos, none kept hadolint, so the fleet no longer runs ShellCheck over Dockerfile `RUN` scripts) |
| CSS/SCSS lint (stylelint) | 16 | lightningcss, grass (for SCSS) | `icss` | won't do | biome (Stage 2) covers the fleet's CSS: counted 2026-10-03, of the 16 stylelint repos only veltzer.github.io has SCSS, and only one authored file (`sass/style.scss`), which stays on stylelint. Revisit only if SCSS spreads |
| SVG lint (svglint) | 0 | regex | `isvglint` | done | `src/engines/svglint/`: a port of svglint 4.2.1's built-in rules (`valid`, `elm`, `attr`) over ports of what they run on: htmlparser2 3.10.1's lenient XML-mode parser, css-select's selector semantics and fast-xml-parser 5.10.1's validator. Rules are TOML (`valid`, an `elm` table, `attr` tables) instead of `.svglintrc.js`; JavaScript `custom` rules are not portable. Verified against svglint over the fleet's 5776 SVG files and 3000 mutants under three rule sets: identical findings. The usvg idea was dropped: usvg accepts what svglint rejects and vice versa, and the fleet's two svglint repos at the first count have since moved to `ixmllint`/scripts, so parity with svglint (the tool a future user would compare against) is the useful target. The `rssvglint` repo is a 15-line stub and is superseded by this |
| YAML query (yq) | 0 | serde_yaml_ng, jaq-core/jaq-std/jaq-json | `iyq` | done | `src/processor/checker/iyq.rs`: every document of the file read as YAML (a leading byte order mark skipped, as PyYAML does), turned into JSON and run through the configured jq `filter` (default `.`) with jaq; a YAML or filter error fails the file. Checked against `yq` over the fleet's 1015 YAML files under two filters: identical verdicts. `ijq` still evaluates no filter; the jaq engine is now in the tree if it ever should. Done 2026-10-09 |
| Jinja2 templates (jinja2) | 0 | minijinja | `ijinja2` | done | `src/processor/generator/ijinja2.rs`: the jinja2 generator's contract (discovery, output paths, project-root loader, environment as context) rendered with minijinja configured like a plain Jinja2 `Environment` (no autoescape, trailing newline dropped, line endings normalised), minijinja-contrib's Python method compatibility, and Jinja2's argument lists for `truncate`/`replace`/`round`/`int`/`sum`/`center`/`forceescape` where minijinja's are shorter. Checked against Python Jinja2 over 34 templates and 59 single-construct probes: identical except keyword-only `wordwrap(width=)` and `truncate(leeway=)`, numeric-only `sum`, `2 ** -1`, dict keys named like Python methods, `attr` on dicts, `reverse` on a list and `urlencode` of a dict (`+` vs `%20`). Done 2026-10-09 |
| Protobuf compile (protobuf) | 0 | protox, prost-build, prost-types, prost | `iprotobuf` | done | `src/processor/generator/iprotobuf.rs`: a stanza's `.proto` files compiled together with protox (imports against `include_paths` or the `src_dirs`, well-known types built in), Rust generated with prost-build as one `<package>.rs` per package, the descriptor set optionally written. protox's descriptors compared with protoc 3.21's over proto2/proto3 files covering the syntax: byte-identical. Rust output only; a repo needing C++ stays on protobuf. Done 2026-10-09 |
| Shell lint (shellcheck) | 80 | imbl, unicode-general-category | `ishellcheck` | done | `src/engines/shellcheck/`: a module-by-module port of ShellCheck 0.11.0 (its Parsec parser on a small Parsec engine, the AST and its helpers, the control flow graph and dataflow analysis, every check of `Analytics`, `Commands` and `ShellSupport`, directives, `.shellcheckrc`, `source` following, the gcc output format). The dataflow states use persistent maps (imbl) as Haskell's `Data.Map` is persistent: with deep copies the largest script needed 5.4 GB. The tree-sitter-bash idea was dropped: a different grammar cannot give ShellCheck's parse errors or positions, and parity is what lets a repo switch without new findings. Verified against shellcheck over 4494 real scripts, ShellCheck's 2194 unit-test snippets and 28000 mutants, under each `shell` override, `external_sources` and `check_sourced`: identical findings. Faster than shellcheck (2.3 s against 15.7 s over 2781 scripts) and smaller (400 MB against 1.9 GB on the largest), after a 2026-10-10 pass (merges and patches that skip the subtrees two states share, bitset property sets, a parse error that no longer copies its message list). Unit tests run shellcheck 0.11.0 itself against the engine over `testdata/` and its mutants. Switching a repo is `shellcheck` → `ishellcheck`, `args` options → fields of the same name. The fleet switched on 2026-10-11 with release 0.9.123 (80 repos); four whose scripts source each other took `external_sources = true`, since shellcheck's batch run had followed those as inputs and ishellcheck checks each file alone |
| Java checks (checkstyle) | 15 | regex, unicode-general-category, xmlparser | `icheckstyle` | done | `src/engines/checkstyle/`: a port of checkstyle 8.36.1 for the fleet's shared `checkstyle.xml`: the ANTLR 2 Java lexer and recognizer generated from `java.g`, rule by rule (lookahead sets extracted by `scripts/gen-icheckstyle-tables.py`; the hidden-token filter and text-block lexer switching, ANTLR's tree building and `DetailAstImpl`'s pointer quirks, its exceptions as parse errors), `FileContents`' comment and Javadoc tracking, `TreeWalker`, the 15 modules of the shared config with all their properties, messages in checkstyle's order and dedup. The "no Rust Java linter" block was a crate search; porting the grammar removed it. Verified against checkstyle over 5169 files (the fleet's, checkstyle's own sources and test inputs) and 6000 mutants under three configurations: identical messages and identical exceptions for the ~4400 files checkstyle stops on. Other checkstyle modules are a configuration error naming them. Switching a repo is `checkstyle` → `icheckstyle`, dropping `args = ["-c", "checkstyle.xml"]`. The fleet switched on 2026-10-11 with release 0.9.123 (15 repos) |
| Perl analysis (perlcritic) | 7 | regex | `iperlcritic` | done | `src/engines/perlcritic/`: a port of Perl::Critic 1.156 on PPI 1.284: PPI's tokenizer and lexer method by method (the interleaving where the tokenizer consults the tree for `use feature` / `use v5.36` signatures, heredocs, the quote engines, POD and `__END__`/`__DATA__`, locations with `#line`), the element methods the policies call, Perl::Critic's document index, `## no critic` annotations, profile and policy selection, the violation formats, and the 22 severity-5 policies the fleet's shared `.perlcriticrc` enables, with all their parameters. The "no Rust Perl analyser" block was a crate search; porting PPI removed it. Verified against Perl::Critic over the fleet's 1313 Perl files, the 3950 modules of a Debian Perl installation, 207 edge cases and 12300 mutants: identical PPI trees (every token and location) and identical reports, PPI's parse errors included. A profile enabling a lower-severity policy is an error naming it. Switching a repo is `perlcritic` → `iperlcritic`; the shared `.perlcriticrc` stays. The fleet switched on 2026-10-11 with release 0.9.123 (6 repos; demos-lang-perl dropped its `libperl-critic-perl` dependency) |
| Kernel style (checkpatch) | 1 | none | `icheckpatch` | done | `src/engines/checkpatch/`: a port of `checkpatch.pl` at kernel commit `e5f92606156a` in `--no-tree -f` mode: the `diff -u /dev/null` input it reads, line sanitising, the statement and block walkers, the value annotator, `possible()` rebuilding the type patterns as the file teaches it type names, and every check file mode reaches, with the patterns verbatim. The "a Perl script, no Rust equivalent" block fell to a Perl-compatible regex engine of its own (recursion for `$balanced_parens`, possessive and atomic groups, lookbehind, Perl's capture semantics, the empty-match rules of `m//g`, `s///g` and `split`; Perl's super-linear cache, start class and required-substring checks for speed), with the patterns interpolated as Perl interpolates them (`qr//` stringification, `\Q`, `$;`). The tool recipe is pinned to the same commit so tests and port cannot drift. Verified against checkpatch.pl over the fleet's 5666 C files, 12005 files of the Linux 7.1.4 tree and 17117 mutants: identical output byte for byte, checkpatch's dies included (and its one exponential-backtracking hang, which the port shares); twice as fast on ordinary code. Switched c-kcpp (`checkpatch` → `icheckpatch`) on 2026-10-11 with release 0.9.123 |
| C/C++ lint (cpplint) | 1 | fancy-regex, unicode-width | `icpplint` | done | `src/engines/cpplint/`: a function-by-function port of cpplint 2.0.2 (line views, nesting stack, include/header-guard state, NOLINT, filters, the CPPLINT.cfg chain, every check, the regexes verbatim). Verified against cpplint over its own samples (22 files, 12 option sets), the 1245 snippets of its unit tests, demos-os-linux's 1231 files with and without `.cpplint`, and 3000 mutants: identical. Not ported: the dead "Unexpected \r" warning and the cross-file leak of CPPLINT.cfg settings. Switching demos-os-linux is `cpplint` → `icpplint`, `args = ["--config=.cpplint"]` → `config_file = ".cpplint"`; done on 2026-10-09 with release 0.9.120 (`cpplint` dropped from its dev group) |

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
| chromium, drawio, libreoffice | 0 | Drive those applications for rendering; the application is the capability | won't do |
| slidev, marp, mermaid | 0, 1, 1 | Node renderers with no Rust implementation of the format | blocked (2026-09-21) |
| pandoc | 5 | Multi-format conversion; `imarkdown2html` covers Markdown to HTML, nothing in Rust covers the rest | in progress (2026-10-11) as the separate MIT tool rspandoc: HTML (with `--standalone`, `--embed-resources`, `--css`, `--lua-filter`) matches pandoc byte for byte on teaching-syllabi, business-syllabi and data; DOCX, with rspandoc's own MIT styles (pandoc's reference document is GPL), reads back as the same document on all three. Still needed: PDF (all five repos; data and demos-ai-coding with xelatex, demos-lang-md PDF only), and demos-ai-coding's Markdown (ordered lists, fenced code with highlighting, setext headers) |
| a2x | 1 | AsciiDoc; no mature Rust AsciiDoc implementation | blocked (2026-09-21) |
| objdump | 0 | binutils; the `object` crate could dump sections but not disassemble faithfully | blocked (2026-09-21) |
| npm, gem | 0 | Package managers of other ecosystems | won't do |
| jekyll, sphinx | 0, 51 | Site and documentation generators for their own source formats. The capability "build documentation" is covered in Rust by mdbook; a sphinx or jekyll repo that wants the Rust path rewrites its sources, which is that repo's decision | covered by mdbook |
| explicit, generator, mass_generator (the `generic` processors), script | 25, 17, 2, 11 | Run whatever the user configures; the language is the user's | won't do (by design) |

## Stage 5: making the Rust path visible

Nothing here changes any repo's configuration. It makes the Rust option
easy to find and easy to choose.

| Item | Status | Notes |
|---|---|---|
| Docs page of every non-Rust processor names its Rust alternative(s) | done | A Rust processor declares what it replaces (`RustAlternative`, in its own file); the `rust_alternatives_are_consistent` test fails when the replaced processor's page does not link it |
| `rsconstruct processor list` shows the Rust alternative | done | A `Rust alternative` column; the JSON has a `rust_alternatives` array |
| `rsconstruct processor recommend` prefers the Rust option where one exists | done | The Rust processor is listed, the other moves to the `Also` column (`also` in the JSON) |
| Per-repo switching stays a per-repo decision | won't do | No rsmultigit sweep, no deprecation. A repo owner who wants the Rust path edits its `rsconstruct.toml`; the coverage script's usage column shows the uptake, nothing enforces it |

## How to work an item

1. Measure first: run the existing tool and the Rust candidate over every
   fleet repo that uses the existing one; diff the findings. Write the gaps
   into the row and into the new processor's docs page.
2. For a wrapper: `ToolInfo` in `src/tools.rs` (bare binary name, runtime,
   install methods), then the processor file, `is_rust: true`.
   For a builtin: the processor file over the crate, `is_builtin: true`,
   `is_rust: true`; the `builtin_processors_are_rust` test enforces the pair.
3. Docs page under `docs/src/processor/<type>/`, test file under
   `tests/processor/`; `cargo nextest run` must be green with the tool
   installed (tests never skip).
4. `rsconstruct processor defconfig <name>` shows every config field; if
   the new processor needs a config file of its own (selene's `selene.toml`,
   say), a fleet-shared default goes into rsmultigit's shared set so a repo
   that opts in gets a working one.
5. The existing processor's docs page gets its "Rust alternative" line. The
   existing processor is otherwise untouched: not deprecated, not removed,
   and no repo is switched.
6. Update this file: the row's status, and the log below with the new
   coverage number.

## Measuring

`scripts/rust_coverage.py` (fleet directory as its argument, `~/git` by
default) prints the coverage line and the non-Rust processors the fleet still
declares. Below is the first version, which produced the baseline; it reads
`rsconstruct --json processor list` and every `rsconstruct.toml` under
`~/git`.

```python
import glob, json, os, re, subprocess

entries = json.loads(subprocess.check_output(["rsconstruct", "--json", "processor", "list"]))
usage = {}
for path in glob.glob(os.path.expanduser("~/git/*/rsconstruct.toml")):
    # Headers are processor.<type>.<name>[.<instance>]; the pname is the first three segments.
    for m in re.finditer(r"^\[processor\.([a-z_]+\.[a-z0-9_]+)", open(path).read(), re.M):
        usage.setdefault("processor." + m.group(1), set()).add(path.split("/")[-2])
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
| 2026-10-04 | Done rows removed (is_rust flag; JSON, Markdown, TOML lint; templating; Sass; PDF merge; Markdown to HTML; oxlint; biome); `icss` set to won't do; script updated for `processor.<type>.<name>` names | 30 / 96 | 741 / 1587 (46 %) |
| 2026-10-11 | Script checked in; Rust alternatives in `processor list`, `recommend` and the docs; fleet on ishellcheck, icheckstyle, iperlcritic, icheckpatch; mypy, eslint, stylelint gone from the fleet | 46 / 112 | 1454 / 1665 (87 %) |
