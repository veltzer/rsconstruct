# Binary Releases

RSBuild publishes pre-built binaries as GitHub releases on every push to master.

## Supported Architectures

| Architecture | Binary name | GitHub runner |
|---|---|---|
| x86_64 | `rsbuild-x86_64-unknown-linux-gnu` | `ubuntu-latest` |
| aarch64 (arm64) | `rsbuild-aarch64-unknown-linux-gnu` | `ubuntu-24.04-arm` |

Both binaries are built natively (no cross-compilation) and tests run on the
actual target architecture before the release is published.

## How It Works

The release workflow (`.github/workflows/release.yml`) has two jobs:

1. **build** — a matrix job that runs in parallel on x86_64 and arm64
   runners. Each job builds the release binary, runs all tests, and uploads
   the binary as a GitHub Actions artifact.
2. **release** — waits for both builds to finish, downloads the artifacts,
   and creates a single GitHub release tagged `latest` with both binaries
   attached.

The release is a rolling prerelease — each push to master replaces the
previous `latest` release.

## Release Profile

The binary is optimized for size and performance:

```toml
[profile.release]
strip = true        # Remove debug symbols
lto = true          # Link-time optimization across all crates
codegen-units = 1   # Single codegen unit for better optimization
```

## Known Issues

### No `ubuntu-latest-arm` runner

GitHub provides `ubuntu-latest` for x86_64, which automatically rolls
forward to the newest Ubuntu version. There is no equivalent
`ubuntu-latest-arm` for arm64. The arm64 runner must be pinned to a
specific version (`ubuntu-24.04-arm`).

This means the arm64 runner version needs to be updated manually in
`.github/workflows/release.yml` when GitHub releases newer arm64 runner
images (e.g., `ubuntu-26.04-arm`). There is an
[open community discussion](https://github.com/orgs/community/discussions/149392)
requesting that GitHub add `ubuntu-latest-arm`.

### Linux only

Only Linux binaries are published. macOS and Windows users must build from
source with `cargo build --release`.
