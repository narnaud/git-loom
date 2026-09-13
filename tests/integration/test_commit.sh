#!/usr/bin/env bash
# Integration tests for: gl commit
set -euo pipefail
source "$(dirname "$0")/helpers.sh"

# ── PRECONDITIONS ─────────────────────────────────────────────────────────────

describe "precond: not in a git repository"
new_tmpdir TMP_NOGIT
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

# ── LOOSE COMMIT (branch name matches upstream local counterpart) ─────────────

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

describe "loose commit: -i commits to the integration branch"
setup_repo_with_remote
write_file "loose.txt" "loose content"
git -C "$WORK" add loose.txt
out=$(gl commit -i -m "Integration commit")
assert_exit_ok $? "integration_ok"
assert_contains "$out" "Created commit"     "integration_created_msg"
assert_not_contains "$out" "on branch"      "integration_no_branch_label"
assert_head_msg "Integration commit"        "integration_head_msg"
assert_head_parent_count 1                  "integration_single_parent"

describe "loose commit: -i skips the picker and leaves woven branches alone"
setup_repo_with_remote
create_feature_branch "g-untouched"
switch_to g-untouched
commit_file "Branch work" "branch-work.txt"
switch_to integration
weave_branch "g-untouched"
old_tip=$(branch_oid g-untouched)
old_head=$(git -C "$WORK" rev-parse HEAD)
write_file "on-integration.txt" "integration content"
git -C "$WORK" add on-integration.txt
out=$(gl commit -i -m "On integration")
assert_exit_ok $? "integration_woven_ok"
assert_head_msg "On integration"            "integration_woven_head_msg"
assert_head_parent_count 1                  "integration_woven_single_parent"
assert_msg_at 1 "Merge g-untouched"          "integration_woven_parent_is_merge"
assert_eq "$old_tip" "$(branch_oid g-untouched)" "integration_woven_tip_unmoved"
assert_ne "$old_head" "$(git -C "$WORK" rev-parse HEAD)" "integration_woven_head_moved"

describe "loose commit: -i together with -b is rejected"
setup_repo_with_remote
write_file "both.txt" "both content"
git -C "$WORK" add both.txt
gl_capture commit -i -b g-nope -m "Both flags"
assert_exit_fail "$CODE" "integration_conflict_fails"
assert_contains "$OUT" "cannot be used with" "integration_conflict_msg"
assert_branch_not_exists "g-nope"            "integration_conflict_no_branch"

# ── COMMIT TO EXISTING WOVEN BRANCH ───────────────────────────────────────────

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

# ── COMMIT TO NEW BRANCH ──────────────────────────────────────────────────────

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

# ── STAGING — ZZ TOKEN ────────────────────────────────────────────────────────

describe "zz stages all unstaged changes before committing"
setup_repo_with_remote
write_file "zz-a.txt" "alpha"
write_file "zz-b.txt" "beta"
# Both files are untracked — zz should stage them all
out=$(gl commit -b g-zz-dest zz -m "Staged all via zz")
assert_exit_ok $? "zz_ok"
assert_log_contains "Staged all via zz"          "zz_in_log"
assert_contains "$(gl status)" "no changes"      "zz_clean_wt"

describe "zz wins over explicit file arguments when both provided"
setup_repo_with_remote
write_file "zz-win-a.txt" "a"
write_file "zz-win-b.txt" "b"
# Provide both zz and an explicit file — zz wins (stages everything)
out=$(gl commit -b g-zz-wins zz zz-win-a.txt -m "ZZ wins")
assert_exit_ok $? "zz_wins_ok"
assert_contains "$(gl status)" "no changes" "zz_wins_clean"

# ── STAGING — SPECIFIC FILES ──────────────────────────────────────────────────

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

# ── ALIAS ─────────────────────────────────────────────────────────────────────

describe "ci alias works identically to commit"
setup_repo_with_remote
write_file "alias-file.txt" "alias content"
git -C "$WORK" add alias-file.txt
out=$(gl ci -b g-alias-branch -m "Via ci alias")
assert_exit_ok $? "alias_ci_ok"
assert_contains "$out" "on branch"       "alias_ci_on_branch"
assert_log_contains "Via ci alias"       "alias_ci_in_log"

# ── CONTINUE / ABORT ──────────────────────────────────────────────────────────
# Engineering a rebase conflict for `commit` is impractical: the staged diff
# is relative to the integration HEAD, but it must apply to the feature branch
# state, so any file touched by both will conflict during commit creation itself
# rather than during the weave rebase. We therefore test continue/abort via a
# synthetic state file, which exercises the same dispatch and rollback paths.

describe "commit to branch: conflict → continue (resolving two conflicts) → new commit created"
setup_repo_with_remote
create_feature_branch "g-cont-conflict"
switch_to g-cont-conflict
printf "feature\n" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt; git -C "$WORK" commit -q -m "Feature commit"
switch_to integration
weave_branch "g-cont-conflict"
printf "integration\n" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt; git -C "$WORK" commit -q -m "Integration commit"
# C is created at HEAD (diff "integration"→"feature-v2"), then moved to the branch section.
# Cherry-picking C onto FA1="feature" conflicts: base="integration" ≠ ours="feature".
printf "feature-v2\n" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt
gl_capture commit -b g-cont-conflict -m "Feature v2"
assert_state_file   "commit_cont_state"
assert_contains "$OUT" "loom continue" "commit_cont_hint"
# 1st conflict: cherry-pick C onto FA1. Resolve to a non-empty value.
printf "resolved\n" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt
gl_capture continue
assert_state_file   "commit_cont_state2"
printf "integration\n" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt
gl_capture continue
assert_exit_ok  "$CODE" "commit_cont_ok"
assert_no_state_file   "commit_cont_state_removed"
assert_contains "$OUT" "Created commit" "commit_cont_msg"
assert_log_contains "Feature v2" "commit_cont_new_commit_in_log"

describe "commit to branch: conflict → abort → HEAD restored"
setup_repo_with_remote
create_feature_branch "g-abort-conflict"
switch_to g-abort-conflict
printf "feature\n" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt; git -C "$WORK" commit -q -m "Feature commit"
switch_to integration
weave_branch "g-abort-conflict"
printf "integration\n" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt; git -C "$WORK" commit -q -m "Integration commit"
old_head=$(head_hash)
printf "feature-v2\n" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt
gl_capture commit -b g-abort-conflict -m "Feature v2"
assert_state_file   "commit_abort_state"
gl_capture abort
assert_exit_ok  "$CODE" "commit_abort_ok"
assert_contains "$OUT" "Aborted"   "commit_abort_msg"
assert_no_state_file   "commit_abort_state_removed"
assert_eq "$old_head" "$(head_hash)" "commit_abort_head_restored"
assert_log_not_contains "Feature v2" "commit_abort_new_commit_gone"

# ── GIT ARGUMENT FORWARDING ───────────────────────────────────────────────────

describe "an option after -- is passed to git commit"
setup_repo_with_remote
create_feature_branch "g-forward"
switch_to integration
weave_branch "g-forward"
echo "forwarded" > "$WORK/forwarded.txt"
gl_capture commit -b g-forward -m "Forwarded commit" forwarded.txt -- "--author=Someone Else <someone@example.com>"
assert_exit_ok "$CODE" "commit_forward_ok"
assert_eq "Someone Else" "$(git -C "$WORK" log -1 --format=%an g-forward)" "commit_forward_author"

describe "a pre-commit hook is skipped with -- --no-verify"
setup_repo_with_remote
mkdir -p "$WORK/.git/hooks"
# The global config may point core.hooksPath elsewhere; aim it back at the repo.
git -C "$WORK" config core.hooksPath "$WORK/.git/hooks"
printf '#!/bin/sh
exit 1
' > "$WORK/.git/hooks/pre-commit"
chmod +x "$WORK/.git/hooks/pre-commit"
echo "hooked" > "$WORK/hooked.txt"
gl_capture commit -i -m "Hooked commit" hooked.txt
assert_exit_fail "$CODE" "commit_hook_blocks"
gl_capture commit -i -m "Hooked commit" hooked.txt -- --no-verify
assert_exit_ok "$CODE" "commit_no_verify_ok"
assert_head_msg "Hooked commit" "commit_no_verify_msg"

describe "an option before -- is rejected with a hint"
setup_repo_with_remote
echo "strict" > "$WORK/strict.txt"
gl_capture commit -i -m "Strict commit" strict.txt --no-verify
assert_exit_fail "$CODE" "commit_strict_parse_fails"
assert_contains "$OUT" "unexpected argument" "commit_strict_parse_msg"
assert_contains "$OUT" "-- --no-verify"      "commit_strict_parse_hint"

pass
