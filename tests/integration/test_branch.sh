#!/usr/bin/env bash
# Integration tests for: gl branch new / merge / unmerge
set -euo pipefail
source "$(dirname "$0")/helpers.sh"
trap 'rm -rf "$TMPROOT"' EXIT

# ══════════════════════════════════════════════════════════════════════════════
# PRECONDITIONS
# ══════════════════════════════════════════════════════════════════════════════

describe "precond: not in a git repository"
TMP_NOGIT=$(mktemp -d)
CODE=0
(cd "$TMP_NOGIT" && NO_COLOR=1 GIT_TERMINAL_PROMPT=0 "$GL_BIN" branch new g-test >/dev/null 2>&1) || CODE=$?
assert_exit_fail "$CODE" "precond_not_git_repo"
rm -rf "$TMP_NOGIT"

# ══════════════════════════════════════════════════════════════════════════════
# BRANCH NEW — DEFAULT TARGET (MERGE-BASE)
# ══════════════════════════════════════════════════════════════════════════════

describe "branch new creates branch at upstream merge-base by default"
setup_repo_with_remote
out=$(gl branch new g-default)
assert_exit_ok $? "new_default_ok"
assert_branch_exists "g-default" "new_default_exists"
assert_eq "$(upstream_oid)" "$(branch_oid g-default)" "new_default_at_mergebase"
assert_contains "$out" "Created branch" "new_default_msg"

describe "implicit form: gl branch <name> is the same as gl branch new <name>"
setup_repo_with_remote
out=$(gl branch g-implicit)
assert_exit_ok $? "new_implicit_ok"
assert_branch_exists "g-implicit" "new_implicit_exists"
assert_eq "$(upstream_oid)" "$(branch_oid g-implicit)" "new_implicit_at_mergebase"

describe "create alias works identically to new"
setup_repo_with_remote
out=$(gl branch create g-created)
assert_exit_ok $? "new_create_alias_ok"
assert_branch_exists "g-created" "new_create_alias_exists"

describe "branch at merge-base does not create merge topology"
setup_repo_with_remote
commit_file "Loose commit" "loose.txt"
gl branch new g-at-base
# HEAD should still be a single-parent commit (no weaving at merge-base)
assert_head_parent_count 1 "new_no_weave_at_base"

# ══════════════════════════════════════════════════════════════════════════════
# BRANCH NEW — EXPLICIT TARGET
# ══════════════════════════════════════════════════════════════════════════════

describe "branch new --target by full commit hash"
setup_repo_with_remote
commit_file "Target commit" "target.txt"
target_hash="$(head_hash)"
commit_file "Later commit" "later.txt"
out=$(gl branch new g-by-hash --target "$target_hash")
assert_exit_ok $? "new_hash_ok"
assert_branch_exists "g-by-hash" "new_hash_exists"

describe "branch new --target by short commit ID"
setup_repo_with_remote
commit_file "Short ID commit" "sid.txt"
commit_file "Later on top" "later_sid.txt"
short_id=$(commit_sid_from_status "Short ID commit")
out=$(gl branch new g-by-shortid --target "$short_id")
assert_exit_ok $? "new_shortid_ok"
assert_branch_exists "g-by-shortid" "new_shortid_exists"

describe "branch new --target by branch name (resolves to branch tip)"
setup_repo_with_remote
create_feature_branch "g-source"
switch_to g-source
commit_file "Source tip" "source.txt"
switch_to integration
out=$(gl branch new g-from-branch --target g-source)
assert_exit_ok $? "new_branch_tip_ok"
assert_branch_exists "g-from-branch" "new_branch_tip_exists"
assert_eq "$(branch_oid g-source)" "$(branch_oid g-from-branch)" "new_branch_tip_eq"

# ══════════════════════════════════════════════════════════════════════════════
# BRANCH NEW — WEAVING
# ══════════════════════════════════════════════════════════════════════════════

describe "target on first-parent line triggers automatic weaving"
setup_repo_with_remote
commit_file "Weaveable commit" "weave.txt"
weave_target="$(head_hash)"
commit_file "Second loose commit" "second.txt"
gl branch new g-woven-auto --target "$weave_target"
# Weaving creates a merge commit; loose commits build on top of it, so HEAD
# is the loose commit (1 parent) and the merge commit is one step below (2 parents).
assert_head_parent_count 1 "new_weave_head_topo"
assert_eq "$(parent_count_at HEAD~1)" "2" "new_weave_merge_topo"
assert_branch_exists "g-woven-auto" "new_weave_branch_exists"

describe "target at HEAD triggers weaving (all loose commits move into branch)"
setup_repo_with_remote
commit_file "Commit A" "a.txt"
commit_file "Commit B" "b.txt"
head_before="$(head_hash)"
gl branch new g-at-head --target HEAD
assert_branch_exists "g-at-head" "new_head_branch_exists"
assert_head_parent_count 2 "new_head_merge_topo"

# ══════════════════════════════════════════════════════════════════════════════
# BRANCH NEW — NAME VALIDATION
# ══════════════════════════════════════════════════════════════════════════════

describe "duplicate branch name is rejected"
setup_repo_with_remote
gl branch new g-dup
gl_capture branch new g-dup
assert_exit_fail "$CODE" "new_dup_fail"
assert_contains "$OUT" "already exists" "new_dup_msg"

describe "invalid git name (contains ..) is rejected"
setup_repo_with_remote
gl_capture branch new "g..bad"
assert_exit_fail "$CODE" "new_invalid_name_fail"

# ══════════════════════════════════════════════════════════════════════════════
# BRANCH MERGE
# ══════════════════════════════════════════════════════════════════════════════

describe "branch merge weaves a local branch into integration"
setup_repo_with_remote
create_feature_branch "g-merge-target"
switch_to g-merge-target
commit_file "Feature work" "feat.txt"
switch_to integration
out=$(gl branch merge g-merge-target)
assert_exit_ok $? "merge_ok"
assert_contains "$out" "Woven" "merge_success_msg"
assert_contains "$out" "g-merge-target" "merge_name_in_msg"
assert_head_parent_count 2 "merge_no_ff_topo"
assert_log_contains "Feature work" "merge_commit_in_log"

describe "branch merge rejects an already-woven branch"
setup_repo_with_remote
create_feature_branch "g-already-woven"
switch_to g-already-woven
commit_file "Already woven work" "aw.txt"
switch_to integration
weave_branch "g-already-woven"
gl_capture branch merge g-already-woven
assert_exit_fail "$CODE" "merge_already_woven_fail"
assert_contains "$OUT" "already woven" "merge_already_woven_msg"

describe "branch merge rejects a non-existent branch"
setup_repo_with_remote
gl_capture branch merge g-does-not-exist
assert_exit_fail "$CODE" "merge_nonexistent_fail"

# ══════════════════════════════════════════════════════════════════════════════
# BRANCH MERGE — CONFLICT RECOVERY
# ══════════════════════════════════════════════════════════════════════════════
# Both branches modify the same file to trigger a conflict on merge.

describe "branch merge: conflict → continue (resolving conflict) → success"
setup_repo_with_remote
create_feature_branch "g-conflict-cont"
switch_to g-conflict-cont
echo "feature content" > "$WORK/shared.txt"
git -C "$WORK" add shared.txt
git -C "$WORK" commit -q -m "Feature change"
switch_to integration
echo "integration content" > "$WORK/shared.txt"
git -C "$WORK" add shared.txt
git -C "$WORK" commit -q -m "Integration change"
gl_capture branch merge g-conflict-cont
assert_exit_ok  "$CODE"            "merge_cont_exit"
assert_state_file                  "merge_cont_state"
assert_contains "$OUT" "loom continue" "merge_cont_hint"
echo "resolved" > "$WORK/shared.txt"
git -C "$WORK" add shared.txt
gl_capture continue
assert_exit_ok  "$CODE"            "merge_cont_ok"
assert_no_state_file               "merge_cont_state_removed"
assert_contains "$OUT" "Woven"     "merge_cont_msg"
assert_head_parent_count 2         "merge_cont_merge_commit"

describe "branch merge: conflict → abort → HEAD restored"
setup_repo_with_remote
create_feature_branch "g-conflict-abort"
switch_to g-conflict-abort
echo "feature content" > "$WORK/shared.txt"
git -C "$WORK" add shared.txt
git -C "$WORK" commit -q -m "Feature change"
switch_to integration
echo "integration content" > "$WORK/shared.txt"
git -C "$WORK" add shared.txt
git -C "$WORK" commit -q -m "Integration change"
old_head=$(head_hash)
gl_capture branch merge g-conflict-abort
assert_state_file                  "merge_abort_state"
gl_capture abort
assert_exit_ok  "$CODE"            "merge_abort_ok"
assert_contains "$OUT" "Aborted"   "merge_abort_msg"
assert_no_state_file               "merge_abort_state_removed"
assert_eq "$old_head" "$(head_hash)" "merge_abort_head_restored"
assert_head_parent_count 1         "merge_abort_not_merged"

# ══════════════════════════════════════════════════════════════════════════════
# BRANCH UNMERGE
# ══════════════════════════════════════════════════════════════════════════════

describe "branch unmerge removes commits from integration, preserves branch ref"
setup_repo_with_remote
create_feature_branch "g-unmerge-me"
switch_to g-unmerge-me
commit_file "Unmerge this" "um.txt"
switch_to integration
weave_branch "g-unmerge-me"
out=$(gl branch unmerge g-unmerge-me)
assert_exit_ok $? "unmerge_ok"
assert_contains "$out" "Unwoven" "unmerge_success_msg"
assert_branch_exists "g-unmerge-me" "unmerge_ref_preserved"
assert_log_not_contains "Unmerge this" "unmerge_commits_gone"

describe "branch unmerge by branch short ID"
setup_repo_with_remote
create_feature_branch "g-unmerge-sid"
switch_to g-unmerge-sid
commit_file "Sid unmerge work" "sid_um.txt"
switch_to integration
weave_branch "g-unmerge-sid"
branch_sid=$(branch_sid_from_status "g-unmerge-sid")
out=$(gl branch unmerge "$branch_sid")
assert_exit_ok $? "unmerge_shortid_ok"
assert_branch_exists "g-unmerge-sid" "unmerge_shortid_ref_preserved"
assert_log_not_contains "Sid unmerge work" "unmerge_shortid_commits_gone"

describe "branch unmerge rejects a branch not woven into integration"
setup_repo_with_remote
create_feature_branch "g-not-woven"
switch_to g-not-woven
commit_file "Not woven work" "nw.txt"
switch_to integration
gl_capture branch unmerge g-not-woven
assert_exit_fail "$CODE" "unmerge_not_woven_fail"
assert_contains "$OUT" "is not woven" "unmerge_not_woven_msg"

describe "branch unmerge rejects a non-existent branch"
setup_repo_with_remote
gl_capture branch unmerge g-missing-xyz
assert_exit_fail "$CODE" "unmerge_nonexistent_fail"

pass
