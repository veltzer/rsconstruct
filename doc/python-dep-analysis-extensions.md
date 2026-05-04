# Feature design: extending the Python dependency analyzer

## Status

Draft — awaiting review.

## Origin

`problems.txt` line 1–4. The current Python analyzer (`src/analyzers/python.rs`)
already handles one of the three requested features. Two remain.

## What's already in place

`scan_python_imports` parses both `import X` and `from X import Y` (regex,
comment-aware). `PythonDepAnalyzer::resolve_module` then tries to resolve
each module to a local project file by looking for `module.py` or
`module/__init__.py`, relative to the importing file and relative to the
project root. Local hits become build-graph dependencies. Non-local names
(stdlib, third-party) are silently dropped.

So this part of the request — **"for `import local_python_module` statements"**
— is done. The remaining two are not.

## Open item A — file dependencies via `open(...)` and similar

The user wants the analyzer to recognize that

```python
with open("data/config.yaml") as f:
    ...
```

means `data/config.yaml` is a build-input dependency of this Python file.
The same logic would extend to other read functions.

This is genuinely hard, in three ways.

### A.1 Which functions count?

The obvious ones — `open`, `pathlib.Path(...).read_text`,
`pathlib.Path(...).open`, `json.load`, `yaml.safe_load`, `csv.reader`,
`tomllib.load`, `pickle.load`, `numpy.load`, `pandas.read_csv`, …

Each one has its own argument shape (`open(path)`, `Path(path).read_text()`,
`json.load(file_obj)` — *not* a path, you have to trace back to the file
object). A complete list is unbounded; user code can wrap any of these.

**Proposal**: maintain a small, explicit allowlist of recognized call shapes.
Not a complete list — a useful one. Anything not on the list is silently
ignored. Document the list in the analyzer's book chapter so users know
what's recognized.

Initial allowlist (small on purpose):

| Pattern                       | Argument that's the path |
| ----------------------------- | ------------------------ |
| `open(<arg>)`                 | first positional         |
| `open(<arg>, ...)`            | first positional         |
| `pathlib.Path(<arg>)`         | first positional         |
| `Path(<arg>)`                 | first positional         |

The user can extend the list later. We do NOT chase aliases (`my_open = open;
my_open("x")` is not recognized) — that requires real flow analysis.

### A.2 Which arguments resolve?

Only **string literals** — `open("data/config.yaml")`. Not f-strings, not
variables, not concatenations. If we can't see the path syntactically, we
ignore the call. This is the same trade-off the import scanner makes
(`__import__(name)` with a variable is ignored).

This is the conservative/safe choice: false negatives (missed deps) are
recoverable (user adds the dep manually); false positives (wrong deps) are
not (the build graph gets corrupted).

### A.3 Regex vs AST

Regex is what we use today for imports. It's fragile but fast and has zero
runtime dependency on Python being installed.

For `open(...)` calls, regex starts to break down: `open` as a method name
(`f.open`), `open` in a string, `open` in a comment, multi-line calls, and
so on. An AST walker via Python's `ast` module is much more accurate but
forces a Python interpreter at analyze time.

**Proposal**: stick with regex for now, written conservatively to minimize
false positives:

- Match only at statement-leading positions or after `=` / `with`.
- Require the call to start with `open(` followed by a quoted string.
- Skip lines that are inside triple-quoted strings (best-effort).

If users complain about misses, we upgrade to AST-based later. The regex
approach is the same shape as the import scanner; the AST approach is a
bigger architectural change (analyzer becomes Python-dependent).

### A.4 What does "dependency" mean for these files?

For imports, "X depends on Y" means "if Y changes, the product built from X
must rebuild". The same semantics apply here: if `data/config.yaml` changes,
the product whose script reads it should rebuild.

This implies: the discovered file path is added to the importing product's
input set (same mechanism as imports). No new graph concept needed.

### A.5 Recommendation

Yes — but conservative. Regex-based, string-literal-only, small allowlist of
call shapes. Documented limits. Off by default behind a config flag
(`scan_runtime_reads = false`) for the first release because false positives
in any analyzer are expensive — let users opt in until we have confidence.

## Open item B — surfacing external (third-party) imports

Today, when `resolve_module` returns `None`, the module name is discarded.
The user wants these collected and surfaced — presumably to keep
`[dependencies].pip` in sync with what the code actually imports.

This is much more tractable than item A.

### B.1 Definition of "external"

A module is external if all of:

- It is not resolvable to a local project file (`resolve_module` returns
  None) — already the case.
- It is not in the Python standard library.

The stdlib check needs a list. `sys.stdlib_module_names` (Python 3.10+) is
authoritative. We could ship a baked-in list (regenerated per Python
release) or call out to the running Python at analyze time. Baked-in is
simpler and avoids the Python-at-analyze-time dependency; we accept the
tax of regenerating it occasionally.

### B.2 What do we do with the list?

Three options, in decreasing magic:

1. **Auto-update `[dependencies].pip`**. The analyzer rewrites the toml.
   Convenient but invasive — surprise edits to a config file the user owns.
2. **Diff and report**. `rsconstruct python-deps check` prints the set of
   imported externals minus the set declared in `[dependencies].pip`,
   exits non-zero if they differ. User opts in to fix.
3. **Surface as a separate output**. `rsconstruct python-deps list`
   prints the externals; nothing happens automatically.

**Proposal**: do (2) and (3), not (1). Option 1 is too aggressive given
rsconstruct's strict-by-default culture (silent config edits are a strong
no-no). The two commands above are read-only and let the user decide.

### B.3 Module name → package name

`import yaml` is provided by the `pyyaml` package. `import cv2` is
`opencv-python`. The mapping is not 1:1 and not derivable from the import
name alone.

Maintain a small mapping table for the common cases (`yaml → pyyaml`,
`cv2 → opencv-python`, `PIL → pillow`, `bs4 → beautifulsoup4`, `sklearn →
scikit-learn`, `dateutil → python-dateutil`, …) and otherwise assume the
package name equals the import name. Ship the table in the analyzer; let
users extend it via config (`extra_package_aliases = { "myimport" =
"mypackage" }`).

The `check` command should diff at the *package* level, not the import
level, after applying the mapping.

### B.4 Recommendation

Yes — straightforward, low-risk, useful. Ship as two commands plus the
mapping table. Off by default if it adds runtime cost; otherwise just
always available.

## Questions for the user before implementation

1. **Item A**: do you want runtime-read scanning at all? It is the more
   speculative of the two and has the higher false-positive risk. If you'd
   rather skip it entirely and just cover imports, that's a simpler ship.

2. **Item A allowlist**: is the four-pattern starting list above sufficient,
   or do you have a specific set of read functions you're hitting in your
   own code that I should add?

3. **Item B mode**: are the two commands (`check` + `list`) the right shape,
   or do you want an auto-update path despite the warnings? (Auto-update
   could be opt-in via `--write` flag — at least it's explicit.)

4. **Item B mapping table**: do you want the `[dependencies].pip` entries
   matched at the import level (`yaml`) or the package level (`pyyaml`)?
   The package level is more accurate; the import level is simpler to
   implement and matches what `pip show` accepts.

5. **Naming of the new commands**: `rsconstruct python-deps {check,list}`
   vs. extending the existing `analyzers` subcommand vs. extending
   `tools install-deps`. The third would be the most integrated but blurs
   the line between "what this code uses" and "what's declared".
