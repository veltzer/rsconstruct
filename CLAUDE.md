# RSConstruct - Rust Build Tool

A fast, incremental build tool written in Rust with tera support, Python linting, and parallel execution.

Detailed documentation is in `docs/src/`. Key references:
- Commands: `docs/src/commands.md`
- Configuration: `docs/src/configuration.md`
- Architecture (subprocess execution, path handling, caching): `docs/src/architecture.md`
- Processor contract: `docs/src/processor-contract.md`
- Coding standards: `docs/src/coding-standards.md`
- Per-processor docs: `docs/src/processors/`

## Philosophy

- **Simplicity first** — keep the code simple whenever possible. Avoid clever solutions that are hard to understand or maintain. When in doubt, choose the straightforward approach.
- **Convention over configuration** — simple naming conventions, explicit config loading, incremental builds by default.
- **No macros** — the goal is zero `macro_rules!` in the codebase. Use regular functions, generics, traits, and structs to eliminate duplication. Every existing macro is a refactoring target to be replaced with plain Rust. Do not add new macros. Exception: the `ctx!` error context macro in `src/errors.rs` is allowed — it adds file:line to error messages which cannot be done with a regular function.
- **Cross-platform via `src/platform.rs`** — all OS-specific code (file permissions, signal handling) lives in `src/platform.rs` behind `#[cfg(unix)]` / `#[cfg(not(unix))]` guards. The rest of the codebase calls these wrappers and stays platform-agnostic. Do not add `#[cfg(...)]` blocks outside of `platform.rs`.
- **Strict by default** — never silently skip errors or ignore failures. Non-strict systems hide problems and are a disaster. If a tool is missing, fail. If a test fails, fix it before moving on.
- **All tests must pass** — always run `cargo test` with no filters or skips. Do not move forward with any failing test. If a test fails, fix it immediately — the failure is real.
- **No scripts to modify code** — never use Python scripts, sed, awk, or any external tool to modify Rust source code. All code changes must be made manually through the editor. Automated bulk changes produce inconsistent results and hide mistakes.
- **Always add context to errors** — every `?` on an IO operation (`fs::read`, `fs::write`, `Command::spawn`, `fs::create_dir_all`, etc.) must have `.with_context(|| format!("..."))` that says what you were trying to do and which file/command was involved. A bare `?` on an IO operation is a bug — it produces useless error messages like "No such file or directory" with no indication of what went wrong. Use `anyhow::Context` everywhere.
- **Never create dummy instances** — never instantiate a processor (or any object) just to inspect its metadata. Metadata (config fields, defaults, descriptions) must be available without creating an instance. If you need config info, get it from the plugin interface, not from a throwaway instance.
- **CLI subcommands are always alphabetical** — every `#[derive(Subcommand)] enum` in `src/cli.rs` (top-level `Commands` and every `*Action` enum) must list its variants in alphabetical order by display name (clap's kebab-case conversion of the variant — e.g. `EnableDetected` → `enable-detected`). Clap renders subcommands in declaration order, so this list IS the help output. When adding a new variant, insert it at its alphabetical position. No exceptions.
