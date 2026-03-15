#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")" && pwd)"
PROFILE="${1:-debug}"

case "$PROFILE" in
    debug)   cargo build ;;
    release) cargo build --release ;;
    *)
        echo "Usage: $0 [debug|release]"
        exit 1
        ;;
esac

# Append .exe on Windows (Git Bash reports msys or cygwin)
EXT=""
if [[ "$OSTYPE" == "msys" || "$OSTYPE" == "cygwin" ]]; then
    EXT=".exe"
fi

export GL_BIN="$REPO_ROOT/target/$PROFILE/git-loom$EXT"
bash "$REPO_ROOT/tests/integration/run_all.sh"
