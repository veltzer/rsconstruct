# Iprotobuf Processor

## Purpose

Compiles Protocol Buffer definitions in-process, with [protox](https://github.com/andrewhickman/protox), a Rust implementation of the protobuf compiler, and generates Rust code with [prost](https://github.com/tokio-rs/prost). The Rust alternative to the [protobuf](protobuf.md) generator, which runs `protoc` and emits C++.

## How It Works

All the `.proto` files under `src_dirs` are compiled together as one product. `import` statements resolve against `include_paths` (the `src_dirs` by default); the well-known types (`google/protobuf/*.proto`) are built in. prost-build then writes one Rust file per protobuf package into the output directory, named after the package with each component in snake case, or `_.rs` for files without a `package` statement:

```text
proto/greet.proto   (package greet.v1)   →  out/processor.generator.iprotobuf/greet.v1.rs
proto/loose.proto   (no package)         →  out/processor.generator.iprotobuf/_.rs
```

The files of one package land in the same Rust file, which is why the stanza is one product: adding a message to any `.proto` regenerates the set. Well-known types map to the `prost-types` crate rather than being generated. With `descriptor_set` set, the compiled `FileDescriptorSet` is written there too, the file `protoc --include_imports --descriptor_set_out` produces (without source locations, as protoc writes it by default).

### Fidelity

The descriptor set protox compiles was compared with protoc 3.21's over proto2 and proto3 files covering nested and map fields, oneofs, enums with reserved ranges, groups, extensions, default values, field and method options, services with streaming methods, and imports of user files and well-known types: byte-identical. A compile error names the file, line and column and what was expected, as protoc does. Editions (`edition = "2023"`) are not supported by protox 0.10 (nor by protoc 3.21).

## Source Files

- Input: `.proto` files under `src_dirs`
- Output: `<package>.rs` per package under `output_dir`, plus `descriptor_set` when set

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` compiles nothing: name the directories.

## Configuration

```toml
[processor.generator.iprotobuf]
src_dirs = ["proto"]
include_paths = ["proto", "third_party/proto"]
descriptor_set = "out/descriptors.pb"
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `include_paths` | string[] | `[]` | Directories `import` statements resolve against. Empty: the `src_dirs` |
| `descriptor_set` | string | `""` | Where to write the compiled `FileDescriptorSet`. Empty: not written |
| `src_dirs` | string[] | `[]` | Directories to scan |
| `src_extensions` | string[] | `[".proto"]` | File extensions to compile |
| `output_dir` | string | `"out/processor.generator.iprotobuf"` | Where the Rust files go |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

Switching a repo from protobuf: `[processor.generator.protobuf]` → `[processor.generator.iprotobuf]`; the output changes from `<name>.pb.cc` per file to `<package>.rs` per package. A repo that needs C++ (or any language other than Rust) stays on protobuf.

## Batch support

Not applicable: the stanza's files are one product.

## Clean behavior

This processor is a Generator — `rsconstruct clean outputs` removes each declared output file individually with no directory recursion. After all per-product cleans complete, the orchestrator removes any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
