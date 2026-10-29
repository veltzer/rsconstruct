# Protobuf Processor

## Purpose

Compiles Protocol Buffer (`.proto`) files to generated source code using `protoc`.

The Rust alternative is [iprotobuf](iprotobuf.md): the files compiled in-process with protox (descriptors byte-identical to protoc's) and Rust code generated with prost, one file per package. It emits Rust only; a repo that needs the C++ output stays here.

## How It Works

Files matching configured extensions in the `proto/` directory are compiled using the
Protocol Buffer compiler. Output is written to `out/processor.generator.protobuf/`:

```text
proto/hello.proto  →  out/processor.generator.protobuf/hello.pb.cc
```

The `--proto_path` is automatically set to the parent directory of each input file.

## Source Files

- Input: `proto/**/*.proto`
- Output: `out/processor.generator.protobuf/` with `.pb.cc` extension

## Configuration

```toml
[processor.generator.protobuf]
command = "protoc"                        # Protoc binary (default: "protoc")
src_extensions = [".proto"]                   # File extensions to process
output_dir = "out/processor.generator.protobuf"              # Output directory (default: "out/processor.generator.protobuf")
dep_inputs = []                         # Additional files that trigger rebuilds
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `command` | string | `"protoc"` | Path to protoc compiler |
| `src_extensions` | string[] | `[".proto"]` | File extensions to discover |
| `output_dir` | string | `"out/processor.generator.protobuf"` | Output directory |
| `dep_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |

## Installation

`rsconstruct tool install project` downloads the protoc release from the
protobuf GitHub releases into `.rsconstruct/tools`, at the version
rsconstruct.lock pins. The release is kept whole: protoc finds the
well-known types (`google/protobuf/timestamp.proto` and the rest) in the
`include/` beside its `bin/`. Code it generates checks the protobuf runtime
it is compiled or imported against, so the pinned protoc is bumped
deliberately. `rsconstruct tool install machine protoc` installs the
distribution's `protobuf-compiler` package with apt instead.

## Batch support

Each input file is processed individually, producing its own output file.

## Clean behavior

This processor is a Generator — `rsconstruct clean outputs` removes each declared output file individually with no directory recursion. After all per-product cleans complete, the orchestrator removes any parent directories that are now empty. Pass `--no-empty-dirs` to keep them. See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
