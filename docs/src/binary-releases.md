# Binary Releases

RSConstruct publishes pre-built binaries as GitHub releases. A release is made
with `cargo release`; CI builds and publishes the binaries from the release
commit it pushes.

## Supported Platforms

| Platform | Binary name |
|---|---|
| Linux x86_64 | `rsconstruct-linux-x86_64` |
| Linux aarch64 (arm64) | `rsconstruct-linux-aarch64` |
| macOS x86_64 | `rsconstruct-macos-x86_64` |
| macOS aarch64 (Apple Silicon) | `rsconstruct-macos-aarch64` |

RSConstruct is unix-only. There is no Windows build: the release matrix in
`.github/workflows/ci.yml` has never contained a Windows target, and the
codebase assumes unix throughout (`flock`, `/dev/null`, `$HOME`, apt-based
tool installation).

## How It Works

Everything runs from the single CI workflow (`.github/workflows/ci.yml`).
The workflow triggers on branch pushes only. Every push runs the **test**
job; a push to the default branch whose commit message starts with
`chore: Release ` (the commit `cargo release` makes) also runs three release
jobs:

1. **build** — a matrix job that builds the release binary for each platform
   and uploads it as a GitHub Actions artifact. It runs only after the
   **test** job passes, so a release never ships from a commit whose tests
   fail.
2. **release** — waits for all builds to finish, downloads the binary
   artifacts, and creates a GitHub release named after the version in
   `Cargo.toml` (`v<version>`), with auto-generated release notes and all
   binaries attached.
3. **docs** — builds the mdBook documentation and deploys it to GitHub
   Pages.

Pushing a tag on its own does nothing: `cargo release` pushes the tag
together with the release commit, and the workflow ignores tag pushes so
the two do not start two identical runs.

## Creating a Release

```bash
cargo release patch            # dry run: shows what would happen
cargo release patch --execute  # bump, commit, tag, publish, push
```

Use `minor` or `major` instead of `patch` for a bigger bump. With
`--execute`, `cargo release` (configured in `release.toml`):

1. bumps `version` in `Cargo.toml`,
2. commits it as `chore: Release rsconstruct version <version>`,
3. creates a signed tag `v<version>`,
4. publishes the crate to crates.io,
5. pushes the commit and the tag.

The push of that commit to the default branch starts the release jobs
above. Bumping the version and tagging by hand does not produce a release,
because the commit message would not start with `chore: Release `.

## Release Profile

The binary is optimized for size and performance:

```toml
[profile.release]
strip = true        # Remove debug symbols
lto = true          # Link-time optimization across all crates
codegen-units = 1   # Single codegen unit for better optimization
```
