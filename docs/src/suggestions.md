# Suggestions

Ideas for future improvements, organized by category.

## Error Handling

### Mutex unwraps in executor.rs
- `src/executor.rs` has many `.lock().unwrap()` calls on mutexes. If a thread panics while holding a lock, this causes cascading panics.
- Consider using `parking_lot::Mutex` (no poisoning) or handling `PoisonError`.

## Missing Test Coverage

### Limited parallel execution tests
- Only one integration test exercises parallel builds (`independent_products_cached_after_failure_parallel`).
- Add tests for: correct build order with `-j4`, parallel with dependencies, keep-going in parallel, interrupt handling in parallel.

### No ruff/pylint processor tests
- `tests/processors/` has tests for cc, sleep, spellcheck, and template, but not for ruff or pylint.
- Add integration tests for both Python linting processors.

### No make processor tests
- `tests/processors/` has no tests for the make processor.
- Add integration tests covering Makefile discovery and execution.

## Architecture

### Hard-coded processor list in main.rs
- `src/main.rs` — The `all_processors` array is hard-coded. Adding a new processor requires updating this list manually.
- Generate from the processor registry or use a macro.

## Security

### Shell command execution from source file comments
- `src/processors/cc.rs` — `EXTRA_*_SHELL` directives execute arbitrary shell commands parsed from source file comments.
- Document the security implications clearly.
