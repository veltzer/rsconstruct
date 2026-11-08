# Sandboxed Execution

**Status: first version implemented (2026-11-07).** `[build]
execution_model = "sandbox"` works on Linux and catches undeclared writes and
network access; see [Measured on rsconstruct itself](#measured-on-rsconstruct-itself)
and [Open questions](#open-questions) for what is left. It builds on
[Undeclared Reads](undeclared-reads.md), which covers the read side. The
code is `src/sandbox.rs`; user-facing docs are the `execution_model`,
`sandbox_network` and `sandbox_writable` entries in
[Configuration](../configuration.md).

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
  written to the wrong path. Natively such a file is invisible to the cache,
  `clean` never removes it, and a downstream product may read it without
  any dependency edge.
- **Undeclared removals.** A tool deletes something in the tree that is not
  its own output.
- **Hidden network access.** A tool that fetches something at build time
  produces a result that depends on the network, which no cache key
  covers.
- **Partial outputs.** A tool that fails halfway leaves half-written
  outputs in the tree.

Undeclared *reads* are left to tracing (see
[Undeclared Reads](undeclared-reads.md)).

## Sandboxing is an option, not a requirement

Sandboxing is **opt-in** and must stay optional:

- `[build] execution_model = "native"` is the default. A build without the
  setting runs exactly as it always has.
- A project turns the sandbox on for the whole build with `[build]
  execution_model = "sandbox"`, and any processor can override the
  build-wide value with its own `execution_model` field.
- A machine without `bwrap` (macOS, a minimal container) can still build
  every project that leaves the sandbox off. Only a build that asks for the
  sandbox needs `bwrap`; that build fails in the tool preflight with
  "Missing required tools: bwrap" and an install hint, instead of silently
  running unsandboxed (strict by default).
- The sandbox never changes what a successful product produces, so it is
  not part of any cache key. A product cached by a sandboxed build is valid
  for an unsandboxed one and the other way round.

The intended use is as a **check**: turn it on in CI, or locally while
adding a processor. Even if it later becomes the recommended setting for the
fleet, it remains a setting, not a precondition for using rsconstruct.

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

### What a sandboxed tool sees

Each external tool run (one invocation: one product, or one batch) runs as

```text
bwrap --die-with-parent --ro-bind / / --bind $HOME $HOME --bind /tmp /tmp \
      --dev-bind /dev /dev --proc /proc [--unshare-net] \
      --overlay-src <root> --overlay <upper> <work> <root> \
      [--bind <root>/<writable> <root>/<writable> ...] \
      --chdir <cwd> -- <tool> <args...>
```

- **The whole filesystem, read-only**, at the same paths, so config
  discovery, the `.venv`, `node_modules` and system tools all keep working.
- **`$HOME` and `/tmp` writable, as natively.** Tools write their caches
  under `~/.cache`, `~/.cargo`, `~/.npm`; Chromium needs `/dev/shm`;
  libreoffice serializes through a lock under `/tmp`. Writes outside the
  project are not what this sandbox checks, and keeping them working is
  what lets tools run unchanged. (A throwaway overlay on `$HOME` was tried
  and is rejected by the kernel with `EINVAL` when the upper layer is a
  tmpfs inside bwrap's namespace; it would also have thrown away every
  tool cache after each run.)
- **An overlay over the project root.** Every write the tool makes inside
  the project lands in a private upper directory under
  `.rsconstruct/sandbox/<pid>-<n>/`, never in the real tree. The upper
  directory is inside the project so that moving an output out of it is a
  `rename`, not a copy; the kernel accepts an upper directory below the
  lower one.
- **No network** (`--unshare-net`: a fresh network namespace with only
  loopback), unless a tool the processor runs declares that it needs it
  (`ToolInfo::needs_network`: `cargo-deny` for its advisory database,
  `taplo` for its schemas, and the installers `npm`, `pip`, `uv`,
  `bundle`), or the processor sets `sandbox_network = true` (for a
  script that downloads, whose tool rsconstruct does not know). cargo
  itself is `false`: it builds from its registry cache in `$HOME`, and a
  run that has to download a crate first is the finding the sandbox is
  for. `cargo deny` declares `cargo-deny` wherever it is installed: the
  project's tool directory, which is how the fleet installs it, or a
  global install that cargo finds.
- **Tool state allowed.** Every tool declares where it writes its own
  state in the project (`ToolInfo::sandbox_writable` in `src/tools.rs` or
  in the processor file that owns the tool): `target` for cargo,
  `.ruff_cache`, `.pytest_cache`, `.mypy_cache`, `.rumdl_cache`,
  `.jekyll-cache`, `.cpcache` for clojure, `.git` for git (read-only git
  commands still refresh the index), `.venv` for uv and pip,
  `node_modules` for npm, and `**/__pycache__` for every tool that runs a
  Python interpreter, which writes bytecode beside each module it
  imports, the project's `.venv` included. A sandboxed processor may
  write the places of every tool in its `required_tools`, plus its own
  `sandbox_writable` config (for a script, whose tool is unknown). An
  entry is a root-relative path or `**/NAME`. A path that already exists
  is bind-mounted read-write on top of the overlay, so a large incremental
  state such as `target` is written in place, without copy-up. Anything
  else in an allowed place is captured by the overlay and moved into the
  tree after a successful run, like an output. Nothing is created in
  advance: an empty `.git` would turn a directory into a broken
  repository.

### After the tool exits

rsconstruct reads the upper directory. overlayfs records three kinds of
change there:

| In the upper directory | Meaning |
|---|---|
| a file or symlink | written (created or modified) |
| a character device 0:0 (a *whiteout*) | the path was removed |
| a directory with xattr `user.overlay.opaque = y` over an existing directory | the directory was removed and created anew |

A plain directory in the upper layer is not a change: overlayfs copies a
directory up whenever something inside it changes, and an empty new
directory is left alone. An opaque directory with nothing in the real tree
beneath it is just a new directory. Nor is a file identical to the real
one, with the same bytes and mode (`sandbox::same_as_real`): overlayfs
copies a file up as soon as a tool opens it for writing, even when it
writes nothing, as Python's `shelve` does to read its database.

Before the tool starts, nothing in the real tree is touched. A native run
first unlinks the product's stale outputs (a restored output is a
read-only hardlink the tool could not overwrite; see
[Stale outputs before a rebuild](cache.md#stale-outputs-before-a-rebuild))
and creates its output directories. A sandboxed run does both inside the
overlay instead: the command bwrap runs is a short `sh` prelude that
removes those files and creates those directories, then `exec`s the tool
(`sandbox::inner_command`). The removals become whiteouts like any other,
are always allowed, and reach the real tree only with the rest of an
accepted run. The first applied run of a product clears the list, so a
second command of the same product does not remove what the first wrote;
a failed attempt is discarded and the retry removes them again. A
sandboxed builtin processor (below) gets the same prelude in front of its
child.

Then:

1. If the tool failed (non-zero exit, spawn error, timeout, interrupt),
   everything is discarded: a failed product leaves no partial outputs.
2. If any change is outside the product's declared outputs, output
   directories and its tools' state places, the product fails with a
   message naming the processor and each path (`wrote    stray.log`), and
   everything is discarded. A removed or replaced directory is judged by
   the files the real tree held beneath it, each checked on its own: a
   tool that runs `rm -rf docx` and writes `docx/` anew passes when every
   file `docx` held is a declared output, and each one that is not is
   reported as `removed`. A mass generator's staging and doctrees
   directories under `.rsconstruct/mass_generator/` count as output
   directories of the running product (`fresh_state_dir` declares them
   through `sandbox::declare_output_dir`); `install_staged` then checks
   their contents against the plan.
   In both cases the real tree is exactly as it was before the run,
   previous outputs included.
3. Otherwise removals are applied first (so a replaced directory is
   emptied), then written files are renamed into the real tree.
4. The run directory is removed. overlayfs leaves `work/work` with mode
   000, so its mode is opened up first.

A build killed mid-run (SIGKILL, the OOM killer, power loss) never gets to
step 4. Every build therefore starts by removing whatever is under
`.rsconstruct/sandbox/`, and the directory itself, whatever its execution
model, and prints how many runs it removed
(`sandbox::remove_leftover_runs`). Nothing there can be in use: the build
holds the project's cache database open, which redb locks exclusively, so
no other rsconstruct process is running in the project, and this one has
not started a run yet. Nothing there is worth keeping either: a run
reaches the tree only once it is accepted.

For a batched invocation the declared set is the union of the chunk's
products' outputs.

### Mechanism: bubblewrap

- It needs no root: it uses unprivileged user namespaces. Ubuntu restricts
  those (`kernel.apparmor_restrict_unprivileged_userns = 1`) but ships an
  AppArmor profile (`bwrap-userns-restrict`) that allows bwrap.
- It is the mechanism Flatpak relies on, so its behaviour across kernels is
  well exercised.
- It keeps namespace and mount code (unsafe, Linux-specific, easy to get
  wrong) out of `src/platform.rs`. The only platform helpers the sandbox
  needs are whiteout detection and an xattr read (through the `xattr`
  crate, which builds on macOS too).
- It fits the tool model: `bwrap` has a central `ToolInfo` entry (apt
  `bubblewrap`), because rsconstruct itself runs it, not any one processor.
- Overlay support needs bubblewrap 0.9 or newer. Developed against 0.11.1.

**macOS** has no bwrap. A build that enables the sandbox there fails the
preflight; a build that does not is unaffected. `sandbox-exec` could be
added later as a second backend, chosen at run time (no `#[cfg]`).

### Where it hooks in

- **Config.** `[build] execution_model` (`ExecutionModel` in
  `src/config/settings.rs`) is copied onto the `BuildContext` with the other
  build-wide settings. The per-processor `execution_model`,
  `sandbox_network` and `sandbox_writable` are universal `StandardConfig`
  fields, listed in `STANDARD_EXTRA_FIELDS`. The tools' own places come
  from `ToolInfo::sandbox_writable` (`sandbox::writable_for`). None is part
  of the cache key. A misspelled `execution_model` or a malformed
  `sandbox_writable` entry is a config error.
- **Which product is running.** The executor sets a thread-local
  `sandbox::Spec` (declared outputs, output directories, network,
  writable directories) around `execute` and `execute_batch`, next to the
  declared-tools record, and clears it afterwards. No processor runs tools
  from other threads, so the thread-local is reliable.
- **One spawn point.** `processor::run_command_inner` asks
  `sandbox::prepare` to wrap the command and calls `Run::finish` once the
  tool exited. Every product tool run goes through it. The four direct
  `.output()` calls under `src/processor/` (the sphinx, jekyll, generic and
  zola planners) run during discovery, outside any product, as do version
  probes, `doctor` and `tool install`; they run natively by design.
- **Preflight.** `check_required_tools` requires `bwrap` when any processor
  with external tools resolves to the sandbox.
- **Processors that cannot be sandboxed.** Every plugin entry carries
  `sandboxable` (`ProcessorPlugin` in `src/registries/processor.rs`),
  shown by `processor list` as `Sandbox`. marp, mermaid, chromium and
  drawio are `false` (open question 1). Before anything else, `build` calls
  `sandbox::check_sandboxable` over the configured instances: an enabled
  instance of a non-sandboxable processor that resolves to the sandbox is a
  config error naming every such instance, so the build fails in
  milliseconds instead of on each product. It reads the raw instance
  tables, so it judges every configured instance, including one that
  matches no file in this repo.

### Builtin processors

A builtin processor (`tera`, `iyamllint`, `ishellcheck`, ...) does its work
in rsconstruct's own code, so there is no tool to wrap. Natively it runs in
the build's process, as always. When it resolves to the sandbox (the
build-wide or its own `execution_model = "sandbox"`, and only then), the
executor runs rsconstruct itself instead, through the same sandbox as any
tool (`sandbox::execute_builtin`):

1. The command is the running executable with the hidden subcommand
   `sandbox-run` and the build's output flags (`--color`, `--json`,
   `--quiet`, `--show-output`, `--show-child-processes`). Its standard
   input is a JSON job: the instance's resolved config (overrides
   included), the products (inputs, outputs, output directories,
   variant), whether to run them as one batch, and what the build sets
   up around a processor (how it walks the tree, `[build] python`,
   `max_arg_len`, `command_timeout_secs`).
2. The child recreates the processor from that config and runs its
   discovery on a scratch graph, because a processor may keep state from
   discovery for execute (tera keeps the templates `{% include %}` can
   name). It then executes the products, with `execute` or, for a batch,
   `execute_batch`, exactly as the executor would natively.
3. It prints whatever the processor printed, then a marker line and one
   result per product (`null`, or the error). The parent passes the
   printed text on and maps the results back to the products. A child
   that fails outright (a config the processor rejects) fails every
   product of the job.
4. Being an ordinary sandboxed command, its writes land in the overlay and
   are judged like a tool's: declared outputs are applied, anything else
   fails the products and is discarded. Commands the processor runs (tera
   runs `python`, `sh`, `git`) execute directly inside that sandbox.

The child cannot use the build's caches, which the parent holds open: the
mtime cache is off, and the webcache (`.rsconstruct/webcache.redb`) is not
read or written, because a write to it would be undeclared and children
running side by side cannot share its lock, so `iyamlschema` downloads its
schemas on every sandboxed run. A builtin with no tool that still needs the
network says so through its config default: `iyamlschema` defaults to
`sandbox_network = true` (`ProcessorDefaults::sandbox_network`).

### Not covered

- **Undeclared reads.** Everything is readable by design; see
  [Undeclared Reads](undeclared-reads.md).
- **Writes outside the project** (`$HOME`, `/tmp`) are allowed and not
  checked; see above.
- **What a tool-running processor's own code does in-process** between
  tool runs (renaming a generated file, writing a manifest) is rsconstruct
  code and runs natively. Only builtin processors, whose whole work is
  rsconstruct code, run in a sandboxed child.

## Measured on rsconstruct itself

A forced cold build of this repository (187 products) with `execution_model
= "sandbox"`:

1. **Before `sandbox_writable`:** every rumdl product failed on
   `.rumdl_cache/workspace_index.bin`, and the cargo-based checkers and the
   cargo mass generator failed on `target/`. These are tool state, not
   outputs, which is what led to `sandbox_writable` and its defaults.
2. **With it:** 185 of 187 products pass. The two failures:
   - `processor.checker.clippy.deny`: `cargo deny` fetches the RustSec
     advisory database from GitHub at build time. A correct finding: its
     result depends on the network. Fixed by `cargo-deny` declaring
     `needs_network`.
   - `processor.checker.clippy.nextest`: one test,
     `build::svg_change_rebuilds_marp_and_ipdfunite`, runs marp, whose
     Chromium cannot start inside the sandbox (see open question 1).

Product-level results otherwise matched the native build.

## Open questions

1. **Chromium inside the sandbox.** Ubuntu's `bwrap-userns-restrict`
   AppArmor profile stops processes inside bwrap from creating user
   namespaces of their own. Chromium needs one for its own sandbox, falls
   back to the SUID helper, and aborts ("The SUID sandbox helper binary was
   found, but is not configured correctly"). Anything built on Chromium
   (marp, mermaid, chromium, puppeteer-based tools) therefore fails under
   the sandbox on Ubuntu. Measured on 2026-11-07, outside any other
   sandbox: marp and google-chrome work natively and abort inside bwrap;
   drawio (Electron) aborts the same way. Those four processors are marked
   `sandboxable: false`, and a build that would sandbox one fails up front
   (see "Where it hooks in"); they run with `execution_model = "native"`.
   Still open: passing Chromium `--no-sandbox` through the tool's own
   config, or a bwrap/AppArmor setup that permits nesting, either of which
   would let them be marked sandboxable. It is an environment limitation,
   not a correctness finding.
2. **Changes to the real tree during a run.** overlayfs documents changes
   to the lower layer while it is mounted as undefined behaviour. With
   `-j` > 1, other products move their outputs into the real tree while a
   sandboxed tool's overlay is mounted on it. Dependency ordering means a
   product's own inputs are complete before it starts, so the paths that
   change are ones it should not read; no wrong result has been observed,
   but this deserves watching.
3. **Tools that succeed with a non-zero exit.** A run counts as successful
   only on exit status 0; anything else discards what the tool wrote. A
   processor that accepts a non-zero status and still uses the outputs
   would lose them under the sandbox. None is known yet.
4. **GitHub runners.** `ubuntu-24.04` runners need `bubblewrap` installed
   (`scripts/ci-install-tools.sh`) and must allow it to create user
   namespaces. Verify with a throwaway job before relying on it. If it does
   not work there, the fix is in the environment setup, never in `ci.yml`.
   For the same reason there is no end-to-end sandbox test in `tests/` yet:
   it would need bwrap on every machine that runs the suite.
5. **Cost.** bwrap start-up plus an overlay mount per invocation is a few
   milliseconds; batching amortizes it for checkers. Not yet measured on a
   large fleet build.
6. **Cargo's target directory** is `target` relative to the project root by
   default, which is what cargo declares; a crate in a subdirectory, or a
   `CARGO_TARGET_DIR` inside the project, needs its own `sandbox_writable`
   entry.

## Rollout

1. ~~Route every product's tool runs through one spawn point.~~ Already the
   case; the remaining direct spawns are discovery-time.
2. ~~Implement the opt-in write and network sandbox.~~ Done.
3. Run cold fleet builds with `execution_model = "sandbox"` and count
   violations per processor and repository (started: rsconstruct itself,
   above).
4. Fix the violations: declare outputs, add `sandbox_writable` defaults,
   set `sandbox_network` where a download is the point. The sandbox stays
   opt-in; whether the fleet's shared config turns it on is a separate
   decision.
5. Separately, add the strace-based undeclared-reads report.
