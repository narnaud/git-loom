#!/usr/bin/env bash
# Integration tests for: gl update
set -euo pipefail
source "$(dirname "$0")/helpers.sh"
trap 'rm -rf "$TMPROOT"' EXIT

# ══════════════════════════════════════════════════════════════════════════════
# PRECONDITIONS
# ══════════════════════════════════════════════════════════════════════════════

describe "precond: not in a git repository"
TMP_NOGIT=$(mktemp -d)
CODE=0
(cd "$TMP_NOGIT" && NO_COLOR=1 GIT_TERMINAL_PROMPT=0 "$GL_BIN" update >/dev/null 2>&1) || CODE=$?
assert_exit_fail "$CODE" "precond_not_git_repo"
rm -rf "$TMP_NOGIT"

describe "precond: detached HEAD"
setup_repo_with_remote
git -C "$WORK" checkout -q --detach HEAD
gl_capture update
assert_exit_fail "$CODE" "precond_detached_head"
assert_contains "$OUT" "detached" "precond_detached_msg"

describe "precond: branch with no upstream tracking"
setup_repo_with_remote
git -C "$WORK" branch --unset-upstream integration
gl_capture update
assert_exit_fail "$CODE" "precond_no_upstream"
assert_contains "$OUT" "no upstream" "precond_no_upstream_msg"

# ══════════════════════════════════════════════════════════════════════════════
# ALREADY UP TO DATE
# ══════════════════════════════════════════════════════════════════════════════

describe "already up-to-date: succeeds and reports updated branch"
setup_repo_with_remote
out=$(gl update 2>&1)
assert_exit_ok $? "already_up_to_date_ok"
assert_contains "$out" "Fetched latest changes"  "already_up_to_date_fetched"
assert_contains "$out" "Rebased onto upstream"   "already_up_to_date_rebased"
assert_contains "$out" "Updated branch"          "already_up_to_date_success_msg"

# ══════════════════════════════════════════════════════════════════════════════
# FETCH AND REBASE NEW UPSTREAM COMMITS
# ══════════════════════════════════════════════════════════════════════════════

describe "new upstream commit: integration rebased on top"
setup_repo_with_remote
OTHER="$TMPROOT/other"
git clone -q "$TMPROOT/remote.git" "$OTHER"
git -C "$OTHER" config user.email "test@test.com"
git -C "$OTHER" config user.name "Test"
git -C "$OTHER" config core.autocrlf false
commit_file_in "$OTHER" "Upstream feature" "upstream.txt"
git -C "$OTHER" push -q origin

old_head=$(head_hash)
out=$(gl update 2>&1)
assert_exit_ok $? "upstream_new_ok"
assert_contains "$out" "Fetched latest changes"  "upstream_new_fetched"
assert_contains "$out" "Rebased onto upstream"   "upstream_new_rebased"
assert_contains "$out" "Updated branch"          "upstream_new_success_msg"
# Integration HEAD moved forward (merged upstream)
new_head=$(head_hash)
assert_ne "$old_head" "$new_head" "upstream_new_head_advanced"
# The upstream commit is now in history
assert_log_contains "Upstream feature" "upstream_new_commit_in_log"

describe "new upstream commit: output includes upstream short hash"
# (upstream_info is appended to the success message)
assert_contains "$out" "origin/" "upstream_remote_name_in_msg"

# ══════════════════════════════════════════════════════════════════════════════
# LOCAL COMMITS PRESERVED
# ══════════════════════════════════════════════════════════════════════════════

describe "local commits on integration are preserved after rebase"
setup_repo_with_remote
commit_file "Local integration work" "local.txt"
local_msg=$(head_msg)

OTHER="$TMPROOT/other"
git clone -q "$TMPROOT/remote.git" "$OTHER"
git -C "$OTHER" config user.email "test@test.com"
git -C "$OTHER" config user.name "Test"
git -C "$OTHER" config core.autocrlf false
commit_file_in "$OTHER" "Remote upstream work" "remote.txt"
git -C "$OTHER" push -q origin

out=$(gl update 2>&1)
assert_exit_ok $? "local_commits_preserved_ok"
# Local commit message still in history
assert_log_contains "Local integration work" "local_commits_still_in_log"
# Remote commit also in history
assert_log_contains "Remote upstream work" "remote_commit_in_log"

# ══════════════════════════════════════════════════════════════════════════════
# DIRTY WORKING TREE (AUTOSTASH)
# ══════════════════════════════════════════════════════════════════════════════

describe "dirty working tree is preserved via autostash"
setup_repo_with_remote
# Commit a tracked file so we can modify it
commit_file "Tracked base" "tracked.txt"

OTHER="$TMPROOT/other"
git clone -q "$TMPROOT/remote.git" "$OTHER"
git -C "$OTHER" config user.email "test@test.com"
git -C "$OTHER" config user.name "Test"
git -C "$OTHER" config core.autocrlf false
commit_file_in "$OTHER" "Upstream while dirty" "upfile.txt"
git -C "$OTHER" push -q origin

# Create an uncommitted modification
write_file "tracked.txt" "dirty content"

out=$(gl update 2>&1)
assert_exit_ok $? "dirty_tree_ok"
assert_contains "$out" "Rebased onto upstream" "dirty_tree_rebased"
# The uncommitted change must survive
assert_file_content "tracked.txt" "dirty content" "dirty_tree_change_preserved"

# ══════════════════════════════════════════════════════════════════════════════
# WOVEN BRANCHES SURVIVE REBASE (--update-refs)
# ══════════════════════════════════════════════════════════════════════════════

describe "woven feature branch ref is updated after rebase"
setup_repo_with_remote
create_feature_branch "g-woven"
switch_to g-woven
commit_file "Woven commit A" "woven-a.txt"
commit_file "Woven commit B" "woven-b.txt"
local_tip=$(head_hash)
switch_to integration
weave_branch "g-woven"

OTHER="$TMPROOT/other"
git clone -q "$TMPROOT/remote.git" "$OTHER"
git -C "$OTHER" config user.email "test@test.com"
git -C "$OTHER" config user.name "Test"
git -C "$OTHER" config core.autocrlf false
commit_file_in "$OTHER" "Upstream progress" "progress.txt"
git -C "$OTHER" push -q origin

out=$(gl update 2>&1)
assert_exit_ok $? "woven_survive_ok"
assert_contains "$out" "Rebased onto upstream" "woven_survive_rebased"
# The feature branch still exists
assert_branch_exists "g-woven" "woven_survive_branch_exists"
# The feature branch tip changed (was rebased)
new_tip=$(branch_oid "g-woven")
assert_ne "$local_tip" "$new_tip" "woven_survive_ref_updated"
# Its commits are still reachable from the branch
assert_contains "$(git -C "$WORK" log g-woven --oneline)" "Woven commit B" "woven_survive_tip_msg"

# ══════════════════════════════════════════════════════════════════════════════
# GONE UPSTREAM CLEANUP
# ══════════════════════════════════════════════════════════════════════════════

describe "--yes removes a local branch whose remote tracking branch was deleted"
setup_repo_with_remote
# Create and push a feature branch, then weave it so commits are merged
create_feature_branch "g-gone-branch"
switch_to g-gone-branch
commit_file "Gone branch commit" "gone.txt"
git -C "$WORK" push -q -u origin g-gone-branch >/dev/null
switch_to integration
weave_branch "g-gone-branch"
# Delete it on the remote
git -C "$WORK" push -q origin --delete g-gone-branch >/dev/null
# gl update with --yes should fetch (prune), detect gone, and remove
out=$(gl update --yes 2>&1)
assert_exit_ok $? "gone_yes_ok"
assert_contains "$out" "gone upstream" "gone_yes_warning"
assert_contains "$out" "g-gone-branch"  "gone_yes_branch_listed"
assert_contains "$out" "Removed branch" "gone_yes_branch_removed"
assert_branch_not_exists "g-gone-branch" "gone_yes_branch_deleted"


describe "--yes removes only gone branches, not live ones"
setup_repo_with_remote
# A live branch (has remote)
create_feature_branch "g-live-branch"
switch_to g-live-branch
commit_file "Live branch commit" "live.txt"
git -C "$WORK" push -q -u origin g-live-branch >/dev/null
switch_to integration
weave_branch "g-live-branch"
# A gone branch (also woven so safe_delete can remove it)
create_feature_branch "h-gone-only"
switch_to h-gone-only
commit_file "Gone only commit" "goneonly.txt"
git -C "$WORK" push -q -u origin h-gone-only >/dev/null
switch_to integration
weave_branch "h-gone-only"
git -C "$WORK" push -q origin --delete h-gone-only >/dev/null
out=$(gl update --yes 2>&1)
assert_exit_ok $? "gone_selective_ok"
assert_branch_not_exists "h-gone-only"  "gone_selective_gone_removed"
assert_branch_exists     "g-live-branch" "gone_selective_live_kept"

# ══════════════════════════════════════════════════════════════════════════════
# SUBMODULE UPDATE
# ══════════════════════════════════════════════════════════════════════════════

describe "submodule update runs when .gitmodules is present"
setup_repo_with_remote

# Create a submodule repo in the temp dir
SUB_REMOTE="$TMPROOT/sub-remote.git"
SUB_SEED="$TMPROOT/sub-seed"
git init -q "$SUB_SEED"
git -C "$SUB_SEED" config user.email "test@test.com"
git -C "$SUB_SEED" config user.name "Test"
echo "submod content" > "$SUB_SEED/sub.txt"
git -C "$SUB_SEED" add sub.txt
git -C "$SUB_SEED" commit -q -m "Sub initial"
git clone -q --bare "$SUB_SEED" "$SUB_REMOTE"
rm -rf "$SUB_SEED"

# Add the submodule; pass protocol.file.allow=always via -c so the internal
# git-clone spawned by submodule-add inherits the permission (file:// is
# disallowed by default since Git 2.38.1).
(cd "$WORK" && git -c protocol.file.allow=always submodule -q add "$SUB_REMOTE" mysubmod)
git -C "$WORK" commit -q -m "Add submodule"

out=$(gl update 2>&1)
assert_exit_ok $? "submodule_ok"
assert_contains "$out" "Updating submodules" "submodule_spinner_start"
assert_contains "$out" "Updated submodules"  "submodule_spinner_stop"

pass
