# Plugin Registry: Ecosystem Survey

rsconstruct uses a hand-built plugin registry where processors self-register
at link time via `inventory::submit!`, declare their config schema, and are
instantiated from TOML config at runtime. This page documents the search for
existing Rust crates that could replace this machinery.

## What rsconstruct needs

The plugin system combines four responsibilities:

1. **Link-time self-registration** — each processor file submits a plugin
   entry. No central list to maintain. Adding a processor = adding one file.
2. **Per-plugin TOML config** — each plugin declares known fields, required
   fields, defaults, and a `create(toml::Value) -> Box<dyn Processor>`
   factory. The framework deserializes the matching `[processor.TYPE.NAME]`
   section and passes it to the factory.
3. **Defaults and validation** — processor defaults, scan defaults, and
   output-dir defaults are applied in layers before deserialization. Unknown
   fields are rejected. Required fields are enforced.
4. **Name-to-factory mapping** — the registry maps processor names to their
   plugin entries for creation, introspection (`processor list`), and
   config display.

## Crates evaluated

### inventory / linkme

The foundation rsconstruct already uses. `inventory` provides link-time
collection of typed values into a global iterator. `linkme` does the same
via distributed slices. Neither has any config awareness — they solve (1)
only.

- **Verdict:** already in use; does its job well.

### spring-rs

The closest match conceptually. A Spring Boot-style Rust framework that
combines `inventory`-based plugin registration with TOML config via
`#[derive(Configurable)]` and `#[config_prefix = "..."]` attributes. Each
plugin declares its config struct with the derive macro, and the framework
auto-deserializes the matching TOML section.

However, spring-rs is a **full application framework** for web services
(integrates axum, sqlx, OpenTelemetry, etc.). Pulling it in for a build
tool would add a massive, opinionated dependency tree for ~50 lines of
glue code savings.

- **Verdict:** right pattern, wrong scope. Not suitable.

### config (crate)

Handles layered config loading from multiple sources (TOML, YAML, JSON,
env vars) with type-safe deserialization. No plugin registration awareness
at all — it's a config library, not a plugin framework.

- **Verdict:** solves config layering, not plugin registration.

### extism

A WebAssembly plugin runtime. Plugins are compiled to WASM and loaded at
runtime with sandboxing. Completely different problem — runtime-loaded
external plugins vs. compile-time self-registering internal plugins.

- **Verdict:** wrong problem domain.

### plugin-interfaces

Designed for chat-client applications with FFI and inter-plugin messaging.
Not relevant to build tools.

- **Verdict:** not applicable.

### toml-cfg

Provides compile-time config macros (`#[toml_cfg::toml_config]`) that
embed config values from a TOML file at build time. No runtime registry,
no plugin awareness.

- **Verdict:** compile-time only; not what we need.

## Conclusion

No existing crate provides the combination of link-time registration +
per-plugin TOML config deserialization + defaults/validation + name-to-factory
mapping. This is a genuine gap in the Rust ecosystem.

rsconstruct's manual approach (~50 lines of glue in `src/registries/processor.rs`
using `inventory::submit!` + serde + the `ProcessorPlugin` struct) is the
standard Rust pattern for this. It is well-understood, has no external
framework dependency, and is unlikely to be improved upon by a third-party
crate without bringing in unrelated complexity.

**Decision:** keep the current hand-built registry. Revisit if a focused
plugin-config crate emerges in the ecosystem.

*Survey conducted: April 2026.*
