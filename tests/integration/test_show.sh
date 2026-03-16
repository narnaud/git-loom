#!/usr/bin/env bash
# Integration tests for: gl show
set -euo pipefail
source "$(dirname "$0")/helpers.sh"
trap 'rm -rf "$TMPROOT"' EXIT

# ══════════════════════════════════════════════════════════════════════════════
# PRECONDITIONS
# ══════════════════════════════════════════════════════════════════════════════

describe "precond: unknown target is rejected"
setup_repo_with_remote
gl_capture show "nonexistent-xyz-9999"
assert_exit_fail "$CODE" "precond_unknown_target"

describe "precond: unknown hash is rejected"
setup_repo_with_remote
gl_capture show "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
assert_exit_fail "$CODE" "precond_unknown_hash"

# ══════════════════════════════════════════════════════════════════════════════
# SHOW BY GIT REFERENCE
# ══════════════════════════════════════════════════════════════════════════════

describe "show HEAD commit by full hash shows message and diff"
setup_repo_with_remote
commit_file "Show full hash commit" "showme.txt"
hash="$(head_hash)"
out=$(gl show "$hash")
assert_exit_ok $? "show_full_hash_ok"
assert_contains "$out" "Show full hash commit" "show_full_hash_msg"
assert_contains "$out" "diff --git"            "show_full_hash_diff"
assert_contains "$out" "showme.txt"            "show_full_hash_file"

describe "show commit by partial hash"
setup_repo_with_remote
commit_file "Partial hash commit" "partial.txt"
hash="$(head_hash)"
partial="${hash:0:7}"
out=$(gl show "$partial")
assert_exit_ok $? "show_partial_hash_ok"
assert_contains "$out" "Partial hash commit" "show_partial_hash_msg"
assert_contains "$out" "partial.txt"         "show_partial_hash_file"

describe "show non-HEAD commit by full hash"
setup_repo_with_remote
commit_file "Bottom commit" "bottom.txt"
bottom_hash="$(head_hash)"
commit_file "Top commit" "top.txt"
out=$(gl show "$bottom_hash")
assert_exit_ok $? "show_nonhead_ok"
assert_contains     "$out" "Bottom commit" "show_nonhead_msg"
assert_not_contains "$out" "Top commit"    "show_nonhead_not_top"

describe "show output includes author metadata"
setup_repo_with_remote
commit_file "Author check commit" "author.txt"
hash="$(head_hash)"
out=$(gl show "$hash")
assert_exit_ok $? "show_author_ok"
assert_contains "$out" "Author:" "show_author_field"

describe "show output includes the added file content in the diff"
setup_repo_with_remote
echo "hello from content" > "$WORK/content.txt"
git -C "$WORK" add content.txt
git -C "$WORK" commit -q -m "Content commit"
hash="$(head_hash)"
out=$(gl show "$hash")
assert_exit_ok $? "show_content_ok"
assert_contains "$out" "hello from content" "show_content_in_diff"

# ══════════════════════════════════════════════════════════════════════════════
# SHOW BRANCH TIP
# ══════════════════════════════════════════════════════════════════════════════

describe "show branch name displays its tip commit"
setup_repo_with_remote
create_feature_branch "g-show-branch"
switch_to g-show-branch
commit_file "Branch tip commit" "branch-tip.txt"
switch_to integration
out=$(gl show g-show-branch)
assert_exit_ok $? "show_branch_ok"
assert_contains "$out" "Branch tip commit" "show_branch_msg"
assert_contains "$out" "branch-tip.txt"    "show_branch_file"

describe "show branch name does not show commits below tip"
setup_repo_with_remote
create_feature_branch "g-show-tip"
switch_to g-show-tip
commit_file "Lower commit" "lower.txt"
commit_file "Tip commit"   "tip.txt"
switch_to integration
out=$(gl show g-show-tip)
assert_exit_ok $? "show_branch_tip_ok"
assert_contains     "$out" "Tip commit"   "show_branch_tip_msg"
assert_not_contains "$out" "Lower commit" "show_branch_tip_not_lower"

describe "show integration branch name displays its HEAD commit"
setup_repo_with_remote
commit_file "Integration HEAD commit" "int-head.txt"
out=$(gl show integration)
assert_exit_ok $? "show_integration_ok"
assert_contains "$out" "Integration HEAD commit" "show_integration_msg"

# ══════════════════════════════════════════════════════════════════════════════
# SHOW BY SHORT ID
# ══════════════════════════════════════════════════════════════════════════════

describe "show commit by short ID"
setup_repo_with_remote
create_feature_branch "g-sid-show"
switch_to g-sid-show
commit_file "Short ID show commit" "sid-show.txt"
switch_to integration
weave_branch "g-sid-show"
commit_sid=$(commit_sid_from_status 'Short ID show commit')
out=$(gl show "$commit_sid")
assert_exit_ok $? "show_commit_sid_ok"
assert_contains "$out" "Short ID show commit" "show_commit_sid_msg"
assert_contains "$out" "sid-show.txt"         "show_commit_sid_file"

describe "show branch by short ID displays its tip commit"
setup_repo_with_remote
create_feature_branch "g-branch-sid"
switch_to g-branch-sid
commit_file "Branch SID tip" "branch-sid.txt"
switch_to integration
weave_branch "g-branch-sid"
branch_sid=$(branch_sid_from_status 'g-branch-sid')
out=$(gl show "$branch_sid")
assert_exit_ok $? "show_branch_sid_ok"
assert_contains "$out" "Branch SID tip" "show_branch_sid_msg"
assert_contains "$out" "branch-sid.txt" "show_branch_sid_file"

describe "show by commit short ID equals show by full hash"
setup_repo_with_remote
create_feature_branch "g-equiv-show"
switch_to g-equiv-show
commit_file "Equiv show commit" "equiv-show.txt"
switch_to integration
weave_branch "g-equiv-show"
full_hash=$(git -C "$WORK" log --pretty=%H --all -- equiv-show.txt | head -1)
commit_sid=$(commit_sid_from_status 'Equiv show commit')
out_full=$(gl show "$full_hash")
out_sid=$(gl show "$commit_sid")
assert_exit_ok $? "show_equiv_sid_ok"
assert_contains "$out_sid" "Equiv show commit" "show_equiv_sid_msg"
assert_contains "$out_sid" "equiv-show.txt"    "show_equiv_sid_file"

# ══════════════════════════════════════════════════════════════════════════════
# ALIAS
# ══════════════════════════════════════════════════════════════════════════════

describe "gl sh alias works identically to gl show"
setup_repo_with_remote
commit_file "Alias test commit" "alias.txt"
hash="$(head_hash)"
out=$(gl sh "$hash")
assert_exit_ok $? "show_alias_ok"
assert_contains "$out" "Alias test commit" "show_alias_msg"
assert_contains "$out" "alias.txt"         "show_alias_file"

pass
