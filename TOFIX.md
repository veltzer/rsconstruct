# TOFIX

Findings from a code scan on 2026-10-04.

## High

- `docs/src/commands.md:282` - documents a `rsconstruct deps` command (list/build/show/...) that no longer exists: the binary answers `error: unrecognized subcommand 'deps'`; the functionality is now `rsconstruct analyzer` (list, build, show, clean, stats, used, ...). Stale `rsconstruct deps` examples (23 lines) are also in `docs/src/analyzers.md:62`-`64` and `docs/src/internal/dependency-caching.md:71`-`75`. Rewrite them against `rsconstruct analyzer`.

## Medium

- `docs/src/binary-releases.md:3` - says releases are published "when a version tag (`v*`) is pushed" and the "Creating a Release" steps (lines 37-40) tell you to `git tag v0.2.2 && git push origin v0.2.2`. The workflow does the opposite: `.github/workflows/ci.yml:8`-`15` triggers on branch pushes only ("the tag push triggers nothing") and releases only from a default-branch commit whose message starts with `chore: Release ` (i.e. `cargo release`). Following the doc produces no release. Document the `cargo release` flow instead.
- `docs/src/commands.md:1` - six top-level subcommands that `rsconstruct --help` lists have no section in the command reference: `analyzer`, `error`, `function`, `hook`, `product`, `symlink-install`. Add them (this also replaces the stale `deps` section above).
- `docs/src/SUMMARY.md:1` - 13 registered processors have no docs page and no SUMMARY entry: `duplicate_files`, `encoding`, `ijq`, `ijsonlint`, `ipdfunite`, `isass`, `itaplo`, `iyamllint`, `license_header`, `marp_images`, `prettier`, `svglint`, `svgo` (diff of `rsconstruct processor list` against `docs/src/processor/*/*.md`). Add a page for each.
- `.markdownlint.json:1` - turns off 11 markdownlint rules (MD022, MD024, MD026, MD031, MD032, MD033, MD034, MD053, MD058 and loosens MD007/MD013), but nothing runs a markdown linter on this repo (`rsconstruct.toml` has no rumdl/markdownlint processor). The file is dead config, and a config-level ignore list goes against the lint policy. Delete it, or add `[processor.rumdl]` for `README.md`/`docs/src` and fix the findings instead of disabling the rules.
