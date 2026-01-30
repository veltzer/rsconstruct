# Suggestions

## Code Duplication

### ruff.rs and pylint.rs are nearly identical
- `src/processors/ruff.rs` and `src/processors/pylint.rs` share ~150 lines of identical code: `should_lint()`, `find_python_files()`, `has_python_files()`, `find_py_files_in_dir()`, `find_py_files_in_project()`, `get_stub_path()`, `clean()`, and most of `discover()` and `execute()`.
- Extract common functionality into a shared `PythonLinter` base or a generic struct parameterized by lint command and stub directory name.

### Directory filtering logic repeated across processors
- `ruff.rs`, `pylint.rs`, `cpplint.rs`, and `spellcheck.rs` all repeat the same directory exclusion patterns (`.venv/`, `__pycache__/`, `.git/`, `out/`, etc.).
- Extract into a shared utility function.

## Naming Inconsistencies

### Processor struct names
- Most processors are named `{Name}Processor` (`RuffProcessor`, `PylintProcessor`, `CcProcessor`, `SpellcheckProcessor`, `SleepProcessor`, `TemplateProcessor`).
- `Cpplinter` breaks this convention. Rename to `CpplintProcessor`.

## Error Handling

### Mutex unwraps in executor.rs
- `src/executor.rs` has many `.lock().unwrap()` calls on mutexes. If a thread panics while holding a lock, this causes cascading panics.
- Consider using `parking_lot::Mutex` (no poisoning) or handling `PoisonError`.

### Unwraps in graph.rs
- `src/graph.rs:136-137` — `get_mut().unwrap()` on `dependents` and `dependencies` maps.
- Use `unwrap_or_else()` with a meaningful panic message or propagate errors.

## Potential Bugs

### Combined input checksum skips missing files
- `src/object_store.rs` — `combined_input_checksum()` silently skips files that don't exist. Two different input sets with different missing files could produce the same checksum.
- Include a marker for missing files in the hash to avoid collisions.

### Hard link fallback is silent
- `src/object_store.rs:156-162` — Hard link failure silently falls back to copy. The user has no way to know this happened.
- Log the fallback at debug level.

## Missing Test Coverage

### No parallel execution tests
- No integration tests exercise the `-j` flag. Parallel code paths in `executor.rs` are complex and untested.
- Add tests for: correct build order with `-j4`, parallel with dependencies, keep-going in parallel, interrupt handling in parallel.

### No ruff/pylint processor tests
- `tests/processors/` has tests for cc, sleep, spellcheck, and template, but not for ruff or pylint.
- Add integration tests for both Python linting processors.

## Architecture

### Hard-coded processor list in main.rs
- `src/main.rs:106-114` — The `all_processors` array is hard-coded. Adding a new processor requires updating this list manually.
- Generate from the processor registry or use a macro.

### Hard-coded stub directory cleanup in builder.rs
- `src/builder.rs:176-222` — The `clean()` method manually lists each processor's output directory. Adding a new processor requires updating this list.
- Have each processor declare its output directory, or iterate over all `out/` subdirectories.

### Re-traversal of directory trees
- Each processor walks the filesystem independently. For large projects with many processors enabled, this is wasteful.
- Consider a shared file discovery cache populated once and consumed by all processors.

## Configuration

### Default enabled processors include sleep
- `src/config.rs:110` — Default `enabled` list includes `sleep`, which is a testing-only processor.
- Remove `sleep` from defaults.

## Performance

### String allocations in config_hash
- `src/config.rs:24-28` — `config_hash()` serializes to JSON string for every product during discovery.
- Cache the hash per processor config instead of recomputing.

### Excessive cloning in parallel executor
- `src/executor.rs` — Many `Arc::clone()` and string clones in hot paths.
- Profile and optimize; consider thread-local statistics collection with a final merge.

## Security

### Shell command execution from source file comments
- `src/processors/cc.rs` — `EXTRA_*_SHELL` directives execute arbitrary shell commands parsed from source file comments.
- Document the security implications clearly.

## Documentation

### CLAUDE.md outdated doc/ reference
- `CLAUDE.md` still references `doc/` in the project structure tree. Should be `docs/`.
