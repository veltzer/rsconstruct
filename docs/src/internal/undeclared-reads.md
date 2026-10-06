# Undeclared Reads

**Status: researched (2026-10-06), not started — deferred.** This chapter
records what was found while considering a check for files a tool reads
without them being declared inputs. Pick it up from here.

## The problem

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

## Suggested next step

Before designing the feature, measure the noise: trace one build of a few
fleet repositories with `strace` by hand and count undeclared project-file
reads per processor. If most processors read nothing undeclared, the check
can fail by default with a small allow-list; if many do (config files,
caches), it starts as a report.

See also [Suggestions](suggestions.md) ("Sandboxed execution") and
[Strictness](strictness.md).
