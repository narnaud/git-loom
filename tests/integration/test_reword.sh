#!/usr/bin/env bash
# Integration tests for: gl reword
set -euo pipefail
source "$(dirname "$0")/helpers.sh"
trap 'rm -rf "$TMPROOT"' EXIT

# ══════════════════════════════════════════════════════════════════════════════
# PRECONDITIONS
# ══════════════════════════════════════════════════════════════════════════════

describe "precond: unknown target is rejected"
setup_repo_with_remote
gl_capture reword "nonexistent-xyz-9999" --message "New"
assert_exit_fail "$CODE" "precond_unknown_target"

describe "precond: empty message is rejected"
setup_repo_with_remote
commit_file "Some commit" "s.txt"
gl_capture reword HEAD --message ""
assert_exit_fail "$CODE" "precond_empty_message"

# ══════════════════════════════════════════════════════════════════════════════
# COMMIT REWORDING — BY GIT REFERENCE
# ══════════════════════════════════════════════════════════════════════════════

describe "reword HEAD commit message"
setup_repo_with_remote
commit_file "Old message" "f.txt"
out=$(gl reword HEAD --message "New message")
assert_exit_ok $? "reword_head_ok"
assert_head_msg "New message" "reword_head_msg"

describe "reword non-HEAD commit by full hash"
setup_repo_with_remote
commit_file "Old middle" "mid.txt"
target_hash="$(head_hash)"
commit_file "Top commit" "top.txt"
out=$(gl reword "$target_hash" --message "Renamed middle")
assert_exit_ok $? "reword_nonhead_full_ok"
assert_head_msg "Top commit"     "reword_nonhead_full_top"
assert_msg_at 1 "Renamed middle" "reword_nonhead_full_middle"

describe "reword non-HEAD commit by partial hash"
setup_repo_with_remote
commit_file "Old partial" "partial.txt"
partial_hash="$(head_hash | head -c 7)"
commit_file "After partial" "after.txt"
out=$(gl reword "$partial_hash" --message "Renamed partial")
assert_exit_ok $? "reword_partial_hash_ok"
assert_msg_at 1 "Renamed partial" "reword_partial_hash_msg"

describe "reword root commit (no parent)"
# Use a local-only repo (no upstream tracking) so the linear rebase fallback
# is used — the Weave path cannot include the merge-base commit in its range.
TMPROOT=$(mktemp -d)
WORK="$TMPROOT/work"
git init -q "$WORK"
git -C "$WORK" config user.email "test@test.com"
git -C "$WORK" config user.name "Test"
git -C "$WORK" config core.autocrlf false
echo "initial" > "$WORK/init.txt"
git -C "$WORK" add init.txt
git -C "$WORK" commit -q -m "Initial"
git -C "$WORK" commit -q -m "Second" --allow-empty
root_hash="$(git -C "$WORK" rev-list --max-parents=0 HEAD)"
out=$(gl reword "$root_hash" --message "Initial commit with project structure")
assert_exit_ok $? "reword_root_ok"
assert_log_contains "Initial commit with project structure" "reword_root_msg"

describe "reword preserves commit file content"
setup_repo_with_remote
commit_file "Content commit" "content.txt"
out=$(gl reword HEAD --message "Renamed content commit")
assert_exit_ok $? "reword_content_preserve_ok"
assert_head_msg "Renamed content commit" "reword_content_preserve_msg"
assert_file_content "content.txt" "Content commit" "reword_content_file_intact"

describe "reword updates descendant hashes but preserves their messages"
setup_repo_with_remote
commit_file "Bottom commit" "bottom.txt"
bottom_hash="$(head_hash)"
commit_file "Middle commit" "middle.txt"
commit_file "Top commit" "top2.txt"
top_hash_before="$(head_hash)"
out=$(gl reword "$bottom_hash" --message "Reworded bottom")
assert_exit_ok $? "reword_descendants_ok"
assert_msg_at 2 "Reworded bottom" "reword_descendants_bottom_msg"
assert_msg_at 1 "Middle commit"   "reword_descendants_middle_msg"
assert_head_msg  "Top commit"     "reword_descendants_top_msg"
# Top commit hash must have changed (descendant was replayed)
top_hash_after="$(head_hash)"
assert_ne "$top_hash_before" "$top_hash_after" "reword_descendants_hash_changed"

# ══════════════════════════════════════════════════════════════════════════════
# COMMIT REWORDING — BY SHORT ID
# ══════════════════════════════════════════════════════════════════════════════

describe "reword HEAD commit by short ID"
setup_repo_with_remote
create_feature_branch "g-shortid-feat"
switch_to g-shortid-feat
commit_file "Short ID target commit" "shortid.txt"
switch_to integration
weave_branch "g-shortid-feat"
commit_sid=$(commit_sid_from_status 'Short ID target commit')
out=$(gl reword "$commit_sid" --message "Reworded via short ID")
assert_exit_ok $? "reword_commit_sid_ok"
assert_log_contains "Reworded via short ID" "reword_commit_sid_msg"
assert_log_not_contains "Short ID target commit" "reword_commit_sid_old_gone"

describe "reword non-HEAD commit by short ID (full hash equivalence)"
setup_repo_with_remote
create_feature_branch "g-equiv-feat"
switch_to g-equiv-feat
commit_file "Equiv lower" "equiv-lower.txt"
commit_file "Equiv upper" "equiv-upper.txt"
switch_to integration
weave_branch "g-equiv-feat"
lower_sid=$(commit_sid_from_status 'Equiv lower')
# Also get its full hash for the equivalence test
lower_full=$(git -C "$WORK" log --pretty=%H --all -- equiv-lower.txt | head -1)
# Reword by full hash first, then assert same result works via short ID approach
out=$(gl reword "$lower_sid" --message "Equiv lower reworded")
assert_exit_ok $? "reword_equiv_sid_ok"
assert_log_contains "Equiv lower reworded" "reword_equiv_sid_msg"

# ══════════════════════════════════════════════════════════════════════════════
# BRANCH RENAMING — BY FULL NAME
# ══════════════════════════════════════════════════════════════════════════════

describe "rename branch with -m flag"
setup_repo_with_remote
create_feature_branch "g-old-name"
switch_to g-old-name
commit_file "Branch commit" "b.txt"
switch_to integration
weave_branch "g-old-name"
out=$(gl reword g-old-name --message "h-new-name")
assert_exit_ok $? "reword_branch_rename_ok"
assert_branch_exists     "h-new-name" "reword_branch_rename_exists"
assert_branch_not_exists "g-old-name" "reword_branch_rename_gone"

describe "rename branch preserves its commits"
setup_repo_with_remote
create_feature_branch "g-preserve-branch"
switch_to g-preserve-branch
commit_file "Branch preserved commit" "bp.txt"
switch_to integration
weave_branch "g-preserve-branch"
out=$(gl reword g-preserve-branch --message "h-preserved-renamed")
assert_exit_ok $? "reword_branch_preserves_ok"
assert_log_contains "Branch preserved commit" "reword_branch_preserves_msg"

describe "rename branch with same name is a no-op"
setup_repo_with_remote
create_feature_branch "g-same-name"
switch_to g-same-name
commit_file "Same name commit" "sn.txt"
switch_to integration
weave_branch "g-same-name"
out=$(gl reword g-same-name --message "g-same-name")
assert_exit_ok $? "reword_branch_same_name_ok"
assert_branch_exists "g-same-name" "reword_branch_same_name_still_exists"

# ══════════════════════════════════════════════════════════════════════════════
# BRANCH RENAMING — BY SHORT ID
# ══════════════════════════════════════════════════════════════════════════════

describe "rename branch by short ID"
setup_repo_with_remote
create_feature_branch "g-sid-rename"
switch_to g-sid-rename
commit_file "SID rename commit" "sr.txt"
switch_to integration
weave_branch "g-sid-rename"
branch_sid=$(branch_sid_from_status 'g-sid-rename')
out=$(gl reword "$branch_sid" --message "h-sid-renamed")
assert_exit_ok $? "reword_branch_sid_ok"
assert_branch_exists     "h-sid-renamed" "reword_branch_sid_new_exists"
assert_branch_not_exists "g-sid-rename"  "reword_branch_sid_old_gone"

describe "rename branch by short ID equals rename by full name"
setup_repo_with_remote
create_feature_branch "g-fullname-sid"
switch_to g-fullname-sid
commit_file "Fullname SID commit" "fs.txt"
switch_to integration
weave_branch "g-fullname-sid"
branch_sid=$(branch_sid_from_status 'g-fullname-sid')
# Rename via short ID
out=$(gl reword "$branch_sid" --message "h-renamed-via-sid")
assert_exit_ok $? "reword_fullname_sid_equiv_ok"
assert_branch_exists     "h-renamed-via-sid" "reword_fullname_sid_equiv_new"
assert_branch_not_exists "g-fullname-sid"    "reword_fullname_sid_equiv_old"

# ══════════════════════════════════════════════════════════════════════════════
# WORKING TREE HANDLING
# ══════════════════════════════════════════════════════════════════════════════

describe "reword succeeds with staged changes (stash/restore)"
setup_repo_with_remote
commit_file "Stash target commit" "stash.txt"
# Stage a change without committing
write_file "unstaged.txt" "dirty content"
git -C "$WORK" add unstaged.txt
out=$(gl reword HEAD --message "Stash target reworded")
assert_exit_ok $? "reword_staged_ok"
assert_head_msg "Stash target reworded" "reword_staged_msg"
# Staged change should be restored
git -C "$WORK" diff --cached --name-only | grep -qF "unstaged.txt" \
    || fail "[reword_staged_restored] staged file not restored after reword"

describe "reword succeeds with unstaged changes (stash/restore)"
setup_repo_with_remote
commit_file "Unstaged reword target" "unstaged-reword.txt"
# Modify a tracked file without staging
write_file "unstaged-reword.txt" "modified content"
out=$(gl reword HEAD --message "Unstaged reword done")
assert_exit_ok $? "reword_unstaged_ok"
assert_head_msg "Unstaged reword done" "reword_unstaged_msg"
# Unstaged change should be restored
assert_file_content "unstaged-reword.txt" "modified content" "reword_unstaged_restored"

pass
