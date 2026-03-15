#!/usr/bin/env bash
# Integration tests for: gl init
set -euo pipefail
source "$(dirname "$0")/helpers.sh"
trap 'rm -rf "$TMPROOT"' EXIT

# Helper: repo on the default branch (main/master) tracking origin.
# No integration branch is pre-created, so gl init can run cleanly.
setup_repo_for_init() {
    TMPROOT="$(mktemp -d)"

    local seed="$TMPROOT/seed"
    git init -q "$seed"
    git -C "$seed" config user.email "test@test.com"
    git -C "$seed" config user.name "Test"
    git -C "$seed" config core.autocrlf false
    touch "$seed/.gitkeep"
    git -C "$seed" add .
    git -C "$seed" commit -q -m "Initial"

    git clone -q --bare "$seed" "$TMPROOT/remote.git"
    rm -rf "$seed"

    WORK="$TMPROOT/work"
    git clone -q "$TMPROOT/remote.git" "$WORK"
    git -C "$WORK" config user.email "test@test.com"
    git -C "$WORK" config user.name "Test"
    git -C "$WORK" config core.autocrlf false
    # HEAD stays on the default branch (main/master) with origin tracking set up
}

# Helper: return the name of the current branch
current_branch() { git -C "$WORK" rev-parse --abbrev-ref HEAD; }

# Helper: return the upstream tracking ref for a branch (e.g. "origin/main")
branch_upstream() { git -C "$WORK" rev-parse --abbrev-ref --symbolic-full-name "$1@{u}" 2>/dev/null; }

# Helper: return the default base branch name (main or master)
default_base_branch() { git -C "$WORK" symbolic-ref refs/remotes/origin/HEAD | sed 's|refs/remotes/origin/||'; }

describe "default name creates integration branch"
setup_repo_for_init
gl_capture init
assert_exit_ok "$CODE" "default_name"
assert_branch_exists "integration" "default_name"
assert_eq "$(current_branch)" "integration" "default_name_head_switched"

describe "custom name creates branch with that name"
setup_repo_for_init
gl_capture init my-integration
assert_exit_ok "$CODE" "custom_name"
assert_branch_exists "my-integration" "custom_name"
assert_eq "$(current_branch)" "my-integration" "custom_name_head_switched"

describe "success message mentions branch name and tracking"
setup_repo_for_init
gl_capture init
assert_contains "$OUT" "Initialized integration branch" "success_msg_prefix"
assert_contains "$OUT" "integration" "success_msg_branch_name"
assert_contains "$OUT" "tracking" "success_msg_tracking"
assert_contains "$OUT" "origin/" "success_msg_upstream"

describe "new branch tracks the detected upstream"
setup_repo_for_init
gl init
upstream="$(branch_upstream integration)"
assert_contains "$upstream" "origin/" "upstream_tracking_configured"

describe "error when branch name already exists"
setup_repo_for_init
gl init  # first run succeeds; HEAD is now on 'integration'
base="$(default_base_branch)"
git -C "$WORK" checkout -q "$base"
gl_capture init  # second run must fail
assert_exit_fail "$CODE" "duplicate_branch"
assert_contains "$OUT" "integration" "duplicate_branch_msg"

describe "error when no remotes are configured"
TMPROOT="$(mktemp -d)"
WORK="$TMPROOT/work"
git init -q "$WORK"
git -C "$WORK" config user.email "test@test.com"
git -C "$WORK" config user.name "Test"
touch "$WORK/.gitkeep"
git -C "$WORK" add .
git -C "$WORK" commit -q -m "Initial"
gl_capture init
assert_exit_fail "$CODE" "no_remote"
assert_contains "$OUT" "remote" "no_remote_msg"

describe "error when branch name is invalid (git naming rules)"
setup_repo_for_init
gl_capture init ".invalid..name"
assert_exit_fail "$CODE" "invalid_name"

describe "works on detached HEAD — falls back to remote scan"
setup_repo_for_init
git -C "$WORK" checkout -q --detach HEAD
gl_capture init
assert_exit_ok "$CODE" "detached_head"
assert_branch_exists "integration" "detached_head_branch"
assert_eq "$(current_branch)" "integration" "detached_head_head_switched"

describe "works on a branch with no upstream — falls back to remote scan"
setup_repo_for_init
git -C "$WORK" checkout -q -b no-upstream
gl_capture init
assert_exit_ok "$CODE" "no_upstream_tracking"
assert_branch_exists "integration" "no_upstream_tracking_branch"

pass
