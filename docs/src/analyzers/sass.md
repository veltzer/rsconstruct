# sass

Scans Sass and SCSS files for `@use`, `@forward` and `@import` rules and adds every local file they load as a dependency, following imports transitively. Declare it whenever a project compiles Sass with [isass](../processor/generator/isass.md) or [sass](../processor/generator/sass.md): without it, a stylesheet's only input is its own file, so editing a partial it `@use`s does not rebuild it and the old CSS stays in `out/`.

**Native**: Yes.

**Auto-detects**: Projects with `.scss` or `.sass` files.

## Features

- Follows `@use "x"`, `@forward "x"` and `@import "a", "b"`, in both SCSS and the indented syntax (where `@import a, b` may be unquoted).
- Resolves URLs the way the Sass compiler does: relative to the importing file, then each of `load_paths`; a URL without an extension is tried as `.scss`, `.sass` and `.css`, each also as a partial (`_name`), then as a directory's `index` / `_index` file.
- Follows imports transitively: a partial that `@use`s another partial makes both inputs of every stylesheet that loads it.
- Ignores what loads no project file: built-in modules (`@use "sass:math"`), `pkg:` and remote URLs, and plain CSS imports (`@import "x.css"`, `@import url(...)`). A `@use "x.css"` does load the file and is tracked.
- Ignores rules inside `//` and `/* */` comments.
- Resolves a partial that another product generates, before it exists: the stylesheet is built after the partial's producer (see [Generated files](../analyzers.md#generated-files)).

Every source is rescanned on every build rather than read from the dependency cache. The cache is keyed by the importing file's own contents, so it cannot see a `@use` added inside a partial; scanning is cheap enough that always rescanning costs nothing noticeable.

## Configuration

```toml
[analyzer.sass]
load_paths     = ["node_modules", "vendor/styles"]
skip_not_found = false
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `true` | Set to `false` to keep the stanza but skip the analyzer |
| `load_paths` | string[] | `[]` | Directories searched after the importing file's own directory, like the compiler's `--load-path` |
| `skip_not_found` | bool | `false` | Skip imports that resolve nowhere instead of failing |

### `skip_not_found` (default: `false`)

- `false` (default) — an import of a local file that resolves nowhere is a hard error naming the file, the URL and the directories searched:

  ```text
  Sass import not found: "nope" in sass/style.scss (searched sass and load_paths []). Add its directory to load_paths, or set skip_not_found = true.
  ```

- `true` — such imports are skipped. Use it when the compiler finds files through a mechanism the analyzer does not model (a custom importer, say); the imports it skips are then not tracked.

## See also

- [isass](../processor/generator/isass.md) — in-process Sass compiler
- [sass](../processor/generator/sass.md) — Sass compiler (external `sass` command)
