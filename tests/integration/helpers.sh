#!/usr/bin/env bash
# Shared helpers for git-loom integration tests.
# Source this file at the top of each test_*.sh script.

set -euo pipefail

# ── Binary detection ──────────────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

if [[ -z "${GL_BIN:-}" ]]; then
    if [[ "$OSTYPE" == "msys" || "$OSTYPE" == "cygwin" ]]; then
        GL_BIN="$REPO_ROOT/target/debug/git-loom.exe"
    else
        GL_BIN="$REPO_ROOT/target/debug/git-loom"
    fi
fi

if [[ ! -x "$GL_BIN" ]]; then
    echo "[NOK] $(basename "$0" .sh): binary not found at $GL_BIN — run 'cargo build' first"
    exit 1
fi

# ── Colors ────────────────────────────────────────────────────────────────
if [[ -t 1 ]]; then
    RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[0;33m'; NC='\033[0m'
else
    RED=''; GREEN=''; YELLOW=''; NC=''
fi

# ── Global state (set by setup helpers, cleaned up by trap in test) ───────
TMPROOT=""
WORK=""

# ── Repo setup helpers ────────────────────────────────────────────────────

# Create a repo with a bare remote.
# Sets WORK (integration branch, tracking origin/<default-branch>).
setup_repo_with_remote() {
    TMPROOT="$(mktemp -d)"

    # Build a seed repo with an initial commit
    local seed="$TMPROOT/seed"
    git init -q "$seed"
    git -C "$seed" config user.email "test@test.com"
    git -C "$seed" config user.name "Test"
    git -C "$seed" config core.autocrlf false
    touch "$seed/.gitkeep"
    git -C "$seed" add .
    git -C "$seed" commit -q -m "Initial"
    local base_branch
    base_branch="$(git -C "$seed" rev-parse --abbrev-ref HEAD)"

    # Clone it as the bare remote
    git clone -q --bare "$seed" "$TMPROOT/remote.git"
    rm -rf "$seed"

    # Working clone
    WORK="$TMPROOT/work"
    git clone -q "$TMPROOT/remote.git" "$WORK"
    git -C "$WORK" config user.email "test@test.com"
    git -C "$WORK" config user.name "Test"
    git -C "$WORK" config core.autocrlf false
    # Prevent git from opening an interactive editor in tests (e.g. for
    # `git merge --continue` which is equivalent to `git commit`).
    git -C "$WORK" config core.editor "true"

    # Integration branch tracking origin/<default>
    git -C "$WORK" checkout -q -b integration
    git -C "$WORK" branch --set-upstream-to="origin/$base_branch" integration > /dev/null 2>&1
}

# Commit a single file in $WORK.
# Usage: commit_file "commit message" "filename.txt"
commit_file() {
    local msg="$1" file="$2"
    echo "$msg" > "$WORK/$file"
    git -C "$WORK" add "$file"
    git -C "$WORK" commit -q -m "$msg"
}

# Commit a single file in an arbitrary git repo (not necessarily $WORK).
# Usage: commit_file_in REPO "commit message" "filename.txt"
commit_file_in() {
    local repo="$1" msg="$2" file="$3"
    echo "$msg" > "$repo/$file"
    git -C "$repo" add "$file"
    git -C "$repo" commit -q -m "$msg"
}

# Create a feature branch at the current remote base (no commits on it yet).
# Usage: create_feature_branch "feature-a"
create_feature_branch() {
    local name="$1"
    local base_oid
    base_oid="$(git -C "$WORK" rev-parse "$(git -C "$WORK" rev-parse --abbrev-ref --symbolic-full-name @{u})")"
    git -C "$WORK" branch "$name" "$base_oid"
}

# Weave a branch into integration via --no-ff merge.
# Usage: weave_branch "feature-a"
weave_branch() {
    local name="$1"
    git -C "$WORK" merge -q --no-ff "$name" -m "Merge $name"
}

# ── Binary invocation ─────────────────────────────────────────────────────

# Run git-loom from $WORK with color and terminal prompts disabled.
gl() {
    (cd "$WORK" && NO_COLOR=1 GIT_TERMINAL_PROMPT=0 "$GL_BIN" "$@")
}

# ── Action helpers ────────────────────────────────────────────────────────

# Checkout a branch quietly.
switch_to() { git -C "$WORK" checkout -q "$1"; }

# Write a file in $WORK without committing.
write_file() { echo "$2" > "$WORK/$1"; }

# Run gl, capturing stdout+stderr into OUT and exit code into CODE.
# Prevents set -e from aborting the script on failure.
gl_capture() { OUT=$(gl "$@" 2>&1) && CODE=$? || CODE=$?; }

# ── Git query helpers ─────────────────────────────────────────────────────

head_hash()         { git -C "$WORK" rev-parse HEAD; }
head_msg()          { git -C "$WORK" log -1 --pretty=%s; }
msg_at()            { git -C "$WORK" log -1 --skip="$1" --pretty=%s; }  # 0=HEAD, 1=HEAD~1, …
upstream_oid()      { git -C "$WORK" rev-parse "$(git -C "$WORK" rev-parse --abbrev-ref --symbolic-full-name @{u})"; }
branch_oid()        { git -C "$WORK" rev-parse "$1"; }
log_oneline()       { git -C "$WORK" log --oneline; }
head_parent_count()      { git -C "$WORK" log -1         --pretty=%P | wc -w | tr -d ' '; }
parent_count_at()        { git -C "$WORK" log -1 "$1"   --pretty=%P | wc -w | tr -d ' '; }

# Return the short ID for a commit, given its message as shown in gl status.
# Usage: commit_sid=$(commit_sid_from_status "Commit message")
commit_sid_from_status() { gl status | grep "$1" | grep -oE '[0-9a-z]{4,8}' | head -1; }

# Return the short ID for a branch, given its name as shown in gl status.
# Usage: branch_sid=$(branch_sid_from_status "branch-name")
branch_sid_from_status() { gl status | grep -F "[$1]" | awk '{print $(NF-1)}'; }

# ── Assertions ────────────────────────────────────────────────────────────

assert_contains() {
    local output="$1" needle="$2" label="${3:-}"
    grep -qF -- "$needle" <<< "$output" \
        || fail "${label:+[$label] }expected output to contain '$needle'"
}

assert_not_contains() {
    local output="$1" needle="$2" label="${3:-}"
    ! grep -qF -- "$needle" <<< "$output" \
        || fail "${label:+[$label] }expected output NOT to contain '$needle'"
}

assert_exit_ok() {
    local code="$1" label="${2:-}"
    [[ "$code" -eq 0 ]] || fail "${label:+[$label] }expected exit 0, got $code"
}

assert_exit_fail() {
    local code="$1" label="${2:-}"
    [[ "$code" -ne 0 ]] || fail "${label:+[$label] }expected non-zero exit, got 0"
}

assert_head_msg() {
    local expected="$1" label="${2:-}"
    local actual; actual="$(head_msg)"
    [[ "$actual" == "$expected" ]] \
        || fail "${label:+[$label] }HEAD message: expected '$expected', got '$actual'"
}

assert_msg_at() {
    local steps="$1" expected="$2" label="${3:-}"
    local actual; actual="$(msg_at "$steps")"
    [[ "$actual" == "$expected" ]] \
        || fail "${label:+[$label] }message at ~$steps: expected '$expected', got '$actual'"
}

assert_state_file() {
    local label="${1:-}"
    [[ -f "$WORK/.git/loom/state.json" ]] \
        || fail "${label:+[$label] }state file should exist"
}

assert_no_state_file() {
    local label="${1:-}"
    [[ ! -f "$WORK/.git/loom/state.json" ]] \
        || fail "${label:+[$label] }state file should not exist"
}

assert_branch_exists() {
    local name="$1" label="${2:-}"
    git -C "$WORK" rev-parse --verify "refs/heads/$name" > /dev/null 2>&1 \
        || fail "${label:+[$label] }branch '$name' does not exist"
}

assert_branch_not_exists() {
    local name="$1" label="${2:-}"
    ! git -C "$WORK" rev-parse --verify "refs/heads/$name" > /dev/null 2>&1 \
        || fail "${label:+[$label] }branch '$name' should not exist"
}

assert_file_content() {
    local file="$1" expected="$2" label="${3:-}"
    local actual; actual="$(cat "$WORK/$file")"
    [[ "$actual" == "$expected" ]] \
        || fail "${label:+[$label] }$file: expected '$expected', got '$actual'"
}

assert_commit_not_in_log() {
    local hash="$1" label="${2:-}"
    ! git -C "$WORK" log --pretty=%H | grep -qF "$hash" \
        || fail "${label:+[$label] }commit $hash should have been dropped"
}

assert_eq() {
    local a="$1" b="$2" label="${3:-}"
    [[ "$a" == "$b" ]] || fail "${label:+[$label] }expected '$a' == '$b'"
}

assert_ne() {
    local a="$1" b="$2" label="${3:-}"
    [[ "$a" != "$b" ]] || fail "${label:+[$label] }expected '$a' != '$b'"
}

assert_log_contains() {
    local needle="$1" label="${2:-}"
    assert_contains "$(log_oneline)" "$needle" "$label"
}

assert_log_not_contains() {
    local needle="$1" label="${2:-}"
    assert_not_contains "$(log_oneline)" "$needle" "$label"
}

assert_head_parent_count() {
    local expected="$1" label="${2:-}"
    local actual; actual="$(head_parent_count)"
    [[ "$actual" -eq "$expected" ]] \
        || fail "${label:+[$label] }expected $expected parent(s), got $actual"
}

# ── Pass / fail / describe ────────────────────────────────────────────────

describe() { printf " ${YELLOW}=>${NC} %s\n" "$*"; }

pass() {
    printf "${GREEN}[OK]${NC}  %s\n" "$(basename "$0" .sh)"
    exit 0
}

fail() {
    printf "${RED}[NOK]${NC} %s: %s\n" "$(basename "$0" .sh)" "$*"
    exit 1
}
