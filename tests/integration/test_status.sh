#!/usr/bin/env bash
# Integration tests for: gl status
set -euo pipefail
source "$(dirname "$0")/helpers.sh"
trap 'rm -rf "$TMPROOT"' EXIT

# ── Test: status on empty integration branch ──────────────────────────────
setup_repo_with_remote
out=$(gl status)
assert_exit_ok $? "status_empty"

# ── Test: status shows a woven feature branch ─────────────────────────────
setup_repo_with_remote
create_feature_branch "feature-a"
switch_to feature-a
commit_file "Add alpha" "alpha.txt"
switch_to integration
weave_branch "feature-a"

out=$(gl status)
assert_contains "$out" "feature-a" "status_with_branch"
assert_contains "$out" "Add alpha"  "status_with_branch"

# ── Test: status shows multiple woven branches ────────────────────────────
setup_repo_with_remote
create_feature_branch "feature-a"
switch_to feature-a
commit_file "Alpha commit" "a.txt"
switch_to integration

create_feature_branch "feature-b"
switch_to feature-b
commit_file "Beta commit" "b.txt"
switch_to integration

weave_branch "feature-a"
weave_branch "feature-b"

out=$(gl status)
assert_contains "$out" "feature-a" "status_two_branches"
assert_contains "$out" "feature-b" "status_two_branches"

pass
