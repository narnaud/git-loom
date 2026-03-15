#!/usr/bin/env bash
# Integration tests for: gl drop
set -euo pipefail
source "$(dirname "$0")/helpers.sh"
trap 'rm -rf "$TMPROOT"' EXIT

# ── Test: drop HEAD commit removes it ────────────────────────────────────
setup_repo_with_remote
commit_file "Keep this" "keep.txt"
commit_file "Drop this" "drop.txt"
drop_hash="$(head_hash)"

gl drop "$drop_hash" --yes
assert_commit_not_in_log "$drop_hash" "drop_head"
assert_head_msg "Keep this" "drop_head"

# ── Test: drop a non-HEAD commit (middle of stack) ────────────────────────
setup_repo_with_remote
commit_file "First"  "first.txt"
drop_hash="$(head_hash)"
commit_file "Second" "second.txt"
commit_file "Third"  "third.txt"

gl drop "$drop_hash" --yes
assert_commit_not_in_log "$drop_hash" "drop_middle"
assert_head_msg "Third"  "drop_middle"
assert_msg_at 1 "Second" "drop_middle"

# ── Test: drop a commit on a woven feature branch ────────────────────────
setup_repo_with_remote
create_feature_branch "feature-a"
switch_to feature-a
commit_file "A1" "a1.txt"
drop_hash="$(head_hash)"
commit_file "A2" "a2.txt"
switch_to integration
commit_file "Int" "int.txt"
weave_branch "feature-a"

gl drop "$drop_hash" --yes
assert_commit_not_in_log "$drop_hash" "drop_on_branch"
assert_log_contains "A2" "drop_on_branch"

# ── Test: drop fails gracefully on unknown target ─────────────────────────
setup_repo_with_remote
gl_capture drop "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef" --yes
assert_exit_fail "$CODE" "drop_unknown_target"

pass
