# Feature design: structured errors with `errors show` / `errors edit`

## Status

Draft — awaiting review. This is a multi-week feature, not a single-PR change.

## Origin

`problems.txt`:

> error handling. This is a big one. I want every processor to know how
> to parse the output of the tools it is running (if they are external)
> and collect errors in a organized fashion: file, line number,
> description of the error, stack etc. then we can do something like:
> `rsconstruct errors edit` and it will launch an editor of my choice
> on all the previous errors. Internal processors will surely be able
> to collect all the errors into that predefined structure with no
> problems. `rsconstruct errors show` will show the latest errors.

## What's there today

- Errors are flat strings. `check_command_output` (`src/processors/mod.rs:261`)
  builds a multi-line string from `stderr` + `stdout` + a context label
  and bubbles it up via `anyhow::bail!`.
- The executor collects them into a `Vec<String>` (`shared.failed_messages`,
  `src/executor/mod.rs:81`).
- The JSON event stream emits per-product `ProductComplete { error:
  Option<String>, ... }` — also a flat string.
- There is no persistence: errors live for the lifetime of one build run
  and are gone when the process exits.
- There is no `rsconstruct errors` subcommand.

So the proposal is genuinely new functionality: a structured error
**type**, a per-processor **parser** that produces it, **persistence** of
the error set, and two new CLI commands (`show`, `edit`).

## Design

### Layer 1 — the core type

Add a new module `src/diagnostic.rs`:

```rust
pub struct Diagnostic {
    pub file: PathBuf,            // relative to project root
    pub line: Option<u32>,        // 1-indexed; None for whole-file errors
    pub column: Option<u32>,      // 1-indexed
    pub end_line: Option<u32>,    // for multi-line spans (rust-style)
    pub end_column: Option<u32>,
    pub severity: Severity,       // Error | Warning | Note
    pub message: String,          // one-line summary
    pub detail: Option<String>,   // multi-line detail (compiler explanations)
    pub code: Option<String>,     // tool error code, e.g. "E0308" or "no-undef"
    pub processor: String,        // iname of the processor that produced this
    pub tool: Option<String>,     // external tool name when applicable
}

pub enum Severity { Error, Warning, Note }
```

Design choices:

- **`PathBuf` not `String`** for `file`. Forces every parser to produce a
  real path, not a synthetic label.
- **`Option<u32>` for line/column.** Many processors fail at file granularity
  (license_header, terms with no specific line). Forcing line/column would
  produce fake `1:1` positions everywhere.
- **`code: Option<String>`** lets `rsconstruct errors filter --code=E0308`
  later — keeps the door open without forcing every processor to invent codes.
- **`processor` is required.** Always know which processor / iname produced
  the diagnostic; useful for `errors show --processor=ruff`.

### Layer 2 — how processors emit them

Today, `Processor::execute(&self, ctx, product) -> Result<()>`. The error
is the `Err` variant.

Two choices:

**(a)** Replace `Result<()>` return with `Result<Vec<Diagnostic>>` — a
processor explicitly returns the diagnostics it found, even on success
(warnings).

**(b)** Keep `Result<()>` but add a new optional method
`fn diagnostics(&self) -> Vec<Diagnostic>` that the executor calls when
`execute` returns `Err`.

I'd go **(a)** — it forces every processor to think about diagnostics, and
warnings become first-class instead of being lost in tool stdout. But it's
a breaking change to ~70 processor implementations.

Realistic compromise: keep `Result<()>` for source-compatibility, add a
new optional return channel via a thread-local diagnostic sink that
processors push to, and have the executor drain it after each `execute`
call. Less invasive, less principled. Probably the right ship-now choice.

### Layer 3 — per-tool parsers

Each external tool has its own output format:

| Tool        | Format                                                    |
| ----------- | --------------------------------------------------------- |
| rustc       | `--error-format=json` produces structured JSON per line   |
| clippy      | same as rustc                                             |
| ruff        | `--output-format=json` — JSON array                       |
| pylint      | `--output-format=json` — JSON list                        |
| mypy        | `error: <file>:<line>: ...` — text grep                   |
| eslint      | `--format=json` — JSON                                    |
| shellcheck  | `--format=json1` — newline-delimited JSON                 |
| gcc / clang | `<file>:<line>:<col>: error: <msg>` — text grep           |
| make        | `<makefile>:<line>: ...` — text grep                      |
| markdownlint| `--output=...` — text or JSON depending on flag           |
| terms       | internal; we already know the file and lines              |
| aspell      | internal scan; we already know the file and lines         |

Each processor that wraps a tool needs a small **parser** that ingests the
captured stdout/stderr + the tool's exit code and produces
`Vec<Diagnostic>`.

Two patterns:

- **Tools with a JSON option**: pass the JSON flag, parse the JSON, map to
  Diagnostic. Cheap and accurate. This is the preferred path — most modern
  tools support it.
- **Tools without a JSON option**: a regex per known format. Handful of
  these; each is ~20 lines of code.

Internal processors (terms, aspell, license_header, the analyzers) don't
need parsers — they construct Diagnostic values directly when they detect
violations.

### Layer 4 — persistence

For `rsconstruct errors show` to work after the build process has exited,
diagnostics must persist on disk.

Proposal: write to `.rsconstruct/diagnostics.jsonl` at the end of each
build run (overwriting the previous file). Newline-delimited JSON, one
Diagnostic per line. Easy to grep, easy to parse, easy to see in `git
diff` (though it's gitignored — see below).

Add `.rsconstruct/diagnostics.jsonl` to the `/.rsconstruct/` directory
that the project already gitignores.

Don't write incrementally during the build. Write once at the end. If the
process is killed, the previous file remains — slightly stale but fine.

Schema versioning: include a `version: 1` line at the top, separate from
the diagnostics. When the schema changes, `errors show` reading an old
file emits a friendly "file written by older rsconstruct, run a build"
and exits 0. No automatic migration; a build always rewrites it.

### Layer 5 — the CLI

```
rsconstruct errors show [--processor=NAME] [--severity=SEV] [--limit=N]
rsconstruct errors edit [--processor=NAME] [--severity=SEV] [--limit=N]
rsconstruct errors clear
rsconstruct errors list-processors    # which processors emitted diagnostics
rsconstruct errors stats              # count by processor, severity
```

`errors show` prints them in a stable format:

```
src/foo.rs:42:5: error[E0308]: mismatched types
    expected `&str`, found `String`
  [from clippy]

tests/bar.py:17: warning: unused import 'os'
  [from ruff]
```

`errors edit` opens the user's `$EDITOR` with all error locations as
arguments. For editors that support a quickfix-style list (vim, neovim,
helix, emacs), encode the locations as `file:line:column` so vim's
`+arglist` works.

The exact invocation pattern depends on `$EDITOR`. Initial support:

| `$EDITOR`           | Invocation                                        |
| ------------------- | ------------------------------------------------- |
| `vim` / `nvim`      | `$EDITOR -q <quickfix-file>`                      |
| `emacs` / `emacsclient` | `$EDITOR --eval '(grep "<args>")'`            |
| anything else       | `$EDITOR <file1> <file2> ...` (no jump)           |

Build the quickfix file from the diagnostic list. This is straightforward.

### Layer 6 — what about builds that already failed before producing diagnostics?

Some failures don't have a "file:line" location:
- `command not found`
- `permission denied`
- `Failed to create output directory`

These are framework errors, not user code errors. Today they're already
in `failed_messages`. Proposal: treat them as Diagnostics with `file =
"<framework>"` and `line = None`. They appear in `errors show` but with
a different leading sigil so they're visually distinct.

Alternative: keep them out of the diagnostic stream entirely, since the
user can't "fix" them in an editor. They show up in the existing
`Exited with X` summary.

Recommend: **keep them out**. The diagnostic stream is for user-fixable
file-level issues. Framework errors stay in the existing channel.

## Implementation phases

This is too big for one PR. Suggested phasing:

**Phase 1 — type and persistence.** Land `Diagnostic`, the diagnostic
sink, and write it to disk at end of build. No parsers yet; the sink
stays empty. `errors show` lists nothing useful but the plumbing is
real.

**Phase 2 — internal processors.** Wire up terms, aspell, license_header,
and the analyzers to push to the sink. These don't need parsers. After
this phase, `errors show` lists *internal* processor errors. Useful by
itself.

**Phase 3 — JSON-output tools.** ruff, pylint, eslint, mypy, clippy,
shellcheck, markdownlint. One parser per processor, 20–50 lines each.
Wire them up incrementally.

**Phase 4 — text-output tools.** gcc/clang, make, the long tail. Each
needs a regex parser and a fallback for "couldn't parse".

**Phase 5 — `errors edit`.** Editor integration. Best left until at
least phase 3 is done so there's actually content to navigate.

After each phase, the feature is a real shippable improvement. The user
can stop me at any phase.

## Open questions

1. **Granularity of `Result<()>` migration**: the breaking-change vs
   thread-local trade-off in Layer 2. I lean toward the thread-local
   sink — it's pragmatic and additive. But if you'd rather take the
   refactor cost now, say so.

2. **What does `errors show` do when there are zero diagnostics from
   the last build, but the build *succeeded*?** Print "No errors from
   last build" and exit 0? Or exit 1 to signal "nothing to show"?
   I'd do the former — exit 0 is the natural meaning of "I did the
   thing, there was nothing to do."

3. **Diagnostic stream and watch mode**: in `rsconstruct watch`, the
   diagnostic file is rewritten on every rebuild. That's correct.
   Should `errors show --watch` exist (live-tail the file)? Out of scope
   for phase 1, but worth knowing if you want it eventually so the
   on-disk format stays compatible.

4. **What about non-build errors** — `rsconstruct test` failures, for
   example? Should test failures (when we eventually have a test
   processor) push to the same sink? Probably yes; it's the same
   abstraction.

5. **Editor selection**: `$EDITOR` is the conventional choice. Worth
   adding a `[errors] editor = "..."` config field? I'd skip that until
   someone asks.

6. **Shipping order**: any phase you'd skip or reorder? E.g. phase 5
   (`errors edit`) is the demo-worthy bit but useless without phases
   2–4 producing real content.

## Why this is hard, soberly

- ~70 processors to retrofit. Each is small (10–50 lines) but the
  overall surface is big.
- Tool output formats drift. JSON fields get added, text format changes.
  We will own a small parser library forever.
- The editor-integration UX is opinionated; vim/emacs users have
  different expectations and the "right" command varies.
- Diagnostics are only useful if the user's editor jumps to them, and
  every editor invocation has corner cases (paths with spaces, errors at
  position 1:1 of a binary file, etc.).

The phased plan above lets this ship in increments, each useful.

## Recommendation

Start with **Phase 1 + Phase 2**: the type, the sink, the disk
persistence, the `errors show` command, and wiring up the *internal*
processors (no external tool parsers yet). That gets you a working
`rsconstruct errors show` for the things rsconstruct already
understands — terms, aspell, license_header, analyzer diagnostics. It's
~500 lines of code and a useful feature on its own.

If you like it, do Phase 3 next (the easy external tools with JSON output)
— another ~500 lines. Phases 4 and 5 are optional later work.

Tell me which scope you want, or if you'd rather restructure the phases.
