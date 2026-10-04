# Nomenclature

This page defines the terminology used throughout RSConstruct's code, configuration, CLI, and documentation.

## Core concepts

| Term | Definition |
|---|---|
| **pname** | Processor name: the processor's full path, `processor.<type>.<name>`, identical to its module path in the source tree (`processor.checker.ruff` is `src/processor/checker/ruff.rs`). Unique across all plugins because the type is part of it; the last segment alone is not (`processor.creator.generic` and `processor.explicit.generic` are different processors). Used as the `[processor.TYPE.NAME]` config section header and in `processor defconfig PNAME`. |
| **iname** | Instance name. The name of a specific processor instance as declared in `rsconstruct.toml`. For a single instance, the iname equals the pname (`[processor.checker.ruff]` → `processor.checker.ruff`). For a named instance, the iname is the pname plus the sub-key (`[processor.creator.generic.venv]` → `processor.creator.generic.venv`). Used in `processor config INAME`, `-p`, `--iset`, build output and cache keys. |
| **processor** | A configured instance that discovers products and executes builds. Created from a plugin + TOML config. Immutable after creation. |
| **plugin** | A factory registered at compile time via `inventory::submit!`. Knows how to create processors from TOML config. Has a pname, a processor type, and config metadata. |
| **native processor** | A processor implemented in pure Rust inside rsconstruct; it needs no external tool. Examples: `tera`, `ijsonlint`, `encoding`. Declared with `is_native: true` on the plugin. |
| **external processor** | A processor that runs another program to do its work. Examples: `ruff`, `pylint`, `tidy`. |
| **rust** (processor flag) | Whether the code that does the work is written in Rust: true for every native processor, and for an external one only when its tool is written in Rust (`ruff`, `taplo`, `rumdl`, `clippy`, `cargo`, `mdbook`, `pyrefly`, `rustc`). Declared with `is_rust` on the plugin; shown by `processor list` and `status`. Processors that run a user-supplied command report false. |
| **product** | A single build unit with inputs, outputs, and a processor. The atomic unit of incremental building. |
| **processor type** | One of five built-in categories — `checker`, `generator`, `creator`, `explicit`, `mass_generator` — plus `lua` for user plugins. The second segment of every pname. Determines how inputs are discovered, how outputs are declared, and how results are cached. See [Processor Types](processor-types.md). |
| **analyzer** | A dependency scanner that runs after product discovery to add extra input edges to existing products (e.g., the `cpp` analyzer adds every `#include`d header as an extra input of a C/C++ product). Analyzers never create products of their own. Declared with `[analyzer.NAME]` sections in `rsconstruct.toml`. Unlike processors, only analyzers explicitly declared in config run — there is no "auto-enable" default. See [Dependency Analyzers](analyzers.md). |
| **analyzer plugin** | A factory registered at compile time via `inventory::submit!` in the analyzer registry. Knows how to construct an analyzer from its `[analyzer.NAME]` TOML section. Each plugin declares its name, description, and whether it is `native` (pure Rust) or external (may invoke subprocesses). |
| **native analyzer** | An analyzer whose default configuration runs entirely in-process (no subprocesses). Example: `icpp` uses a pure-Rust regex scanner for `#include` directives. Some native analyzers become external in non-default configurations (e.g., `icpp` with `pkg_config` set shells out to `pkg-config` for include paths). |
| **external analyzer** | An analyzer that shells out to another program to do its work. Example: `cpp` always runs `gcc -MM` for exact compiler-accurate header scanning. |

## Configuration

| Term | Definition |
|---|---|
| **output_files** | List of individual output files declared in creator/explicit config. Cached as blobs. |
| **output_dirs** | List of output directories declared in creator/explicit config. All files inside are walked and cached as a tree. |
| **src_dirs** | Directories to scan for input files. |
| **src_extensions** | File extensions to match during scanning. |
| **dep_inputs** | Extra files that trigger a rebuild when their content changes. |
| **dep_auto** | Config files added as dep_inputs (e.g., `.eslintrc`). Processor defaults are skipped when absent; entries listed in the config must exist unless `[build] allow_missing_dep_auto` is set. |

## Cache

| Term | Definition |
|---|---|
| **blob** | A file's raw content stored in the object store, addressed by SHA-256 hash. Blobs have no path — the consumer knows where to restore them. |
| **tree** | A serialized list of `(path, mode, blob_checksum)` entries describing a set of output files. Stored in the descriptor store. |
| **marker** | A zero-byte descriptor indicating a checker passed. Its presence is the cached result. |
| **descriptor** | A cache entry (blob reference, tree, or marker) stored in `.rsconstruct/descriptors/`, keyed by the descriptor key. |
| **descriptor key** | A content-addressed hash of `(pname, config_hash, variant, input_checksum)`. Changes when processor config or input content changes. Does NOT include file paths — renaming a file with identical content produces the same key. |
| **input checksum** | Combined SHA-256 hash of all input file contents for a product. |

## Build pipeline

| Term | Definition |
|---|---|
| **discover** | Phase where processors scan the file index and register products in the build graph. |
| **classify** | Phase where each product is classified as skip, restore, or build based on its cache state. |
| **execute** | Phase where products are built in dependency order. |
| **anchor file** | A file whose presence triggers a creator processor to run (e.g., `Cargo.toml` for cargo, `requirements.txt` for pip). |

## CLI conventions

| Command | Name parameter | Meaning |
|---|---|---|
| `processor defconfig PNAME` | pname | Processor type name — shows factory defaults |
| `processor config [INAME]` | iname | Instance name from config — shows resolved config |
| `processor files [INAME]` | iname | Instance name from config — shows discovered files |
| `analyzers defconfig [NAME]` | analyzer name | Analyzer name from the analyzer registry — shows factory defaults |
| `analyzers config [NAME]` | analyzer name | Analyzer name as declared in `[analyzer.NAME]` — shows resolved config |
