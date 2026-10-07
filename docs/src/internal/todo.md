# TODO

## Housekeeping

- Split `db.redb`. `CONFIGS_TABLE` is now the only table in it
  (`src/object_store/mod.rs`), so renaming the file to `configs.redb` is a
  pure cosmetic rename with no correctness argument behind it. Low priority.
