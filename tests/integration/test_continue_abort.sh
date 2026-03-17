#!/usr/bin/env bash
# Integration tests for: loom continue / loom abort
set -euo pipefail
source "$(dirname "$0")/helpers.sh"
trap 'rm -rf "$TMPROOT"' EXIT

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
assert_contains "$OUT" "loom continue" "update_conflict_continue_hint"
assert_contains "$OUT" "loom abort"    "update_conflict_abort_hint"

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

pass
