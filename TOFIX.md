# TOFIX

Findings from writing the missing processor docs pages on 2026-10-06. Each
was reproduced in a scratch project; the pages document the current
behavior, so fixing an item means updating its page too.

## Medium

- `src/processor/checker/iyamllint.rs:27` - parses with `serde_yaml_ng::from_str`, which rejects valid multi-document YAML (`---` separators) with "more than one document is not supported". Parse with a multi-document loader instead. This also matters for plan.md Stage 1, which counts iyamllint as yamllint's Rust alternative: it honours none of `.yamllint.yaml`'s rules (syntax and duplicate keys only), so that row's "confirm every option is honoured" is really "implement the rules".
- `src/processor/generator/isass.rs` - partials (`_*.scss`) are compiled to their own `.css`, unlike the `sass` CLI. Skip files whose name starts with `_` by default.
- `src/builder/fix.rs:43` - `rsconstruct fix run <name>` with a name that matches no fix-capable processor (e.g. the short `script.foo` instead of `processor.checker.script.foo`) prints "No matching processors with fix capability." and exits 0. A requested processor that does not exist should fail, and the short form could be accepted the way `build -p` accepts inames.
- `src/processor/checker/script.rs:26` - `fix_batch` shows as type `?` in `rsconstruct processor defconfig processor.checker.script`: it has no `FieldSpec` with a type. Add one.
- `plan.md` Stage 3 `iyq` row says "reuse ijq's filter evaluation", but `ijq` evaluates no filters: it is a JSON parse check identical to `ijsonlint`. The row needs a real jq engine (e.g. the `jaq` crate).
- Markdown in `README.md` and `docs/src` is not linted. `.markdownlint.json` was deleted as dead config; adding a `rumdl` stanza (with the fleet-shared `.rumdl.toml`) would lint locally, but CI never runs `rsconstruct build` in rs* repos, so enforcing it needs a decision about the shared `ci.yml`.

## Low

- `src/deps_cache.rs` - a cached dependency list is now rechecked against every listed dependency's checksum, but files that are *not* on the list are not watched. Two cases still keep a stale list until the source itself changes: a quoted `#include` skipped as unresolved (`skip_not_found = true`) whose header is created later, and a new header that shadows the resolved one earlier in the search path (e.g. `src/x.h` appearing when `include/x.h` was found). Fixing it means recording the paths probed during resolution, not only the ones found.
- `src/analyzers/python.rs:78` - records the direct imports of a module only, not imports of imports, so a product depending on `a.py` is not rebuilt when a module `a.py` imports changes. Follow imports transitively, as `icpp` does for headers.
