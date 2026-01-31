# Installation

## Download pre-built binary (x86_64 Linux)

Using the GitHub CLI:

```bash
gh release download latest --repo veltzer/rsb --pattern 'rsb' --output rsb --clobber
chmod +x rsb
sudo mv rsb /usr/local/bin/
```

Or with curl:

```bash
curl -Lo rsb https://github.com/veltzer/rsb/releases/download/latest/rsb
chmod +x rsb
sudo mv rsb /usr/local/bin/
```

## Build from source

```bash
cargo build --release
```

The binary will be at `target/release/rsb`.

## Release profile

The release build is configured in `Cargo.toml` for maximum performance with a small binary:

```toml
[profile.release]
strip = true        # Remove debug symbols
lto = true          # Link-time optimization across all crates
codegen-units = 1   # Single codegen unit for better optimization
```

For an even smaller binary at the cost of some runtime speed, add `opt-level = "z"` (optimize for size) or `opt-level = "s"` (balance size and speed).
