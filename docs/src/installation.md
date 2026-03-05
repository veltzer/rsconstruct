# Installation

## Download pre-built binary (Linux)

Pre-built binaries are available for x86_64 and aarch64 (arm64).

Using the GitHub CLI:

```bash
# x86_64
gh release download latest --repo veltzer/rsbuild --pattern 'rsbuild-x86_64-unknown-linux-gnu' --output rsbuild --clobber

# aarch64 / arm64
gh release download latest --repo veltzer/rsbuild --pattern 'rsbuild-aarch64-unknown-linux-gnu' --output rsbuild --clobber

chmod +x rsbuild
sudo mv rsbuild /usr/local/bin/
```

Or with curl:

```bash
# x86_64
curl -Lo rsbuild https://github.com/veltzer/rsbuild/releases/download/latest/rsbuild-x86_64-unknown-linux-gnu

# aarch64 / arm64
curl -Lo rsbuild https://github.com/veltzer/rsbuild/releases/download/latest/rsbuild-aarch64-unknown-linux-gnu

chmod +x rsbuild
sudo mv rsbuild /usr/local/bin/
```

## Build from source

```bash
cargo build --release
```

The binary will be at `target/release/rsbuild`.

## Release profile

The release build is configured in `Cargo.toml` for maximum performance with a small binary:

```toml
[profile.release]
strip = true        # Remove debug symbols
lto = true          # Link-time optimization across all crates
codegen-units = 1   # Single codegen unit for better optimization
```

For an even smaller binary at the cost of some runtime speed, add `opt-level = "z"` (optimize for size) or `opt-level = "s"` (balance size and speed).
