# Binary Releases

RSBuild publishes pre-built binaries as GitHub releases when a version tag
(`v*`) is pushed.

## Supported Platforms

| Platform | Binary name |
|---|---|
| Linux x86_64 | `rsbuild-linux-x86_64` |
| Linux aarch64 (arm64) | `rsbuild-linux-aarch64` |
| macOS x86_64 | `rsbuild-macos-x86_64` |
| macOS aarch64 (Apple Silicon) | `rsbuild-macos-aarch64` |
| Windows x86_64 | `rsbuild-windows-x86_64.exe` |

## How It Works

The release workflow (`.github/workflows/release.yml`) has two jobs:

1. **build** — a matrix job that builds the release binary for each platform
   and uploads it as a GitHub Actions artifact.
2. **release** — waits for all builds to finish, downloads the artifacts,
   and creates a GitHub release with auto-generated release notes and all
   binaries attached.

## Creating a Release

1. Update `version` in `Cargo.toml`
2. Commit and push
3. Tag and push: `git tag v0.2.2 && git push origin v0.2.2`
4. The workflow creates the GitHub release automatically

## Release Profile

The binary is optimized for size and performance:

```toml
[profile.release]
strip = true        # Remove debug symbols
lto = true          # Link-time optimization across all crates
codegen-units = 1   # Single codegen unit for better optimization
```
