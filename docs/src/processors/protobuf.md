# Protobuf Processor

## Purpose

Compiles Protocol Buffer (`.proto`) files to generated source code using `protoc`.

## How It Works

Files matching configured extensions in the `proto/` directory are compiled using the
Protocol Buffer compiler. Output is written to `out/protobuf/`:

```
proto/hello.proto  →  out/protobuf/hello.pb.cc
```

The `--proto_path` is automatically set to the parent directory of each input file.

## Source Files

- Input: `proto/**/*.proto`
- Output: `out/protobuf/` with `.pb.cc` extension

## Configuration

```toml
[processor.protobuf]
protoc_bin = "protoc"                     # Protoc binary (default: "protoc")
extensions = [".proto"]                   # File extensions to process
output_dir = "out/protobuf"              # Output directory (default: "out/protobuf")
extra_inputs = []                         # Additional files that trigger rebuilds
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `protoc_bin` | string | `"protoc"` | Path to protoc compiler |
| `extensions` | string[] | `[".proto"]` | File extensions to discover |
| `output_dir` | string | `"out/protobuf"` | Output directory |
| `extra_inputs` | string[] | `[]` | Extra files whose changes trigger rebuilds |
