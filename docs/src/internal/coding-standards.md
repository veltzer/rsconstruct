# Coding Standards

Rules that apply to the RSConstruct codebase and its documentation.

## Always add context to errors

Every `?` on an IO operation must have `.with_context()` from `anyhow::Context`. A bare `?` on `fs::read`, `fs::write`, `fs::create_dir_all`, `Command::spawn`, or any other syscall-wrapping function is a bug. It produces error messages like "No such file or directory" with no indication of which file or which operation failed.

Good:
```rust
fs::read(&path)
    .with_context(|| format!("Failed to read config file: {}", path.display()))?;
```

Bad:
```rust
fs::read(&path)?;  // useless error message
```

The error chain should read like a stack trace of intent: "Failed to build project > Failed to execute ruff on src/main.py > Failed to spawn command: ruff > No such file or directory".

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
| Entry in `src_dirs()` | `&self.{name}.scan` | `&self.clang_tidy.scan` |
| Entry in `resolve_scan_defaults()` | `self.{name}.scan.resolve(...)` | `self.clang_tidy.scan.resolve(...)` |
| Registration in `create_builtin_processors()` | `Builder::register(..., proc_names::UPPER, {PascalCase}Processor::new(cfg.{name}.clone()))` | `Builder::register(..., proc_names::CLANG_TIDY, ClangTidyProcessor::new(cfg.clang_tidy.clone()))` |
| Re-export in `processors/mod.rs` | `pub use checkers::{PascalCase}Processor` | `pub use checkers::ClangTidyProcessor` |
| Install command in `tool_install_command()` | `"{tool}" => Some("...")` | `"clang-tidy" => Some("apt install clang-tidy")` |

When adding a new processor, use the identity string everywhere. Do not
abbreviate, rename, or add suffixes (`Gen`, `Bin`, etc.) to any of the
derived names.

Never use a `_check` suffix in processor names. Name the processor after the
tool or library it wraps — do not abstract or rename it (e.g. `zspell` not
`spellcheck`, `ruff` not `python_lint`).

## Processor `new()` must be infallible

Every processor's `fn new(config: XxxConfig) -> Self` must return `Self`, not
`Result<Self>`. This is enforced at compile time by the registry macro. If
construction can fail, defer the failure to `execute()` or `discover()`.

## Processor directory layout

Each processor category directory (`src/processors/checkers/`,
`src/processors/generators/`, `src/processors/creators/`) must contain
only processor implementation files — one processor per `.rs` file (plus
`mod.rs`). Shared utilities, helpers, or supporting code used by multiple
processors must live in `src/processors/` directly, not inside a category
subdirectory. This keeps each category directory a flat, scannable list of
processors.

## Test naming for processors

Test functions for a processor must be prefixed with the processor name.
For example, tests for the `cc_single_file` processor must be named
`cc_single_file_compile`, `cc_single_file_incremental_skip`, etc.

## No indented output

All `println!` output must start at column 0. Never prefix output with spaces
or tabs for visual indentation unless when printing some data with structure.

## No shell when spawning subprocesses

Subprocesses must be spawned via direct argv execution
(`Command::new(prog).args([...])`), not via `sh -c` or any other shell.
Routing user-controlled or config-controlled strings through a shell turns
ordinary characters like `<`, `>`, `|`, `&`, `;`, `$`, and spaces into
code-injection surface. A shell is also unnecessary — every package manager
and tool we invoke accepts argv directly.

A small number of features (Tera `shell_output()`, C source pragmas like
`EXTRA_COMPILE_SHELL`, the icpp analyzer's `include_path_commands`, and the
free-form `binary` install methods in the static tool registry) are
contractually shell-shaped and are listed as named exceptions. See
[No-Shell Policy](no-shell-policy.md) for the full rationale, the exception
table, and how to add a new exception if one is genuinely needed.

## Suppress tool output on success

External tool output (compilers, linters, etc.) must be captured and only
shown when a command fails. On success, only rsconstruct's own status messages appear.
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

## Never silently ignore user configuration

Every field a user can write in `rsconstruct.toml` (or in any YAML/TOML
manifest we load: `cc.yaml`, `linux-module.yaml`, etc.) must produce an
observable effect in the engine. The two failure modes to prevent are:

1. **Schema-level silent-ignore** — serde accepts an unknown field because
   the struct doesn't reject it. A user typos `enabeld = false`, we accept
   it, nothing happens, they wonder why their setting had no effect.
2. **Runtime silent-ignore** — serde stores the field in a struct, but no
   code in the engine ever reads it. This is exactly how the
   `[analyzer.X] enabled = false` bug shipped: the CLI subcommand wrote the
   field, the config loader happily deserialized it, and the analyzer
   runner ignored it. A half-wired feature is worse than no feature.

### Rule 1: reject unknown fields at the schema level

Every struct that deserializes user input must use one of:

- `#[serde(deny_unknown_fields)]` — preferred for plain structs (no
  `#[serde(flatten)]`). Serde enforces the reject at deserialize time.
- `KnownFields` trait + `validate_processor_fields()` — for top-level
  processor configs that use `#[serde(flatten)]` to embed `StandardConfig`.
  Serde's `deny_unknown_fields` doesn't see through `flatten` (known
  limitation), so we implement the check ourselves in `Config::load()`.

Nested structs inside a flattened parent (e.g. `CcLibraryDef` inside
`CcManifest`) must use `deny_unknown_fields` — they don't flatten, so the
direct mechanism works.

The only legitimate exception: structs that *intentionally* capture unknown
fields (`ProcessorConfig.extra` for Lua plugins). These are rare and must
be documented at the field.

### Rule 2: every accepted field must be read

When you add a field to any config struct, add the engine code that consumes
it in the same change. Don't ship the schema first and the behaviour "soon."
If the field is a toggle, the runner must check it. If it's a path, something
must open or scan that path. If it's a value, a code path must branch on it.

When you add a CLI subcommand that writes a field (like `analyzers disable`
writing `enabled = false`), verify the runtime reads it by writing an
integration test that exercises the toggle end-to-end — config → build →
observable effect. A passing write-the-config test is not enough; the effect
must be asserted.

When you remove or rename a field, grep the codebase and docs to catch
stragglers. A field that exists in `defconfig_toml` but no longer affects
behaviour is a regression of Rule 2, even if no user reports it.

### When reviewing

Reject a patch that adds a new `Deserialize` struct without either
`deny_unknown_fields` or a `KnownFields` impl. Reject a patch that adds a
config field without the runtime code that reads it. Both failure modes
cost users time in exactly the same way — they write something sensible,
get no feedback, and conclude the tool is broken.

### Rule 3: validate before constructing

Schema validation must run inside `Config::load()`, before any processor or
analyzer is instantiated. `Builder::new()` should never be the first place
that surfaces an unknown-field or unknown-type error, because by the time
`Builder::new()` runs it has already opened `redb` databases, walked the
filesystem to build the `FileIndex`, and created CPU-bound infrastructure
the user doesn't need just to see "you typoed a field name."

The validators are `validate_processor_fields_raw` and
`validate_analyzer_fields_raw` in `src/config/mod.rs`. They return
`Vec<String>` so `Config::load()` can surface errors from both validators
together under a single `Invalid config:` header. If you add a new config
surface (a new top-level section with its own registered plugins), add a
matching validator and call it from `Config::load()` alongside the
existing two.

Unit-test the validators directly (see `src/config/tests.rs`) — not only
through `rsconstruct toml check`. Direct tests pin down the contract that
validation is a pure function of the parsed TOML, independent of
filesystem or plugin instantiation.

## No "latest" git tag

Never create a git tag named `latest`. Use only semver tags (e.g. `v0.3.0`).
A `latest` tag causes confusion with container registries and package managers
that use the word "latest" as a moving pointer, and it conflicts with GitHub's
release conventions.

## Book layout mirrors the filesystem

The book (`docs/src/`) is divided into two sections by `SUMMARY.md`:

1. A top-level user-facing section (introduction, commands, configuration,
   processors, etc.) — for people who use rsconstruct to build their projects.
2. A "For Maintainers" section — for contributors modifying rsconstruct
   itself: architecture, design decisions, coding standards, cache internals,
   and so on.

**The filesystem must mirror this split.** A reader glancing at a path
should be able to tell which audience the document is for:

- **User-facing chapters live at the top level of `docs/src/`** — e.g.
  `docs/src/configuration.md`, `docs/src/commands.md`.
- **Maintainer chapters live under `docs/src/internal/`** — e.g.
  `docs/src/internal/architecture.md`, `docs/src/internal/cache.md`.
- **Per-processor reference docs live under `docs/src/processors/`** —
  these are user-facing (they document how to configure each processor).

When adding a new doc, decide first whether it's user-facing or internal,
then place it accordingly. Moving a doc across the boundary requires
moving the file too — don't leave an internal document at the top level
just because its links would break.

When cross-referencing:

- Inside `internal/` → link to sibling files directly (``[X](other.md)``).
- From a top-level doc to an internal doc → ``[X](internal/other.md)``.
- From `processors/` to an internal doc → ``[X](../internal/other.md)``.
- From `internal/` to a user-facing doc → ``[X](../other.md)``.

This rule is enforced by convention, not by tooling. Reviewers should
reject PRs that add a maintainer-only document at the top level (or
vice versa).
