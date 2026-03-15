#!/usr/bin/env bash
# Integration tests for: gl reword
set -euo pipefail
source "$(dirname "$0")/helpers.sh"
trap 'rm -rf "$TMPROOT"' EXIT

describe "reword HEAD commit message"
setup_repo_with_remote
commit_file "Old message" "f.txt"

gl reword HEAD --message "New message"
assert_head_msg "New message" "reword_head"

describe "reword a non-HEAD commit"
setup_repo_with_remote
commit_file "Old middle" "mid.txt"
target_hash="$(head_hash)"
commit_file "Top commit" "top.txt"

gl reword "$target_hash" --message "Renamed middle"
assert_head_msg "Top commit"     "reword_non_head"
assert_msg_at 1 "Renamed middle" "reword_non_head"

describe "rename a branch"
setup_repo_with_remote
create_feature_branch "old-name"
switch_to old-name
commit_file "Branch commit" "b.txt"
switch_to integration
weave_branch "old-name"

gl reword old-name --message "new-name"
assert_branch_exists     "new-name" "reword_branch_rename"
assert_branch_not_exists "old-name" "reword_branch_rename"

describe "reword with empty message is rejected"
setup_repo_with_remote
commit_file "Some commit" "s.txt"

gl_capture reword HEAD --message ""
assert_exit_fail "$CODE" "reword_empty_message"

pass
