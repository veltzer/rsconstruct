# python

Scans Python source files for `import` and `from ... import` statements and adds dependencies on local Python modules.

**Native**: Yes.

**Auto-detects**: Projects with `.py` files.

## Features

- Follows imports **transitively**: running `main.py` runs what its imports
  import, so a module two imports away is a dependency, and editing it
  rebuilds `main.py`'s products. Import cycles are fine; a module is never
  its own dependency.
- Resolves imports to local files and ignores stdlib and third-party
  packages. A module name is looked up as `name.py` or `name/__init__.py`
  under each root in turn: the importing file's directory, the project
  root, then each entry of `search_paths` (default `src`, for the `src/`
  package layout). The first root that has it wins.
- Depends on the `__init__.py` of every package above an imported module:
  importing `app.util.fs` runs `app/__init__.py` and `app/util/__init__.py`.
- `from pkg import name` also resolves `pkg.name`, so importing a submodule
  by name (`from app.util import fs`) depends on `app/util/fs.py`. A name
  that is not a module resolves nowhere and adds nothing.
- Parses `import a, b`, `import a.b as c`, `from x import (` … `)` across
  lines, and imports indented inside functions or `try` blocks. Comments
  are skipped.
- Every path probed and found missing is recorded, so a local module that
  appears later, or one that would now win in an earlier root, triggers a
  rescan.

Relative imports (`from . import x`) are not resolved.

## Configuration

```toml
[analyzer.python]
search_paths = ["src"]   # extra roots for absolute imports, relative to the project root
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `true` | Set to `false` to keep the stanza but not run the analyzer |
| `search_paths` | string[] | `["src"]` | Directories that absolute imports resolve against, after the importing file's directory and the project root. A directory that does not exist matches nothing. Changing it rescans every file |
