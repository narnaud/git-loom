#!/usr/bin/env bash
# Code coverage over unit and integration tests together.
# Extra arguments go to `cargo llvm-cov report` (default: --html).
#   ./coverage.sh
#   ./coverage.sh --lcov --output-path lcov.info
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$REPO_ROOT"

missing=0
if ! cargo llvm-cov --version >/dev/null 2>&1; then
    missing=1
fi
if ! rustup component list --installed 2>/dev/null | grep -q '^llvm-tools'; then
    missing=1
fi
if [[ "$missing" == 1 ]]; then
    echo "cargo-llvm-cov and llvm-tools are required. Install them with:"
    echo
    echo "    rustup component add llvm-tools-preview"
    echo "    cargo install cargo-llvm-cov"
    exit 1
fi

# Instrumented builds get their own target dir: sharing target/debug would make
# this run and any normal `cargo build` keep rebuilding over each other.
export CARGO_TARGET_DIR="$REPO_ROOT/target/coverage"

# Instruments every build and every process it spawns, including the git-loom
# binary git runs as sequence editor and the one the shell tests drive.
eval "$(cargo llvm-cov show-env --export-prefix)"
cargo llvm-cov clean --workspace

cargo test

EXT=""
if [[ "$OSTYPE" == "msys" || "$OSTYPE" == "cygwin" ]]; then
    EXT=".exe"
fi
export GL_BIN="$CARGO_TARGET_DIR/debug/git-loom$EXT"
bash "$REPO_ROOT/tests/integration/run_all.sh"

if [[ $# -eq 0 ]]; then
    set -- --html
fi
cargo llvm-cov report "$@"
