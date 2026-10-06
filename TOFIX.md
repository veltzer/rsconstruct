# TOFIX

Findings from a code scan on 2026-10-04, plus gaps found on 2026-10-06 while
rewriting the command reference.

## Medium

- `docs/src/SUMMARY.md:1` - 13 registered processors have no docs page and no SUMMARY entry: `duplicate_files`, `encoding`, `ijq`, `ijsonlint`, `ipdfunite`, `isass`, `itaplo`, `iyamllint`, `license_header`, `marp_images`, `prettier`, `svglint`, `svgo` (diff of `rsconstruct processor list` against `docs/src/processor/*/*.md`). Add a page for each.
- `.markdownlint.json:1` - turns off 11 markdownlint rules (MD022, MD024, MD026, MD031, MD032, MD033, MD034, MD053, MD058 and loosens MD007/MD013), but nothing runs a markdown linter on this repo (`rsconstruct.toml` has no rumdl/markdownlint processor). The file is dead config, and a config-level ignore list goes against the lint policy. Delete it, or add `[processor.rumdl]` for `README.md`/`docs/src` and fix the findings instead of disabling the rules.
- `docs/src/processor/checker/script.md` - does not document `fix_command`, `fix_args` or `fix_batch`, which since 2026-08-04 are the only way to make a processor fix-capable for `rsconstruct fix` (every plugin's `can_fix` is `false`). Add them.
- `docs/src/configuration.md` - has no section for `[command.symlink_install]` (`sources`/`targets`); it is described only under `rsconstruct symlink-install` in `commands.md`. Add it to the configuration reference.

## Low

- `docs/src/internal/suggestions-done.md:108` - still lists ruff, black, prettier, eslint, stylelint, standard, taplo, rumdl and markdownlint as fix-capable; the 2026-08-04 decision set every `can_fix` to `false`. Note the reversal there.
