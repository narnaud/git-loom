#!/usr/bin/env bash
# Integration tests for: loom continue / loom abort
set -euo pipefail
source "$(dirname "$0")/helpers.sh"

# ══════════════════════════════════════════════════════════════════════════════
# PRECONDITIONS — no operation in progress
# ══════════════════════════════════════════════════════════════════════════════

describe "continue: no state file → error"
setup_repo_with_remote
gl_capture continue
assert_exit_fail "$CODE" "no_state_continue"
assert_contains "$OUT" "No loom operation" "no_state_continue_msg"

describe "abort: no state file → error"
setup_repo_with_remote
gl_capture abort
assert_exit_fail "$CODE" "no_state_abort"
assert_contains "$OUT" "No loom operation" "no_state_abort_msg"

# ══════════════════════════════════════════════════════════════════════════════
# GUARD — another command blocked while paused
# ══════════════════════════════════════════════════════════════════════════════

describe "blocked: non-exempt command is rejected while a state file exists"
setup_repo_with_remote
mkdir -p "$WORK/.git/loom"
echo '{"command":"update","rollback":{"saved_head":"","saved_refs":{},"delete_branches":[],"saved_staged_patch":"","saved_worktree_patch":""},"context":null}' \
    > "$WORK/.git/loom/state.json"
gl_capture update
assert_exit_fail "$CODE" "blocked_while_paused"
assert_contains "$OUT" "loom continue" "blocked_while_paused_hint"
# No rebase is in progress here: the hint must say so, not claim conflicts.
assert_contains "$OUT" "no rebase is in progress" "blocked_while_stale_hint"

# ══════════════════════════════════════════════════════════════════════════════
# Helper: produce a conflict scenario
# Sets WORK (with remote) and leaves the rebase paused.
# Also sets OLD_HEAD to the pre-update HEAD.
# ══════════════════════════════════════════════════════════════════════════════
setup_conflict() {
    setup_repo_with_remote

    # Commit a base version of conflict.txt and push it upstream
    commit_file "Base commit" "conflict.txt"
    local upstream_branch
    upstream_branch="$(git -C "$WORK" rev-parse --abbrev-ref --symbolic-full-name @{u} | sed 's|origin/||')"
    git -C "$WORK" push -q origin "HEAD:$upstream_branch"

    # Upstream: modify conflict.txt
    OTHER="$TMPROOT/other"
    git clone -q "$TMPROOT/remote.git" "$OTHER"
    git -C "$OTHER" config user.email "test@test.com"
    git -C "$OTHER" config user.name "Test"
    git -C "$OTHER" config core.autocrlf false
    echo "upstream content" > "$OTHER/conflict.txt"
    git -C "$OTHER" add conflict.txt
    git -C "$OTHER" commit -q -m "Upstream change"
    git -C "$OTHER" push -q origin

    # Local: diverge on the same file
    echo "local content" > "$WORK/conflict.txt"
    git -C "$WORK" add conflict.txt
    git -C "$WORK" commit -q -m "Local change"

    OLD_HEAD="$(head_hash)"
}

# ══════════════════════════════════════════════════════════════════════════════
# loom update: continue cycle
# ══════════════════════════════════════════════════════════════════════════════

describe "update: conflict pauses, resolve, continue → success"
setup_conflict

gl_capture update
assert_state_file "state_file_exists_after_conflict"
assert_contains "$OUT" "Conflicts detected" "update_conflict_msg"
assert_contains "$OUT" "loom continue" "update_conflict_continue_hint"
assert_contains "$OUT" "loom abort"    "update_conflict_abort_hint"

# A blocked command during a real pause reports the live operation, not a stale state
gl_capture status
assert_exit_fail "$CODE" "blocked_during_conflict"
assert_contains "$OUT" "still in progress" "blocked_during_conflict_hint"

# Resolve the conflict and stage it
echo "resolved content" > "$WORK/conflict.txt"
git -C "$WORK" add conflict.txt

gl_capture continue
assert_exit_ok "$CODE" "update_continue_ok"
assert_no_state_file "state_file_removed_after_continue"
assert_log_contains "Local change"    "update_continue_local_commit_in_log"
assert_log_contains "Upstream change" "update_continue_upstream_in_log"

# ══════════════════════════════════════════════════════════════════════════════
# loom update: abort cycle
# ══════════════════════════════════════════════════════════════════════════════

describe "update: conflict pauses, abort → original state restored"
setup_conflict

gl_capture update
assert_state_file "abort_state_file_exists"

gl_capture abort
assert_exit_ok "$CODE" "update_abort_ok"
assert_contains "$OUT" "Aborted" "update_abort_msg"
assert_contains "$OUT" "update"  "update_abort_cmd_name"
assert_no_state_file "state_file_removed_after_abort"

# HEAD must be back to the original
new_head="$(head_hash)"
assert_eq "$OLD_HEAD" "$new_head" "update_abort_head_restored"

# Upstream commit must NOT be in local log
assert_log_not_contains "Upstream change" "update_abort_upstream_gone"

# ══════════════════════════════════════════════════════════════════════════════
# Stray rebase: git has a rebase in progress but no loom state file
# ══════════════════════════════════════════════════════════════════════════════

# Set up a conflict and fetch, leaving the ref to conflict with in $UPSTREAM.
setup_conflict_and_fetch() {
    setup_conflict
    UPSTREAM="$(git -C "$WORK" rev-parse --abbrev-ref --symbolic-full-name @{u})"
    git -C "$WORK" fetch -q origin
}

# Leave a plain `git rebase` paused on a conflict, with no loom state file.
setup_stray_rebase() {
    setup_conflict_and_fetch
    git -C "$WORK" rebase "$UPSTREAM" >/dev/null 2>&1 || true
    assert_rebase_in_progress "setup_stray_rebase"
}

setup_stray_merge() {
    setup_conflict_and_fetch
    git -C "$WORK" merge "$UPSTREAM" >/dev/null 2>&1 || true
    assert_merge_in_progress "setup_stray_merge"
}

describe "stray rebase: status reports it instead of a detached HEAD"
setup_stray_rebase
gl_capture status
assert_exit_fail "$CODE" "stray_status_fails"
assert_contains "$OUT" "rebase is in progress" "stray_status_msg"
assert_contains "$OUT" "loom abort" "stray_status_abort_hint"

describe "stray rebase: abort cancels it"
setup_stray_rebase
gl_capture abort
assert_exit_ok "$CODE" "stray_abort_ok"
assert_contains "$OUT" "Canceled the rebase" "stray_abort_msg"
assert_contains "$OUT" "no loom state" "stray_abort_no_rollback_note"
assert_eq "$OLD_HEAD" "$(head_hash)" "stray_abort_head_restored"
assert_no_rebase_in_progress "stray_abort_rebase_gone"

describe "stray rebase: continue finishes it"
setup_stray_rebase
echo "resolved content" > "$WORK/conflict.txt"
git -C "$WORK" add conflict.txt
gl_capture continue
assert_exit_ok "$CODE" "stray_continue_ok"
assert_contains "$OUT" "Completed the rebase git had in progress" "stray_continue_msg"
assert_contains "$OUT" "no loom state, so nothing else was done" "stray_continue_no_state_note"
assert_log_contains "Upstream change" "stray_continue_upstream_in_log"
assert_log_contains "Local change" "stray_continue_local_in_log"
assert_no_rebase_in_progress "stray_continue_rebase_finished"
assert_no_state_file "stray_continue_no_state_file"

describe "stray merge: abort cancels it"
setup_stray_merge

gl_capture abort
assert_exit_ok "$CODE" "stray_merge_abort_ok"
assert_contains "$OUT" "Canceled the merge" "stray_merge_abort_msg"
assert_contains "$OUT" "no loom state to roll back" "stray_merge_abort_no_rollback_note"
assert_file_content "conflict.txt" "local content" "stray_merge_abort_worktree_restored"
assert_log_not_contains "Upstream change" "stray_merge_abort_upstream_gone"
assert_no_merge_in_progress "stray_merge_abort_merge_gone"

describe "stray merge: continue finishes it"
setup_stray_merge
echo "resolved content" > "$WORK/conflict.txt"
git -C "$WORK" add conflict.txt

gl_capture continue
assert_exit_ok "$CODE" "stray_merge_continue_ok"
assert_contains "$OUT" "Completed the merge git had in progress" "stray_merge_continue_msg"
assert_contains "$OUT" "no loom state, so nothing else was done" "stray_merge_continue_no_state_note"
assert_log_contains "Upstream change" "stray_merge_continue_upstream_in_log"
assert_head_parent_count 2 "stray_merge_continue_made_merge_commit"
assert_no_merge_in_progress "stray_merge_continue_merge_finished"
assert_no_state_file "stray_merge_continue_no_state_file"

# ══════════════════════════════════════════════════════════════════════════════
# Non-conflict stop: an untracked file blocks the rebase, nothing is conflicted
# ══════════════════════════════════════════════════════════════════════════════

describe "untracked file in the way: the pause message does not claim conflicts"
setup_repo_with_remote
echo "committed content" > "$WORK/foo.txt"
git -C "$WORK" add foo.txt
git -C "$WORK" commit -q -m "add foo.txt"
git -C "$WORK" rm -q foo.txt
git -C "$WORK" commit -q -m "delete foo.txt"
DELETE_COMMIT="$(git -C "$WORK" rev-parse --short HEAD)"
# Dropping the delete re-picks "add foo.txt", which this untracked file blocks.
echo "untracked content" > "$WORK/foo.txt"

gl_capture drop "$DELETE_COMMIT" --yes
assert_contains "$OUT" "stopped part-way" "untracked_stop_msg"
assert_not_contains "$OUT" "Conflicts detected" "untracked_stop_no_conflict_claim"
assert_state_file "untracked_stop_state_file"

# The file is still in the way, so the continue stops again — with a clean
# index, which the "conflicts remain" message would misdescribe.
gl_capture continue
assert_exit_ok "$CODE" "untracked_continue_ok"
assert_contains "$OUT" "stopped again" "untracked_continue_msg"
assert_not_contains "$OUT" "Conflicts remain" "untracked_continue_no_conflict_claim"
assert_state_file "untracked_continue_state_file"

gl_capture abort
assert_exit_ok "$CODE" "untracked_stop_abort_ok"
assert_no_state_file "untracked_stop_state_removed"

# ══════════════════════════════════════════════════════════════════════════════
# Edit pause: git rebase --continue exits 0 while the rebase is far from over
# ══════════════════════════════════════════════════════════════════════════════

describe "edit pause: continue does not claim the rebase completed"
setup_repo_with_remote
for i in 1 2 3 4; do commit_file "c$i" "f$i.txt"; done
# `sed -i` is GNU-only, so rewrite the todo through a temp file instead.
GIT_SEQUENCE_EDITOR='f() { sed "s/^pick/edit/" "$1" > "$1.new" && mv "$1.new" "$1"; }; f' \
    git -C "$WORK" rebase -i HEAD~3 >/dev/null 2>&1
# c2, c3 and c4 are each an `edit`, so the rebase stops three times. HEAD
# names the step it is sitting on, which says how far it got without reading
# git's own rebase directory.
assert_rebase_in_progress "edit_pause_first_stop"
assert_head_msg "c2" "edit_pause_stopped_at_c2"

gl_capture continue
assert_exit_ok "$CODE" "edit_pause_continue_ok"
assert_contains "$OUT" "paused at an" "edit_pause_msg"
assert_not_contains "$OUT" "Completed" "edit_pause_no_false_success"
assert_rebase_in_progress "edit_pause_still_rebasing"
assert_head_msg "c3" "edit_pause_advanced_one_step"

# The next continue reaches the last `edit` — still not the end of the rebase.
gl_capture continue
assert_exit_ok "$CODE" "edit_pause_continue2_ok"
assert_contains "$OUT" "paused at an" "edit_pause_msg2"
assert_not_contains "$OUT" "Completed" "edit_pause_no_false_success2"
assert_rebase_in_progress "edit_pause_still_rebasing2"
assert_head_msg "c4" "edit_pause_advanced_two_steps"

# Only past the last one does continue report completion.
gl_capture continue
assert_exit_ok "$CODE" "edit_pause_continue3_ok"
assert_contains "$OUT" "Completed" "edit_pause_final_completed"
assert_no_rebase_in_progress "edit_pause_rebase_finished"

# ══════════════════════════════════════════════════════════════════════════════
# A failed abort must not roll back on top of a live rebase
# ══════════════════════════════════════════════════════════════════════════════

describe "abort: a failing git rebase --abort leaves the rollback undone"
# `commit` is the case with something to roll back: it creates a branch and a
# commit before the rebase, so a rollback that runs anyway is visible.
setup_repo_with_remote
commit_file "Base commit" "conflict.txt"
commit_file "Integration change" "conflict.txt"
OLD_HEAD="$(head_hash)"

# Weaving this under the integration commit replays it onto a different
# version of the same line, which conflicts.
echo "feature change" > "$WORK/conflict.txt"
gl_capture commit -b feat -m "Feature change" zz
assert_state_file "failed_abort_state_file"
assert_rebase_in_progress "failed_abort_rebase_running"

# A held index.lock makes `git rebase --abort` fail, as a concurrent git
# process would.
HEAD_BEFORE="$(head_hash)"
touch "$WORK/.git/index.lock"
gl_capture abort
assert_exit_fail "$CODE" "failed_abort_fails"
assert_branch_exists "feat" "failed_abort_rollback_not_applied"
assert_state_file "failed_abort_state_kept"
assert_rebase_in_progress "failed_abort_rebase_kept"
# Nothing was rolled back: the `reset --mixed` never ran either.
assert_eq "$HEAD_BEFORE" "$(head_hash)" "failed_abort_head_untouched"
# "loom abort" alone also matches the ordinary conflict message, so assert on
# the part that only the kept-state hint says.
assert_contains "$OUT" "state was kept" "failed_abort_msg"

# Once the lock is gone, abort works and restores the original state
rm -f "$WORK/.git/index.lock"
gl_capture abort
assert_exit_ok "$CODE" "failed_abort_retry_ok"
assert_no_state_file "failed_abort_retry_state_removed"
assert_no_rebase_in_progress "failed_abort_retry_rebase_gone"
assert_eq "$OLD_HEAD" "$(head_hash)" "failed_abort_retry_head_restored"
assert_branch_not_exists "feat" "failed_abort_retry_branch_removed"

# A conflicted `branch merge` is the merge-side twin of the case above: the
# state file describes a merge, so `loom abort` runs `git merge --abort`.
describe "abort: a failing git merge --abort keeps the state too"
setup_repo_with_remote
create_feature_branch "g-failed-abort"
switch_to g-failed-abort
echo "feature content" > "$WORK/shared.txt"
git -C "$WORK" add shared.txt
git -C "$WORK" commit -q -m "Feature change"
switch_to integration
echo "integration content" > "$WORK/shared.txt"
git -C "$WORK" add shared.txt
git -C "$WORK" commit -q -m "Integration change"
OLD_HEAD="$(head_hash)"

gl_capture branch merge g-failed-abort
assert_state_file "failed_merge_abort_state"
assert_merge_in_progress "failed_merge_abort_merge_running"

touch "$WORK/.git/index.lock"
gl_capture abort
assert_exit_fail "$CODE" "failed_merge_abort_fails"
assert_state_file "failed_merge_abort_state_kept"
assert_merge_in_progress "failed_merge_abort_merge_kept"
assert_contains "$OUT" "state was kept" "failed_merge_abort_msg"

# Once the lock is gone the retry goes through, as for the rebase case.
rm -f "$WORK/.git/index.lock"
gl_capture abort
assert_exit_ok "$CODE" "failed_merge_abort_retry_ok"
assert_no_state_file "failed_merge_abort_retry_state_removed"
assert_no_merge_in_progress "failed_merge_abort_retry_merge_gone"
assert_eq "$OLD_HEAD" "$(head_hash)" "failed_merge_abort_retry_head_restored"

pass
