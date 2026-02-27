# Coding Standards

Rules that apply to the RSB codebase and its documentation.

## Fail hard, never degrade gracefully

When something fails, it must fail the entire build. Do not try-and-fallback,
do not silently substitute defaults for missing resources, do not swallow errors.
If a processor is configured to use a file and that file does not exist, that is
an error. The user must fix their configuration or their project, not the code.

Optional features must be opt-in via explicit configuration (default off).
When the user enables a feature, all resources it requires must exist.

## Processor naming conventions

Every processor has a single **identity string** (e.g. `ruff`, `clang_tidy`,
`mdbook`). All artifacts derived from a processor must use that same string
consistently:

| Artifact | Convention | Example (`clang_tidy`) |
|---|---|---|
| Name constant | `pub const UPPER: &str = "name";` in `processors::names` | `CLANG_TIDY: &str = "clang_tidy"` |
| Source file | `src/processors/checkers/{name}.rs` or `generators/{name}.rs` | `checkers/clang_tidy.rs` |
| Processor struct | `{PascalCase}Processor` | `ClangTidyProcessor` |
| Config struct | `{PascalCase}Config` | `ClangTidyConfig` |
| Field on `ProcessorConfig` | `pub {name}: {PascalCase}Config` | `pub clang_tidy: ClangTidyConfig` |
| Match arm in `processor_enabled_field()` | `"{name}" => self.{name}.enabled` | `"clang_tidy" => self.clang_tidy.enabled` |
| Entry in `default_processors()` | `names::UPPER.into()` | `names::CLANG_TIDY.into()` |
| Entry in `validate_processor_fields()` | `processor_names::UPPER => {PascalCase}Config::known_fields()` | `processor_names::CLANG_TIDY => ClangTidyConfig::known_fields()` |
| Entry in `expected_field_type()` | `("{name}", "field") => Some(FieldType::...)` | `("clang_tidy", "compiler_args") => ...` |
| Entry in `scan_dirs()` | `&self.{name}.scan` | `&self.clang_tidy.scan` |
| Entry in `resolve_scan_defaults()` | `self.{name}.scan.resolve(...)` | `self.clang_tidy.scan.resolve(...)` |
| Registration in `create_builtin_processors()` | `Builder::register(..., proc_names::UPPER, {PascalCase}Processor::new(cfg.{name}.clone()))` | `Builder::register(..., proc_names::CLANG_TIDY, ClangTidyProcessor::new(cfg.clang_tidy.clone()))` |
| Re-export in `processors/mod.rs` | `pub use checkers::{PascalCase}Processor` | `pub use checkers::ClangTidyProcessor` |
| Install command in `tool_install_command()` | `"{tool}" => Some("...")` | `"clang-tidy" => Some("apt install clang-tidy")` |

When adding a new processor, use the identity string everywhere. Do not
abbreviate, rename, or add suffixes (`Gen`, `Bin`, etc.) to any of the
derived names.

## Test naming for processors

Test functions for a processor must be prefixed with the processor name.
For example, tests for the `cc_single_file` processor must be named
`cc_single_file_compile`, `cc_single_file_incremental_skip`, etc.
Tests for the `sleep` processor must be named `sleep_processor`,
`sleep_extra_inputs_valid`, etc.

## No indented output

All `println!` output must start at column 0. Never prefix output with spaces
or tabs for visual indentation unless when printing some data with structure.

## Suppress tool output on success

External tool output (compilers, linters, etc.) must be captured and only
shown when a command fails. On success, only rsb's own status messages appear.
Users who want to always see tool output can use `--show-output`. This keeps
build output clean while still showing errors when something goes wrong.

## Never hard-code counts of dynamic sets

Documentation and code must never state the number of processors, commands,
or any other set that changes as the project evolves. Use phrasing like
"all processors" instead of "all seven processors". Enumerating the members
of a set is acceptable; stating the cardinality is not.

## Use well-established crates

Prefer well-established crates over hand-rolled implementations for common
functionality (date/time, parsing, hashing, etc.). The Rust ecosystem has
mature, well-tested libraries for most tasks. Writing custom implementations
introduces unnecessary bugs and maintenance burden. If a crate exists for it,
use it.

## No trailing newlines in output

Output strings passed to `println!`, `pb.println()`, or similar macros must not
contain trailing newlines. These macros already append a newline. Adding `\n`
inside the string produces unwanted blank lines in the output.

## Include processor name in error messages

Error messages from processor execution must identify the processor so the
user can immediately tell which processor failed. The executor's
`record_failure()` method automatically wraps every error with
`[processor_name]` before printing or storing it, so processors do not need
to manually prefix their `bail!` messages. Just write the error naturally
(e.g. `bail!("Misspelled words in {}", path)`) and the executor will produce
`[aspell] Misspelled words in README.md`.

## Reject unknown config fields

All config structs that don't intentionally capture extra fields must use
`#[serde(deny_unknown_fields)]`. This ensures that typos or unsupported
options in `rsb.toml` produce a clear error instead of being silently ignored.

Structs that use `#[serde(flatten)]` to embed other structs (like `ScanConfig`)
cannot use `deny_unknown_fields` due to serde limitations. These structs must
instead implement the `KnownFields` trait, returning a static slice of all
valid field names (own fields + flattened fields). The `validate_processor_fields()`
function in `Config::load()` checks all `[processor.X]` keys against these
lists before deserialization.

Structs that intentionally capture unknown fields (like `ProcessorConfig.extra`
for Lua plugins) should use neither `deny_unknown_fields` nor `KnownFields`.
