#!/bin/bash
# This repo's own CI installs: the system libraries the crate links against
# and the external tools its tests shell out to. The fleet-shared
# scripts/ci-install-tools.sh runs this in every rs* repo, in the test job
# (TARGET unset) and in the release build job (TARGET names the release
# triple), after it has put the rsconstruct release binary on PATH and before
# it installs the tools rsconstruct.toml declares. Keep it strict: anything
# that fails to install must fail the build here.
set -euo pipefail

# TARGET is set only by ci.yml's release build job, which runs nothing but
# `cargo build --release`. rsconstruct links no system library (mlua-sys and
# zstd-sys build from vendored sources; the macOS release jobs, which never
# run this script, prove the build needs nothing from it), so the release
# job has nothing to install. Installing the test matrix there anyway put
# every release behind ~10 downloads no step uses, and one of them,
# checkpatch.pl from raw.githubusercontent.com, failed a release on an
# anonymous-rate-limit 429 (run 37225847062).
if [[ -n "${TARGET:-}" ]]; then
	exit 0
fi

# This repo is rsconstruct, so the binary ci.yml's Build step runs is the
# one under test: built here from the checkout (the Build step's cargo
# creator reuses the same artifacts) and copied over the release binary the
# shared script just downloaded into cargo's bin dir. Building every commit
# with the previous release instead would leave a change this repo's own
# rsconstruct.toml needs failing CI until released.
cargo build
cp target/debug/rsconstruct "${CARGO_HOME:-${HOME}/.cargo}/bin/rsconstruct"

# The whole tool matrix, in one registry-driven command: every external tool
# the tests exercise comes from its own registry entry, so adding a
# processor never requires touching CI.
#
# Nothing is skipped and nothing is filtered — `--all` treats a tool it
# cannot install automatically as a hard error rather than a warning, so a
# registry entry that loses its install method fails this step instead of
# quietly shrinking the matrix. It is a superset of the `tool install` the
# shared script runs next, which then finds everything in place.
#
# drawio's .deb URL is resolved from the GitHub releases API at install time,
# so this inherits GITHUB_TOKEN from the workflow step to stay under the
# authenticated rate limit (see the comment on that step in ci.yml). Unset
# locally, where one machine never approaches the anonymous 60/hour.
rsconstruct tool install --all
