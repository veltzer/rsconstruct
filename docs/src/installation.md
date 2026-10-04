# Installation

## Download pre-built binary (Linux)

Pre-built binaries are available for x86_64 and aarch64 (arm64).

Using the GitHub CLI:

```bash
# x86_64
gh release download --repo veltzer/rsconstruct --pattern 'rsconstruct-linux-x86_64' --output rsconstruct --clobber

# aarch64 / arm64
gh release download --repo veltzer/rsconstruct --pattern 'rsconstruct-linux-aarch64' --output rsconstruct --clobber

chmod +x rsconstruct
sudo mv rsconstruct /usr/local/bin/
```

Or with curl:

```bash
# x86_64
curl -Lo rsconstruct https://github.com/veltzer/rsconstruct/releases/latest/download/rsconstruct-linux-x86_64

# aarch64 / arm64
curl -Lo rsconstruct https://github.com/veltzer/rsconstruct/releases/latest/download/rsconstruct-linux-aarch64

chmod +x rsconstruct
sudo mv rsconstruct /usr/local/bin/
```

## Install from crates.io

```bash
cargo install rsconstruct
```

This downloads, compiles, and installs the latest published version into `~/.cargo/bin/`.

## Build from source

```bash
cargo build --release
```

The binary will be at `target/release/rsconstruct`.

## Release profile

The release build is configured in `Cargo.toml` for maximum performance with a small binary:

```toml
[profile.release]
strip = true        # Remove debug symbols
lto = true          # Link-time optimization across all crates
codegen-units = 1   # Single codegen unit for better optimization
```

For an even smaller binary at the cost of some runtime speed, add `opt-level = "z"` (optimize for size) or `opt-level = "s"` (balance size and speed).
