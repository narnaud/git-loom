#!/usr/bin/env bash
# Integration tests for: gl fold (amend / fixup / move)
set -euo pipefail
source "$(dirname "$0")/helpers.sh"
trap 'rm -rf "$TMPROOT"' EXIT

describe "fold a file into HEAD (amend)"
setup_repo_with_remote
commit_file "First commit" "file.txt"
old_hash="$(head_hash)"

write_file "file.txt" "amended content"

gl fold file.txt HEAD
assert_head_msg "First commit" "fold_amend_head"
assert_ne "$old_hash" "$(head_hash)" "fold_amend_head"
assert_file_content "file.txt" "amended content" "fold_amend_head"

describe "fold a file into a non-HEAD commit"
setup_repo_with_remote
commit_file "Base commit" "base.txt"
target_hash="$(head_hash)"
commit_file "Top commit"  "top.txt"

write_file "base.txt" "amended base"

gl fold base.txt "$target_hash"
assert_head_msg "Top commit" "fold_amend_non_head"
assert_file_content "base.txt" "amended base" "fold_amend_non_head"

describe "fold a commit into another (fixup / move commit)"
setup_repo_with_remote
commit_file "Target commit" "target.txt"
target_hash="$(head_hash)"
commit_file "Source commit" "source.txt"
source_hash="$(head_hash)"

gl fold "$source_hash" "$target_hash"
assert_commit_not_in_log "$source_hash" "fold_commit_into_commit"
assert_log_contains "Target commit" "fold_commit_into_commit"

describe "fold with a nonexistent file exits with error"
setup_repo_with_remote
commit_file "Clean commit" "clean.txt"

gl_capture fold nonexistent.txt HEAD
assert_exit_fail "$CODE" "fold_nothing_to_fold"

pass
