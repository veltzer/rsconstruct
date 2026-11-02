# Python Environment Fingerprint

A processor that runs on the project's Python environment (`./.venv`)
depends on more than its input files. pyrefly resolves imports against the
installed packages, pytest imports them, and a build script that does
`import yaml` produces whatever the installed PyYAML makes of its input.
Since 2026-11 the cache key of such a processor carries a `python_env`
component (see [Cache System](cache.md#cache-key-components)), so upgrading
the packages rebuilds what was checked or built against the old ones.

This chapter records what problem prompted it, the options that were
weighed for each of the three decisions it took, and what was chosen.
The code is `src/python_env.rs`; the builder applies it right after the
tool-version hashes (`Builder::apply_python_env`).

## The problem

On 2026-11-02 the whole fleet ran `uv lock --upgrade && uv sync`. The
following `rsconstruct build` re-ran ruff everywhere ruff itself was
upgraded, and nothing else. A product's key used to be:

1. the content of its input files (for the Python checkers: the source
   file, plus the `dep_auto` config files such as `pyproject.toml`);
2. the processor's config fields;
3. the identity (path, size, mtime) of the processor's tool executables
   (`tool_lock::processor_tool_hashes`), resolved with `./.venv/bin` first
   on PATH.

None of these sees the installed packages. `uv lock --upgrade` changes only
`uv.lock`, which no processor read; `uv sync` changes
`.venv/lib/python3.X/site-packages`, which nothing looked at. So:

- ruff re-ran only because its own binary changed (correctly);
- pyrefly, whose verdict depends on the installed packages and their stubs,
  stayed cached wherever pyrefly itself was not upgraded (128 repos);
- pytest re-ran only where pytest itself happened to be upgraded;
- build scripts run by generic processors (`command = "python"`,
  `command = "scripts/x.py"`) stayed cached even where the packages they use
  changed: in teaching-animations (manim 0.22.0 to 0.22.1, 24 animations)
  and demos-lang-python-ds (ipython, python-dateutil 4.0.0, fonttools, 24
  executed notebooks). For `command = "python"` the tool identity is
  `.venv/bin/python`, a symlink to the system interpreter that `uv sync`
  never touches.
- sphinx re-ran only because the 52 py* repos had hand-added
  `dep_inputs = [..., "uv.lock"]`.

## Decision 1: what is fingerprinted

| | Fingerprint | Cost per build | Lock refreshed, not synced | `uv pip install` / manual change | Same on every machine | False rebuilds |
|---|---|---|---|---|---|---|
| A | Hash of the whole `uv.lock` file | read ~1 MB | **Wrong**: re-runs against the *old* packages and caches that result under the new key | Missed | Yes | Any lock edit, even with no version change |
| B | `uv.lock` parsed to (name, version, source) per package | parse ~1 MB | **Wrong**, as A | Missed | Yes | Real version changes only |
| **C** | **Installed set: sorted `*.dist-info` / `*.egg-info` directory names in `site-packages` (each is `name-version`), plus the interpreter version from `pyvenv.cfg`** | **one `readdir` and one small file** | **Right**: nothing changes until the sync | **Caught** | **Yes, for the same packages** | **Real version changes only** |
| D | As C, but hashing each distribution's `RECORD` (sha256 of every installed file) | ~700 small reads, a few ms | Right | Caught, including a rebuilt wheel with an unchanged version | Yes | Real changes only |
| E | mtime of the `site-packages` directory | one `stat` | Right | Caught | **No**: breaks a shared/remote cache | Anything that touches the directory |

**Chosen: C.** It fingerprints what the tools actually run against rather
than what the lock says should be there, which avoids A and B's failure of
caching a result computed against the old packages under the new key. D
adds only the "same version, different build" case, which uv and PyPI
practically never produce. E is non-deterministic across machines.

Details of C as implemented:

- Only the `implementation` and `version*` lines of `pyvenv.cfg` count
  (`version` from `python -m venv`, `version_info` from uv). The `home` path
  and the uv release that created the venv say nothing about what code runs
  and would invalidate every key on an unrelated change.
- An interpreter upgrade (3.14.0 to 3.14.1) changes the standard library
  while the venv's `python` symlink stays put; the `pyvenv.cfg` version is
  how it is noticed. (`uv sync` rewrites `pyvenv.cfg` when it recreates the
  venv for a new interpreter.)
- The venv location is not part of the fingerprint, so two checkouts with
  the same packages share keys.
- No `./.venv/pyvenv.cfg` means no fingerprint and no component: a project
  without a venv has no Python state to track.
- A py* repo installs its own package into its venv, so bumping its version
  also changes the fingerprint. That is arguably right (the installed
  metadata changed) and costs one rebuild per release.

## Decision 2: which processors carry it

**Python checkers, by plugin default.** ruff, pyrefly, mypy, pylint,
black, pytest, doctest and the sphinx mass generator set
`hash_python_env: true` in their `ProcessorDefaults`. ruff and black do not
read installed packages, so for them this only costs re-runs. It was a
deliberate decision (ruff is fast, and "every Python checker re-checks
after an environment change" is a simpler rule to state). ruff and pyrefly
also need the default because they are native binaries, which detection
cannot see running Python.

**Python scripts.** Two options were weighed:

- **(i) Automatic detection (chosen).** A processor whose resolved
  `command` (or any other declared required tool) runs the venv's Python is
  keyed:
    - the tool resolves inside `./.venv` and is an interpreter
      (`python`, `python3`, `python3.14`), or
    - the tool is a script whose shebang names an interpreter inside
      `./.venv` (what `uv sync` writes into `.venv/bin/pytest`), or
      `/usr/bin/env python*` (resolved through PATH, where the venv comes
      first).

  This covers `command = "python"`, `python -m scripts.x`,
  `scripts/build.py` with `#!/usr/bin/env python`, Python console scripts
  installed in the venv (yamllint, cpplint, sphinx-build, ...), and the
  tera generator, whose `load_python` function runs the venv's `python3`.
  A system script with `#!/usr/bin/python3` is Python too, but it never
  sees the venv's packages and is not keyed. Detection misses Python run
  from behind a shell wrapper; such a processor sets
  `hash_python_env = true`.
- (ii) Opt-in only, `hash_python_env = true` per processor. Rejected:
  explicit, but it is exactly the per-repo setting that gets forgotten, and
  forgetting it is silent.

Either way the per-processor field overrides: `true` keys a processor
detection cannot see, `false` keeps the venv out of a processor's key even
if it is a Python checker or detected. `[build] hash_python_env = false`
turns the whole mechanism off.

## Decision 3: how much of the environment counts

- **Whole venv (chosen).** Any package change rebuilds every keyed
  processor. Simple, and it catches dependencies reached in ways no static
  analysis sees: tools launched through `subprocess` (manim, `jupyter
  nbconvert`), plugins, dynamic imports.
- Per-script transitive imports only (hash just the distributions a script
  imports, recursively). Rejected: more precise, but it misses dynamic
  imports, subprocess-launched tools and plugin entry points, and it is a
  lot of machinery for a saving that only matters on lock refreshes.

## Consequences

- `rsconstruct product show <path>` lists the `python_env` component, so a
  rebuild caused by an environment change is attributable.
- The hand-added `"uv.lock"` in the py* repos' sphinx `dep_inputs` is
  redundant now that sphinx carries the component by default.
- After `uv lock --upgrade && uv sync`, every keyed processor rebuilds once.
  With the fleet's ~1000-file Python repos that means a full ruff and
  pyrefly pass, which is the intent.
