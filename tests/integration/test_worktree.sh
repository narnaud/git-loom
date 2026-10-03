#!/usr/bin/env bash
# Integration tests for: gl worktree
set -euo pipefail
source "$(dirname "$0")/helpers.sh"

# Run gl_capture from another worktree, restoring $WORK afterwards.
gl_capture_in() {
    local dir="$1"
    shift
    local main="$WORK"
    WORK="$dir"
    gl_capture "$@"
    WORK="$main"
}

describe "new creates a sibling worktree on its own integration branch"
setup_repo_with_remote
gl_capture wt new foo
assert_exit_ok "$CODE" "new_ok"
assert_contains "$OUT" "integration-foo" "new_msg_branch"
FOO="$TMPROOT/work-foo"
assert_eq "$(git -C "$FOO" rev-parse --abbrev-ref HEAD)" "integration-foo" "new_head"
assert_eq "$(git -C "$FOO" rev-parse --abbrev-ref integration-foo@{u})" "origin/$BASE_BRANCH" "new_upstream"

describe "loom works inside the new worktree"
gl_capture_in "$FOO" status
assert_exit_ok "$CODE" "status_in_worktree"
assert_contains "$OUT" "origin/$BASE_BRANCH" "status_in_worktree_upstream"
commit_file_in "$FOO" "Wt commit" "wt.txt"
gl_capture_in "$FOO" branch feature-wt -t HEAD
assert_exit_ok "$CODE" "branch_in_worktree"
gl_capture_in "$FOO" status
assert_contains "$OUT" "[feature-wt]" "status_shows_worktree_branch"

describe "merge refuses a branch woven into another worktree"
gl_capture branch merge feature-wt
assert_exit_fail "$CODE" "merge_guard"
assert_contains "$OUT" "integration-foo" "merge_guard_msg"

describe "list shows every worktree with an ID"
gl_capture wt list
assert_exit_ok "$CODE" "list_ok"
assert_contains "$OUT" "[integration]" "list_main"
assert_contains "$OUT" "[integration-foo]" "list_foo"
FOO_ID="$(grep -F "[integration-foo]" <<< "$OUT" | awk '{print $2}')"
gl_capture_json wt list --agent
assert_exit_ok "$CODE" "list_agent_ok"
assert_contains "$JSON" "[integration-foo]" "list_agent_on_stdout"
assert_contains "$(json_line)" '"status":"ok"' "list_agent_last_line"
gl_capture wt
assert_exit_ok "$CODE" "list_default_ok"
assert_contains "$OUT" "[integration-foo]" "list_default"

describe "drop keeps a branch holding commits no other ref has"
gl_capture_in "$FOO" branch unmerge feature-wt
commit_file_in "$FOO" "Loose" "loose.txt"
gl_capture wt drop "$FOO_ID"
assert_exit_ok "$CODE" "drop_ok"
assert_contains "$OUT" "Kept branch" "drop_kept_msg"
assert_eq "$(test -d "$FOO" && echo yes || echo no)" "no" "drop_removed_dir"
assert_branch_exists "integration-foo" "drop_kept_branch"
assert_branch_exists "feature-wt" "drop_kept_feature"

describe "init in a plain git worktree names the branch after it"
git -C "$WORK" worktree add -q --detach "$TMPROOT/work-bar"
gl_capture_in "$TMPROOT/work-bar" init
assert_exit_ok "$CODE" "init_in_worktree"
assert_eq "$(git -C "$TMPROOT/work-bar" rev-parse --abbrev-ref HEAD)" "integration-bar" "init_in_worktree_head"
gl_capture wt drop bar
assert_exit_ok "$CODE" "drop_by_name"
assert_contains "$OUT" "Deleted branch" "drop_deleted_msg"
assert_branch_not_exists "integration-bar" "drop_deleted_branch"

describe "path prints a worktree path, and the main one from a linked worktree"
gl wt new baz > /dev/null
BAZ="$TMPROOT/work-baz"
gl_capture wt path baz
assert_exit_ok "$CODE" "path_by_name"
assert_eq "$(cd "$OUT" && pwd -P)" "$(cd "$BAZ" && pwd -P)" "path_by_name_path"
gl_capture_in "$BAZ" wt path
assert_exit_ok "$CODE" "path_to_main"
assert_eq "$(cd "$OUT" && pwd -P)" "$(cd "$WORK" && pwd -P)" "path_to_main_path"

pass
