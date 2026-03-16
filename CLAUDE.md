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
- **Prefer plain code over macros** — use regular functions, generics, traits, and structs to eliminate duplication. Only resort to macros (`macro_rules!`) when the same result genuinely cannot be achieved with normal Rust abstractions (e.g., generating trait impls that vary by type, compile-time code generation). If a helper function or a new type can do the job, prefer that over a macro.
- **Cross-platform via `src/platform.rs`** — all OS-specific code (file permissions, signal handling) lives in `src/platform.rs` behind `#[cfg(unix)]` / `#[cfg(not(unix))]` guards. The rest of the codebase calls these wrappers and stays platform-agnostic. Do not add `#[cfg(...)]` blocks outside of `platform.rs`.
