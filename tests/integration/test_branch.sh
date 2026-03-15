#!/usr/bin/env bash
# Integration tests for: gl branch new / merge / unmerge
set -euo pipefail
source "$(dirname "$0")/helpers.sh"
trap 'rm -rf "$TMPROOT"' EXIT

# ── Test: branch new creates a branch at the upstream base ────────────────
setup_repo_with_remote
gl branch new feature-x
assert_branch_exists "feature-x" "branch_new"
assert_eq "$(upstream_oid)" "$(branch_oid feature-x)" "branch_new"

# ── Test: branch new with explicit --target ───────────────────────────────
setup_repo_with_remote
commit_file "Base work" "base.txt"
base_commit="$(head_hash)"
gl branch new feature-y --target "$base_commit"
assert_branch_exists "feature-y" "branch_new_with_target"
assert_eq "$base_commit" "$(branch_oid feature-y)" "branch_new_with_target"

# ── Test: branch merge weaves a branch into integration ──────────────────
setup_repo_with_remote
create_feature_branch "feature-a"
switch_to feature-a
commit_file "Feature A work" "fa.txt"
switch_to integration

gl branch merge feature-a
assert_head_parent_count 2 "branch_merge"
assert_log_contains "Feature A work" "branch_merge"

# ── Test: branch unmerge removes a branch from integration ───────────────
setup_repo_with_remote
create_feature_branch "feature-b"
switch_to feature-b
commit_file "Feature B work" "fb.txt"
switch_to integration
commit_file "Integration work" "int.txt"
weave_branch "feature-b"

gl branch unmerge feature-b
assert_log_not_contains "Feature B work" "branch_unmerge"
assert_branch_exists "feature-b" "branch_unmerge_keeps_ref"

pass
