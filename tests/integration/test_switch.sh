#!/usr/bin/env bash
# Integration tests for: gl switch
set -euo pipefail
source "$(dirname "$0")/helpers.sh"
trap 'rm -rf "$TMPROOT"' EXIT

# ══════════════════════════════════════════════════════════════════════════════
# PRECONDITIONS
# ══════════════════════════════════════════════════════════════════════════════

describe "precond: staged changes block switch"
setup_repo_with_remote
create_feature_branch "g-target"
write_file "staged.txt" "staged content"
git -C "$WORK" add staged.txt
gl_capture switch g-target
assert_exit_fail "$CODE" "staged_blocks"
assert_contains "$OUT" "uncommitted changes" "staged_blocks_msg"

describe "precond: unstaged changes to tracked file block switch"
setup_repo_with_remote
commit_file "Tracked" "tracked.txt"
create_feature_branch "g-target"
write_file "tracked.txt" "modified"
gl_capture switch g-target
assert_exit_fail "$CODE" "unstaged_blocks"
assert_contains "$OUT" "uncommitted changes" "unstaged_blocks_msg"

describe "precond: branch not found errors"
setup_repo_with_remote
gl_capture switch no-such-branch
assert_exit_fail "$CODE" "not_found_exit"
assert_contains "$OUT" "not found" "not_found_msg"

# ══════════════════════════════════════════════════════════════════════════════
# SWITCHING TO A LOCAL BRANCH
# ══════════════════════════════════════════════════════════════════════════════

describe "switch to local branch by name moves HEAD onto that branch"
setup_repo_with_remote
create_feature_branch "g-feat"
out=$(gl switch g-feat)
assert_exit_ok $? "local_switch_ok"
current=$(git -C "$WORK" rev-parse --abbrev-ref HEAD)
assert_eq "$current" "g-feat" "local_switch_head_on_branch"
assert_contains "$out" "Switched to" "local_switch_msg"
assert_contains "$out" "g-feat"      "local_switch_msg_name"

describe "HEAD is not detached after switching to a local branch"
setup_repo_with_remote
create_feature_branch "g-local"
commit_file "Local commit" "local.txt"
switch_to integration
gl switch g-local
assert_exit_ok $? "local_not_detached_ok"
detached=$(git -C "$WORK" symbolic-ref --short HEAD 2>/dev/null || echo "DETACHED")
assert_eq "$detached" "g-local" "local_not_detached_head"

describe "untracked files do not block switch"
setup_repo_with_remote
create_feature_branch "g-untracked"
write_file "untracked.txt" "not staged"
out=$(gl switch g-untracked)
assert_exit_ok $? "untracked_ok"
assert_contains "$out" "Switched to" "untracked_ok_msg"

# ══════════════════════════════════════════════════════════════════════════════
# SWITCHING TO A REMOTE-ONLY BRANCH
# ══════════════════════════════════════════════════════════════════════════════

describe "switch to remote-only branch detaches HEAD at the remote ref"
setup_repo_with_remote
# Create, push, then delete the local branch so only origin/g-remote exists
create_feature_branch "g-remote"
switch_to g-remote
commit_file "Remote commit" "remote.txt"
git -C "$WORK" push -q origin g-remote >/dev/null
remote_oid=$(git -C "$WORK" rev-parse origin/g-remote)
git -C "$WORK" checkout -q integration
git -C "$WORK" branch -D g-remote
# Now origin/g-remote exists but g-remote local branch does not
assert_branch_not_exists "g-remote" "remote_only_setup"
out=$(gl switch origin/g-remote)
assert_exit_ok $? "remote_only_ok"
assert_contains "$out" "Detached HEAD" "remote_only_msg"
assert_contains "$out" "origin/g-remote" "remote_only_msg_name"
head_oid=$(git -C "$WORK" rev-parse HEAD)
assert_eq "$head_oid" "$remote_oid" "remote_only_at_correct_oid"

describe "switch to remote-only branch does not create a local branch"
setup_repo_with_remote
create_feature_branch "g-remote2"
switch_to g-remote2
commit_file "Remote2 commit" "remote2.txt"
git -C "$WORK" push -q origin g-remote2 >/dev/null
git -C "$WORK" checkout -q integration
git -C "$WORK" branch -D g-remote2
gl switch origin/g-remote2
assert_exit_ok $? "remote_only_no_local_ok"
assert_branch_not_exists "g-remote2" "remote_only_no_local_branch"

describe "HEAD is detached after switching to a remote-only branch"
setup_repo_with_remote
create_feature_branch "g-detach"
switch_to g-detach
commit_file "Detach commit" "detach.txt"
git -C "$WORK" push -q origin g-detach >/dev/null
git -C "$WORK" checkout -q integration
git -C "$WORK" branch -D g-detach
gl switch origin/g-detach
assert_exit_ok $? "remote_detach_ok"
# symbolic-ref fails on detached HEAD — that's what we expect
if git -C "$WORK" symbolic-ref HEAD >/dev/null 2>&1; then
    fail "remote_detach_head: HEAD should be detached but is on a branch"
fi

# ══════════════════════════════════════════════════════════════════════════════
# SHORT ID RESOLUTION
# ══════════════════════════════════════════════════════════════════════════════

describe "switch by branch short ID resolves to the correct local branch"
setup_repo_with_remote
create_feature_branch "g-shortid"
switch_to g-shortid
commit_file "Shortid commit" "shortid.txt"
switch_to integration
weave_branch "g-shortid"
sid=$(branch_sid_from_status "g-shortid")
# Also verify the full-name form works
out_full=$(gl switch g-shortid)
assert_exit_ok $? "shortid_full_ok"
assert_contains "$out_full" "Switched to" "shortid_full_msg"
# Switch back and use the short ID
switch_to integration
out_sid=$(gl switch "$sid")
assert_exit_ok $? "shortid_sid_ok"
assert_contains "$out_sid" "Switched to"  "shortid_sid_msg"
assert_contains "$out_sid" "g-shortid"    "shortid_sid_name"
current=$(git -C "$WORK" rev-parse --abbrev-ref HEAD)
assert_eq "$current" "g-shortid" "shortid_sid_head"

# ══════════════════════════════════════════════════════════════════════════════
# ALIAS
# ══════════════════════════════════════════════════════════════════════════════

describe "gl sw is an alias for gl switch"
setup_repo_with_remote
create_feature_branch "g-alias"
out=$(gl sw g-alias)
assert_exit_ok $? "alias_ok"
current=$(git -C "$WORK" rev-parse --abbrev-ref HEAD)
assert_eq "$current" "g-alias" "alias_head"
assert_contains "$out" "Switched to" "alias_msg"

pass
