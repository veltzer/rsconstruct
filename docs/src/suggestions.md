# Suggestions

Ideas for future improvements, organized by category.

## Error Handling

### ~~Mutex unwraps in executor.rs~~ *(Done)*
- Switched to `parking_lot::Mutex` which doesn't poison. Eliminated 48 `.lock().unwrap()` calls and 5 `.into_inner().map_err(...)` chains.

## Missing Test Coverage

### ~~Limited parallel execution tests~~ *(Done)*
- Five tests now exercise parallel builds: `-j` flag, keep-going in parallel, all-products parallel, parallel timings, and parallel caching after failure.

### No ruff/pylint processor tests
- `tests/processors/` has tests for cc, sleep, spellcheck, and template, but not for ruff or pylint.
- Add integration tests for both Python linting processors.

### No make processor tests
- `tests/processors/` has no tests for the make processor.
- Add integration tests covering Makefile discovery and execution.

## New Processors

### Linting / Checking (stub-based)

#### shellcheck
- Lint shell scripts (`.sh`, `.bash`) using `shellcheck`.
- Stub-based, very similar to cpplint/ruff pattern.
- Config: `checker` (default `"shellcheck"`), `args`, `extra_inputs`, `scan`.

#### yamllint
- Lint YAML files (`.yml`, `.yaml`) using `yamllint`.
- Catches syntax errors and style violations.
- Config: `linter` (default `"yamllint"`), `args`, `extra_inputs`, `scan`.

#### jsonlint
- Validate JSON files (`.json`) for syntax errors.
- Could use `python3 -m json.tool` or a dedicated tool like `jsonlint`.
- Config: `linter`, `args`, `extra_inputs`, `scan`.

#### toml-lint
- Validate TOML files (`.toml`) for syntax errors.
- Could use `taplo check` or a built-in Rust parser.
- Config: `linter` (default `"taplo"`), `args`, `extra_inputs`, `scan`.

#### markdownlint
- Lint Markdown files (`.md`) for structural issues (complements spellcheck which only checks spelling).
- Uses `mdl` or `markdownlint-cli`.
- Config: `linter` (default `"mdl"`), `args`, `extra_inputs`, `scan`.

#### mypy
- Python type checking using `mypy`.
- Batch-capable like ruff/pylint.
- Config: `args`, `extra_inputs`, `scan`.

#### black-check
- Python formatting verification using `black --check`.
- Verifies files are formatted without modifying them.
- Config: `args`, `extra_inputs`, `scan`.

### Compilation / Generation

#### rust_single_file
- Compile single-file Rust programs (`.rs`) to executables, like cc_single_file but for Rust.
- Useful for exercise/example repositories.
- Config: `rustc` (default `"rustc"`), `flags`, `output_suffix`, `extra_inputs`, `scan`.

#### sass
- Compile `.scss`/`.sass` files to `.css`.
- Single-file transformation using `sass` or `dart-sass`.
- Config: `compiler` (default `"sass"`), `args`, `extra_inputs`, `scan`.

#### protobuf
- Compile `.proto` files to generated code using `protoc`.
- Config: `protoc` (default `"protoc"`), `args`, `language` (default `"cpp"`), `extra_inputs`, `scan`.

#### pandoc
- Convert Markdown (`.md`) to other formats (PDF, HTML, EPUB) using `pandoc`.
- Single-file transformation.
- Config: `output_format` (default `"html"`), `args`, `extra_inputs`, `scan`.

### Testing

#### pytest
- Run Python test files and produce pass/fail stubs.
- Each `test_*.py` file becomes a product.
- Config: `runner` (default `"pytest"`), `args`, `extra_inputs`, `scan` (default extensions `["test_*.py"]`).

#### doctest
- Run Python doctests and produce stubs.
- Each `.py` file with doctests produces a stub.
- Config: `args`, `extra_inputs`, `scan`.

## Security

### Shell command execution from source file comments
- `src/processors/cc.rs` — `EXTRA_*_SHELL` directives execute arbitrary shell commands parsed from source file comments.
- Document the security implications clearly.
