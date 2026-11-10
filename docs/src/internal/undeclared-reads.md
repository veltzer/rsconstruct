# Undeclared Reads

**Status: report mode implemented (2026-11-10), opt-in.** `[build]
undeclared_reads = "report"` runs each product's tools under strace and
warns about the project files they read without declaring them. Failing
the build on a finding is not implemented; see "Next steps". The research
that led here follows the implementation notes.

## How the report works

- **Where.** Like the sandbox, the report hangs off the product running on
  the thread. The executor records a spec (`read_trace::set_spec`) next to
  the sandbox spec, holding the product's declared inputs and outputs, its
  output directories, and the places its tools keep their own state (the
  same `sandbox_writable` entries the sandbox uses). `run_command_inner`
  wraps the command with `read_trace::prepare` and reads the log in
  `Trace::finish`. Builtin processors are not watched: their work is
  rsconstruct's own code, reading exactly the inputs it is handed.
- **The trace.** `strace -f -qq -z -y --kill-on-exit -e
  trace=open,openat,openat2 -e signal=none -o LOG -- TOOL ARGS`. `-f`
  follows children (a script's interpreter, a compiler's subprocesses);
  `-z` keeps successful calls only; `-y` prints the absolute path each
  returned descriptor refers to, so a relative open resolves without
  tracking the caller's directory. strace goes outermost: under the
  sandbox it wraps bwrap and follows the tool inside it, and the paths it
  prints are the real ones, since the overlay is mounted on the project
  root. The logs live under `.rsconstruct/read-trace/` and are removed after
  each run.
- **What counts as a read.** An `open`, `openat` or `openat2` without
  `O_WRONLY`, `O_CREAT`, `O_TRUNC`, `O_DIRECTORY` or `O_PATH`. A file gone
  after the run was the tool's scratch and is dropped, and so is anything
  that is not a regular file.
- **What is reported.** Each such path inside the project that is not a
  declared input (the processor's own, and the ones analyzers add) or a
  symlink's target among them, not a declared output, not under a declared
  output directory, not under one of the tool's state places, and not under
  `.rsconstruct/`, `.venv/`, `node_modules/` or `.git/` (rsconstruct's state,
  the tools' installations, and git's metadata, which tools probe for the
  project root or a version). A batched run is compared with the union of
  its products' declarations. Only a successful run is reported.
- **Preflight.** A build with the report on and any non-builtin processor
  to run fails up front when `strace` is missing, the way a sandboxed build
  fails without `bwrap`; `tool install machine` installs it.

### Limitations

- **Tools that leave a daemon running.** strace waits for every process it
  traces, so a tool that starts a server and exits keeps the run going
  until the server does. `dotnet build` is the measured case: msbuild's
  node-reuse servers outlive the build, the traced run sat for fifteen
  minutes, and the CLR finally aborted ("Internal CLR error"). Turn the
  daemon off for the traced build (`dotnet build -nodeReuse:false`, or
  `MSBUILDDISABLENODEREUSE=1`).
- **Only products that run are watched.** A product restored from the
  cache or up to date runs no tool. Use the report with `build --force` or
  on a cold cache.
- **Reads, not stats.** A tool that only checks whether a file exists
  (`stat`, `access`) is not reported, nor is a directory listing.
- **Cost.** Every traced syscall stops the tool twice; I/O-heavy tools are
  noticeably slower. The report is a diagnostic, not a mode for every build.

## Measurements, 2026-11-10

Forced builds (`build --force -k`, the user's `execution_model =
"sandbox"`) of eight fleet repositories with the report on. Two runs are
not counted: embeddings and book-openbook's pyrefly products failed on the
measuring shell's `VIRTUAL_ENV` pointing at a personal venv, without the
report as well.

| Repository | Findings |
|---|---|
| pytconf, demos-hw-stm32, rstype | none |
| demos-lang-cs | none before the `dotnet build` daemon hang above |
| demos-lang-rust | the cargo stanza reads `rust-toolchain.toml` and its own command, `scripts/cargo_build.py` |
| book-openbook | four lilypond book stanzas read `images/logo.eps`; `pytest` reads three `.ly` templates |
| teaching-slides | the two check scripts read `text_labels.yaml` and `resources/diagram_themes.yaml`; biome reads `package.json`; one generic stanza reads 1694 slide sources it does not declare |

Every finding is real: editing any of those files rebuilds nothing. Two
patterns stand out:

1. **Config files** a script or tool reads by convention (`package.json`,
   a YAML of labels or themes, `rust-toolchain.toml`). The fix is
   `dep_inputs` in the repository's config, or the processor learning the
   file as a default input.
2. **A generic stanza's own command.** `processor.explicit.generic` does not
   make its `command` an input, so a stanza that does not list its script
   in `inputs` never rebuilds when the script changes. Several repositories
   list it; demos-lang-rust's cargo stanza does not.

## Next steps

1. Fix the findings above in their repositories, and decide whether a
   generic stanza's `command` should be an input by default.
2. Measure more of the fleet, then decide whether a finding should fail
   the product (`undeclared_reads = "fail"`), which needs an allow-list
   field for reads that are known and accepted.

## The problem (research, 2026-10-06)

rsconstruct does not declare dependencies; it discovers them. Processors
find their inputs by scanning, analyzers add what source files reference
(`#include`, `import`, `@use`, template includes), and `dep_inputs` /
`dep_auto` add the rest. When all of that misses a file a tool actually
reads, nothing notices: the file is not part of the product's cache key,
editing it rebuilds nothing, and the build stays green with a stale result.

A check would run each tool, see which files it opened, and report any
file inside the project that is not among the product's inputs.

## How other build systems handle it

Two families: **prevent** undeclared reads by running each action where
only its declared inputs exist, or **observe** what each action reads and
compare it with what it declared.

### Prevent — sandboxes

- **Bazel** runs every action in a sandbox containing only its declared
  inputs. On Linux, `linux-sandbox` builds a private mount namespace with
  each input bind-mounted or symlinked into a fresh execution root (network
  off by default); on macOS, `sandbox-exec` profiles; elsewhere, a weaker
  symlink-forest sandbox. An undeclared file is simply absent, so the tool
  fails with "not found" — Bazel never sees the read itself. Remote
  execution gives the same guarantee: only declared inputs are uploaded to
  the worker.
- **Buck2** gets hermeticity chiefly from remote execution, with the same
  only-declared-inputs property. Local execution is, as far as we know, not
  sandboxed by default, so a local Buck2 build can read an undeclared file
  unnoticed; the guarantee holds where builds run remotely, typically CI.
- **Pants** runs each process in a temporary directory into which only its
  declared inputs are materialized.
- **Please** isolates each process with Linux namespaces.
- **Nix** builds in a namespace sandbox holding only the declared store
  paths, without network.

### Observe — tracing

- **tup** watches every file a command opens (a FUSE filesystem on Linux,
  library interposition elsewhere) and fails the build when a command read
  a file it did not declare, or wrote one it did not declare. The closest
  match to what rsconstruct would want.
- **BuildXL** (Microsoft) intercepts every file access of every process —
  Detours on Windows; on Linux, `LD_PRELOAD` interposition backed by ptrace
  for static binaries — and uses the observations both to enforce
  declarations and for caching.
- **fabricate.py** and similar tools run commands under `strace` and
  *infer* dependencies from what was opened, rather than checking
  declarations.
- **Ninja / Make** take dependency lists from the compiler (`-MD`,
  `deps = gcc`) and trust them; nothing is enforced.

## Which family fits rsconstruct

**Observation.** Sandboxing works for Bazel because a `BUILD` file declares
every input of every action before it runs. rsconstruct runs tools in the
real working tree, at real paths, where they find config by walking up
directories, and many read files nobody would think to declare — Sphinx's
`conf.py` imports, `book.toml`, `Cargo.lock`, `node_modules`, a whole
`.venv`. A sandbox would break most processors until every such file was
declared, inverting the design (discovery over declaration).

Tracing leaves tools running exactly as now and adds a report on the side,
so it can be opt-in and used in CI, as tup uses it. It also catches the
read even when the undeclared file exists; a sandbox only fails when the
file is missing from it.

## Mechanisms considered

| Mechanism | Sees | Cost | Notes |
|---|---|---|---|
| **`strace -f -e trace=openat,open,execve`** around each tool run | every syscall, including static binaries and Go programs | traced runs are much slower; needs the `strace` tool | Linux only — on macOS the mode would fail with a clear "needs strace" error, consistent with the platform rules (no `#[cfg]`). Leading candidate. |
| `LD_PRELOAD` shim intercepting `open`/`openat` | dynamically linked programs only | fast | Misses static binaries, Go, direct syscalls: it would under-report silently, against strict-by-default. Also a `.so` to build and ship. |
| fanotify, in-process | all opens under the project | no external tool | Attributing an open to the product that caused it needs the process tree; unprivileged fanotify is limited on older kernels; Linux-only code to keep in `platform.rs`. |
| ptrace / seccomp user-notify, in-process | every syscall | moderate | Precise, but a substantial amount of Linux-specific code. |

## Open questions

1. **Mechanism.** `strace` is the leading candidate (complete, simple to
   attach — tool runs already go through central helpers in
   `src/processor/mod.rs`), at the price of slow traced runs and a tool
   dependency for this mode.
2. **What a finding does.** Fail the product with a message naming the
   file (consistent with how a missing tool is handled), or warn. Failing
   needs an escape hatch: a per-processor field listing reads that are
   known and accepted (e.g. a tool's own config file outside the project).
3. **What counts.** Only reads inside the project tree, excluding
   `.rsconstruct/` and the tool's own installation; reads of a product's
   own outputs and of declared output directories are expected.
4. **Batch runs.** A batched invocation reads files for many products at
   once; a read is undeclared only if no product of the batch declares it.

See also [Sandboxed Execution](sandbox.md) (the sandbox, on by default, that
covers undeclared writes and network access, leaving reads to this
check), [Suggestions](suggestions.md) ("Sandboxed execution") and
[Strictness](strictness.md).
