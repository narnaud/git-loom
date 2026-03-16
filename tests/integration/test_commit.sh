#!/usr/bin/env bash
# Integration tests for: gl commit
set -euo pipefail
source "$(dirname "$0")/helpers.sh"
trap 'rm -rf "$TMPROOT"' EXIT

# ══════════════════════════════════════════════════════════════════════════════
# PRECONDITIONS
# ══════════════════════════════════════════════════════════════════════════════

describe "precond: not in a git repository"
TMP_NOGIT=$(mktemp -d)
CODE=0
(cd "$TMP_NOGIT" && NO_COLOR=1 GIT_TERMINAL_PROMPT=0 "$GL_BIN" commit -b g-x -m "msg" >/dev/null 2>&1) || CODE=$?
assert_exit_fail "$CODE" "precond_not_git_repo"
rm -rf "$TMP_NOGIT"

describe "precond: nothing staged produces clear error"
setup_repo_with_remote
# verify_has_staged_changes fires before branch resolution
gl_capture commit -b g-target -m "nothing staged"
assert_exit_fail "$CODE" "precond_nothing_staged_fail"
assert_contains "$OUT" "Nothing to commit" "precond_nothing_staged_msg"

# ══════════════════════════════════════════════════════════════════════════════
# LOOSE COMMIT (branch name matches upstream local counterpart)
# ══════════════════════════════════════════════════════════════════════════════

describe "loose commit: created directly when branch name matches upstream"
setup_repo_with_remote
# Switch to the base branch (e.g. main) which tracks origin/main
base_branch=$(git -C "$WORK" rev-parse --abbrev-ref integration@{upstream} | sed 's|origin/||')
switch_to "$base_branch"
write_file "loose.txt" "loose content"
git -C "$WORK" add loose.txt
out=$(gl commit -m "Loose commit")
assert_exit_ok $? "loose_ok"
assert_contains "$out" "Created commit"  "loose_created_msg"
assert_not_contains "$out" "on branch"   "loose_no_branch_label"
assert_head_msg "Loose commit"           "loose_head_msg"
assert_head_parent_count 1               "loose_single_parent"

describe "loose commit: -b overrides loose-commit path and targets a branch"
setup_repo_with_remote
base_branch=$(git -C "$WORK" rev-parse --abbrev-ref integration@{upstream} | sed 's|origin/||')
switch_to "$base_branch"
write_file "forced.txt" "forced content"
git -C "$WORK" add forced.txt
out=$(gl commit -b g-forced -m "Forced onto branch")
assert_exit_ok $? "loose_override_ok"
assert_contains "$out" "on branch"    "loose_override_on_branch"
assert_contains "$out" "g-forced"     "loose_override_branch_name"
assert_branch_exists "g-forced"       "loose_override_branch_exists"

# ══════════════════════════════════════════════════════════════════════════════
# COMMIT TO EXISTING WOVEN BRANCH
# ══════════════════════════════════════════════════════════════════════════════

describe "commit to existing woven branch by name"
setup_repo_with_remote
create_feature_branch "g-woven-target"
switch_to g-woven-target
commit_file "Initial branch work" "init.txt"
switch_to integration
weave_branch "g-woven-target"
old_tip=$(branch_oid g-woven-target)
write_file "new-work.txt" "new work"
git -C "$WORK" add new-work.txt
out=$(gl commit -b g-woven-target -m "New work on target")
assert_exit_ok $? "woven_name_ok"
assert_contains "$out" "on branch"          "woven_name_on_branch"
assert_contains "$out" "g-woven-target"     "woven_name_branch_label"
assert_ne "$old_tip" "$(branch_oid g-woven-target)" "woven_name_tip_moved"
assert_log_contains "New work on target"    "woven_name_in_log"

describe "commit to existing woven branch by short ID"
setup_repo_with_remote
create_feature_branch "g-sid-dest"
switch_to g-sid-dest
commit_file "SID branch base" "sid-base.txt"
switch_to integration
weave_branch "g-sid-dest"
branch_sid=$(branch_sid_from_status "g-sid-dest")
write_file "sid-work.txt" "sid work"
git -C "$WORK" add sid-work.txt
out=$(gl commit -b "$branch_sid" -m "Commit via branch short ID")
assert_exit_ok $? "woven_sid_ok"
assert_contains "$out" "g-sid-dest"                  "woven_sid_branch_in_out"
assert_log_contains "Commit via branch short ID"     "woven_sid_in_log"

describe "commit to a non-woven branch is rejected"
setup_repo_with_remote
# Branch must have commits outside integration's history to be truly non-woven.
# An empty branch at the merge-base is treated as an empty woven section.
create_feature_branch "g-not-woven"
switch_to g-not-woven
commit_file "Outside integration" "outside.txt"
switch_to integration
write_file "staged.txt" "staged"
git -C "$WORK" add staged.txt
gl_capture commit -b g-not-woven -m "should fail"
assert_exit_fail "$CODE" "non_woven_fail"
assert_contains "$OUT" "is not woven" "non_woven_msg"

# ══════════════════════════════════════════════════════════════════════════════
# COMMIT TO NEW BRANCH
# ══════════════════════════════════════════════════════════════════════════════

describe "new branch name: branch is created, commit lands there, topology woven"
setup_repo_with_remote
write_file "new-branch-work.txt" "new branch content"
git -C "$WORK" add new-branch-work.txt
out=$(gl commit -b g-brand-new -m "First commit on new branch")
assert_exit_ok $? "new_branch_ok"
assert_branch_exists "g-brand-new"                       "new_branch_exists"
assert_contains "$out" "g-brand-new"                     "new_branch_name_in_out"
assert_contains "$out" "on branch"                       "new_branch_on_branch"
assert_log_contains "First commit on new branch"         "new_branch_in_log"
# New branch is woven in — integration HEAD becomes a merge commit
assert_head_parent_count 2                               "new_branch_woven_topo"

describe "second commit to same new branch appends correctly"
setup_repo_with_remote
write_file "first.txt" "first"
git -C "$WORK" add first.txt
gl commit -b g-growing -m "First"
write_file "second.txt" "second"
git -C "$WORK" add second.txt
out=$(gl commit -b g-growing -m "Second")
assert_exit_ok $? "second_commit_ok"
assert_log_contains "Second"  "second_commit_in_log"
assert_log_contains "First"   "first_commit_preserved"

# ══════════════════════════════════════════════════════════════════════════════
# STAGING — ZZ TOKEN
# ══════════════════════════════════════════════════════════════════════════════

describe "zz stages all unstaged changes before committing"
setup_repo_with_remote
write_file "zz-a.txt" "alpha"
write_file "zz-b.txt" "beta"
# Both files are untracked — zz should stage them all
out=$(gl commit -b g-zz-dest zz -m "Staged all via zz")
assert_exit_ok $? "zz_ok"
assert_log_contains "Staged all via zz"          "zz_in_log"
# Working tree should be clean after zz-staged commit
assert_contains "$(gl status)" "no changes"      "zz_clean_wt"

describe "zz wins over explicit file arguments when both provided"
setup_repo_with_remote
write_file "zz-win-a.txt" "a"
write_file "zz-win-b.txt" "b"
# Provide both zz and an explicit file — zz wins (stages everything)
out=$(gl commit -b g-zz-wins zz zz-win-a.txt -m "ZZ wins")
assert_exit_ok $? "zz_wins_ok"
assert_contains "$(gl status)" "no changes" "zz_wins_clean"

# ══════════════════════════════════════════════════════════════════════════════
# STAGING — SPECIFIC FILES
# ══════════════════════════════════════════════════════════════════════════════

describe "specific file argument stages only that file, leaving others dirty"
setup_repo_with_remote
write_file "stage-this.txt"  "to be staged"
write_file "leave-this.txt"  "to stay dirty"
out=$(gl commit -b g-specific -m "Only stage-this" stage-this.txt)
assert_exit_ok $? "specific_file_ok"
assert_log_contains "Only stage-this"                     "specific_file_in_log"
# leave-this.txt is still untracked/dirty in the working tree
status_out=$(gl status)
assert_contains     "$status_out" "leave-this.txt"        "specific_leave_dirty"
assert_not_contains "$status_out" "stage-this.txt"        "specific_staged_clean"

# ══════════════════════════════════════════════════════════════════════════════
# ALIAS
# ══════════════════════════════════════════════════════════════════════════════

describe "ci alias works identically to commit"
setup_repo_with_remote
write_file "alias-file.txt" "alias content"
git -C "$WORK" add alias-file.txt
out=$(gl ci -b g-alias-branch -m "Via ci alias")
assert_exit_ok $? "alias_ci_ok"
assert_contains "$out" "on branch"       "alias_ci_on_branch"
assert_log_contains "Via ci alias"       "alias_ci_in_log"

pass
