# Sandboxed Execution

**Status: designed (2026-11-07), not started.** This chapter records the
design for running each external tool in its own sandbox. It builds on
[Undeclared Reads](undeclared-reads.md), which covers the read side.

## Goal: correctness, not security

The sandbox exists to make builds **correct**: a product's result must
depend only on what rsconstruct knows about it, and a tool must write only
what its product declares. It catches build mistakes, not attackers.

**Security is explicitly not a goal.** The sandbox does not defend against
a malicious or untrusted tool. It does not hide `$HOME`, secrets, SSH keys
or the rest of the filesystem, it filters no syscalls, and it makes no
promise that a determined tool cannot escape. Every tool rsconstruct runs
is one the project chose to install and trust. Treating the sandbox as a
security boundary would demand a minimal filesystem view, seccomp filters
and much else that breaks tools, for a threat this build tool does not
model. A design change that adds restrictions *for security's sake* is out
of scope; one that adds restrictions to catch a *correctness* bug belongs
here.

The mistakes it is meant to catch:

- **Undeclared writes.** A tool writes a file its product does not declare
  as an output: a stray temp file, a cache directory in the tree, an output
  written to the wrong path. Today such a file is invisible to the cache,
  `clean` never removes it, and a downstream product may read it without
  any dependency edge.
- **Hidden network access.** A tool that fetches something at build time
  produces a result that depends on the network, which no cache key
  covers.
- **Partial outputs.** A tool that fails halfway leaves half-written
  outputs in the tree.

Undeclared *reads* are left to tracing (see below and
[Undeclared Reads](undeclared-reads.md)).

## Sandboxing is an option, not a requirement

Sandboxing is **opt-in** and must stay optional:

- `[build] sandbox = false` is the default. A build without the setting
  runs exactly as it does today.
- A project turns it on for the whole build with `[build] sandbox = true`,
  and any processor can override the build-wide value with its own
  `sandbox` field.
- A machine without `bwrap` (macOS, a minimal container) must still be able
  to build every project that leaves the sandbox off. Only a build that asks
  for the sandbox needs `bwrap`, and that build fails with a clear "needs
  bwrap" error instead of silently running unsandboxed (strict by default).
- The sandbox never changes what a successful product produces, so it is
  not part of any cache key (`affects_output = false`). A product cached by
  a sandboxed build is valid for an unsandboxed one and the other way
  round.

The intended use is as a **check**: turn it on in CI, or locally while
adding a processor, to catch the mistakes above. Even if it later becomes
the recommended setting for the fleet, it remains a setting, not a
precondition for using rsconstruct.

## Why not a Bazel-style sandbox

Bazel's sandbox exposes only an action's declared inputs, so an undeclared
read fails with "file not found". That works because a `BUILD` file
declares every input of every action. rsconstruct *discovers* inputs
instead (scan settings, analyzers, `dep_inputs`), and its tools find their
configuration by walking up directories and read files nobody would think
to declare: Sphinx's `conf.py` imports, `Cargo.lock`, `node_modules`, a
whole `.venv`. Hiding everything undeclared would break most processors
until every such file was listed, reversing the design. Undeclared reads
are better caught by observing them (strace), which keeps tools running
unchanged.

## Design

### What the sandbox looks like

Each external tool run (one process tree, one product or one batch) gets:

- **The whole filesystem, read-only.** Tools see every path they see
  today, at the same location, so config discovery, the `.venv`,
  `node_modules` and system tools all keep working.
- **An overlay over the project root.** An overlayfs is mounted on the
  project directory; every write the tool makes lands in a private *upper*
  directory, never in the real tree.
- **A private `/tmp`.**
- **No network** (`--unshare-net`), unless the processor sets
  `sandbox_network = true`. Processors that genuinely download (pip, npm,
  cargo, gem) set it in their plugin defaults.

### After the tool exits

rsconstruct inspects the upper directory:

1. Files under the product's declared outputs and output directories are
   moved into the real tree.
2. Any other file in the upper directory fails the product, with a message
   naming the processor, the product and each undeclared path.
3. If the tool failed, nothing is moved: a failed product leaves no
   partial outputs behind.

The upper directory lives on the same filesystem as the project (under
`.rsconstruct/`), so moving an output out of it is a `rename`, not a copy.

For a batched invocation the declared set is the union of the batch's
products' outputs.

### Mechanism: bubblewrap

The sandbox is built with `bwrap` (bubblewrap) as an external tool:

```text
bwrap --ro-bind / / --dev /dev --proc /proc --tmpfs /tmp --unshare-net \
      --overlay-src <project> --overlay <upper> <work> <project> \
      --chdir <cwd> -- <tool> <args...>
```

- It needs no root: it uses unprivileged user namespaces. Ubuntu restricts
  those (`kernel.apparmor_restrict_unprivileged_userns = 1`) but ships an
  AppArmor profile (`bwrap-userns-restrict`) that allows bwrap.
- It is the mechanism Flatpak relies on, so its behaviour across kernels is
  well exercised.
- It keeps namespace and mount code (unsafe, Linux-specific, easy to get
  wrong) out of `src/platform.rs`.
- It fits the tool model: `bwrap` gets a `ToolInfo` entry (apt
  `bubblewrap`) and is required only when the sandbox is on.

Verified by hand on 2026-11-07 with bubblewrap 0.11.1: a `touch stray_file`
inside the sandbox appeared in the upper directory and not in the
project tree, and network was unavailable. Overlay support needs
bubblewrap 0.9 or newer.

**macOS** has no bwrap. A build that enables the sandbox there fails with
a clear error; a build that does not is unaffected. `sandbox-exec` could
be added later as a second backend, chosen at run time (no `#[cfg]`).

### Where it hooks in

- **One spawn point.** `run_command_inner` in `src/processor/mod.rs` is
  where the command is wrapped. About 60 `Command::new` sites under
  `src/processor/` still spawn directly (cc, zola, sphinx, jekyll, ...);
  they must all move onto the shared helpers first, with a test that
  rejects new direct spawns, or the sandbox has holes.
- **Per-product context.** The runner needs the product's declared outputs
  and output directories. They reach it through a thread-local set per
  product execution in `src/executor/execution.rs`, the same way
  `DECLARED_TOOLS` does today.
- **Config.** `[build] sandbox` (bool, default `false`) in `BuildConfig`;
  per-processor `sandbox` and `sandbox_network` fields, each with a
  `FieldSpec` entry (`affects_output = false`).

### Not covered

- **Builtin processors** (`iyamllint`, `ishellcheck`, ...) run inside
  rsconstruct's own process; there is no child to wrap. Their file access
  is rsconstruct code reading its declared inputs.
- **Undeclared reads.** Everything is readable by design; see
  [Undeclared Reads](undeclared-reads.md) for the strace-based check that
  complements this sandbox.
- **Tool installation** (`tool install`) and rsconstruct's own helper runs
  (`doctor`, version probes) are not products and are not sandboxed.

## Open questions

1. **Writes outside the project.** Tools write caches under `~/.cache`
   (ruff, mypy, pip, uv). The read-only root makes those writes fail.
   Either bind the known cache directories writable, or point tools at a
   sandbox-private directory through their environment variables. This must
   be settled before measuring, or every Python checker reports a
   violation.
2. **Expected in-tree writes.** Some tools legitimately write files that
   are not outputs (`__pycache__`, `.mypy_cache`, `.pytest_cache`). These
   need a per-processor allow-list of paths that are discarded rather than
   reported, mirroring the allow-list question in
   [Undeclared Reads](undeclared-reads.md).
3. **GitHub runners.** `ubuntu-24.04` runners need `bubblewrap` installed
   (`scripts/ci-install-tools.sh`) and must allow it to create user
   namespaces. Verify with a throwaway job before relying on it. If it does
   not work there, the fix is in the environment setup, never in `ci.yml`.
4. **Cost.** bwrap start-up plus an overlay mount per invocation is a few
   milliseconds; batching amortizes it for checkers. Measure on a fleet
   build.

## Rollout

1. Move every processor spawn onto the shared runner helpers.
2. Implement the opt-in write and network sandbox.
3. Run cold fleet builds with `sandbox = true` and count violations per
   processor; settle the cache-directory and allow-list questions.
4. Fix the violations. The sandbox stays opt-in; whether the fleet's shared
   config turns it on is a separate decision.
5. Separately, add the strace-based undeclared-reads report.
