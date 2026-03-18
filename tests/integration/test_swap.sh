#!/usr/bin/env bash
# Integration tests for: gl swap
set -euo pipefail
source "$(dirname "$0")/helpers.sh"
trap 'rm -rf "$TMPROOT"' EXIT

# ══════════════════════════════════════════════════════════════════════════════
# PRECONDITIONS
# ══════════════════════════════════════════════════════════════════════════════

describe "precond: not in a git repository"
TMP_NOGIT=$(mktemp -d)
CODE=0
(cd "$TMP_NOGIT" && NO_COLOR=1 GIT_TERMINAL_PROMPT=0 "$GL_BIN" swap a b >/dev/null 2>&1) || CODE=$?
assert_exit_fail "$CODE" "precond_not_git_repo"
rm -rf "$TMP_NOGIT"

describe "precond: missing arguments"
setup_repo_with_remote
gl_capture swap
assert_exit_fail "$CODE" "precond_no_args"

describe "precond: only one argument"
setup_repo_with_remote
commit_file "Only commit" "only.txt"
hash_only=$(head_hash)
gl_capture swap "$hash_only"
assert_exit_fail "$CODE" "precond_one_arg"

describe "precond: commit not in weave graph"
setup_repo_with_remote
commit_file "Loose A" "loose-a.txt"
hash_a=$(head_hash)
commit_file "Loose B" "loose-b.txt"
hash_b=$(head_hash)
# Make a commit that is NOT on integration (orphan-ish via a temp branch)
git -C "$WORK" checkout -q -b g-orphan
commit_file "Orphan commit" "orphan.txt"
orphan_hash=$(head_hash)
switch_to integration
gl_capture swap "$hash_a" "$orphan_hash"
assert_exit_fail "$CODE" "precond_commit_not_in_graph"
assert_contains "$OUT" "not found in weave graph" "precond_commit_not_in_graph_msg"

describe "precond: swap commit with itself"
setup_repo_with_remote
create_feature_branch "g-self"
switch_to g-self
commit_file "Self commit" "self.txt"
self_hash=$(head_hash)
switch_to integration
weave_branch "g-self"
gl_capture swap "$self_hash" "$self_hash"
assert_exit_fail "$CODE" "precond_same_commit"
assert_contains "$OUT" "Cannot swap a commit with itself" "precond_same_commit_msg"

describe "precond: commits from different branch sections"
setup_repo_with_remote
create_feature_branch "g-sect-a"
switch_to g-sect-a
commit_file "Section A commit" "sect-a.txt"
hash_sect_a=$(head_hash)
switch_to integration
weave_branch "g-sect-a"

create_feature_branch "h-sect-b"
switch_to h-sect-b
commit_file "Section B commit" "sect-b.txt"
hash_sect_b=$(head_hash)
switch_to integration
weave_branch "h-sect-b"

gl_capture swap "$hash_sect_a" "$hash_sect_b"
assert_exit_fail "$CODE" "precond_diff_sections"
assert_contains "$OUT" "Cannot swap commits from different branch sections" "precond_diff_sections_msg"

describe "precond: one commit in branch section, other on integration line"
setup_repo_with_remote
commit_file "Loose commit" "loose.txt"
hash_loose=$(head_hash)
create_feature_branch "g-in-branch"
switch_to g-in-branch
commit_file "Branch commit" "branch.txt"
hash_branch=$(head_hash)
switch_to integration
weave_branch "g-in-branch"
gl_capture swap "$hash_loose" "$hash_branch"
assert_exit_fail "$CODE" "precond_diff_locations"
assert_contains "$OUT" "Cannot swap commits from different locations" "precond_diff_locations_msg"


# ══════════════════════════════════════════════════════════════════════════════
# SWAP TWO COMMITS (LOOSE COMMITS ON INTEGRATION LINE)
# ══════════════════════════════════════════════════════════════════════════════

describe "swap two loose commits by full hash — order is reversed"
setup_repo_with_remote
commit_file "First loose" "first-loose.txt"
hash_first=$(head_hash)
commit_file "Second loose" "second-loose.txt"
hash_second=$(head_hash)
# Before: HEAD=Second, HEAD~1=First
assert_msg_at 0 "Second loose" "loose_before_head"
assert_msg_at 1 "First loose"  "loose_before_head1"

out=$(gl swap "$hash_first" "$hash_second")
assert_exit_ok $? "loose_swap_ok"
assert_contains "$out" "Swapped commits" "loose_swap_success_msg"

# After: HEAD=First, HEAD~1=Second
assert_msg_at 0 "First loose"  "loose_after_head"
assert_msg_at 1 "Second loose" "loose_after_head1"

describe "swap two loose commits by short ID — order is reversed"
setup_repo_with_remote
commit_file "Ping commit" "ping.txt"
commit_file "Pong commit" "pong.txt"

sid_ping=$(commit_sid_from_status "Ping commit")
sid_pong=$(commit_sid_from_status "Pong commit")

assert_msg_at 0 "Pong commit" "sid_loose_before_head"
assert_msg_at 1 "Ping commit" "sid_loose_before_head1"

out=$(gl swap "$sid_ping" "$sid_pong")
assert_exit_ok $? "sid_loose_swap_ok"
assert_contains "$out" "Swapped commits" "sid_loose_swap_msg"

assert_msg_at 0 "Ping commit" "sid_loose_after_head"
assert_msg_at 1 "Pong commit" "sid_loose_after_head1"

# ══════════════════════════════════════════════════════════════════════════════
# SWAP TWO COMMITS (WITHIN A BRANCH SECTION)
# ══════════════════════════════════════════════════════════════════════════════

describe "swap two commits within a branch section by full hash — order is reversed"
setup_repo_with_remote
create_feature_branch "g-two-commits"
switch_to g-two-commits
commit_file "Branch first" "br-first.txt"
hash_br_first=$(head_hash)
commit_file "Branch second" "br-second.txt"
hash_br_second=$(head_hash)
switch_to integration
weave_branch "g-two-commits"

# Branch tip order before: second is newer
branch_log_before=$(git -C "$WORK" log g-two-commits --oneline)
assert_contains "$branch_log_before" "Branch second" "branch_before_second_in_log"
assert_contains "$branch_log_before" "Branch first"  "branch_before_first_in_log"

out=$(gl swap "$hash_br_first" "$hash_br_second")
assert_exit_ok $? "branch_commit_swap_ok"
assert_contains "$out" "Swapped commits" "branch_commit_swap_msg"

# After: first commit is now the branch tip
branch_log_after=$(git -C "$WORK" log g-two-commits --oneline)
assert_contains "$branch_log_after" "Branch first"  "branch_after_first_in_log"
assert_contains "$branch_log_after" "Branch second" "branch_after_second_in_log"
# Verify "Branch first" is at tip (newer) now
tip_msg=$(git -C "$WORK" log -1 --pretty=%s g-two-commits)
assert_eq "$tip_msg" "Branch first" "branch_after_tip_is_first"

describe "swap two commits in a branch section by short ID"
setup_repo_with_remote
create_feature_branch "g-sid-branch"
switch_to g-sid-branch
commit_file "Red commit" "red.txt"
commit_file "Blue commit" "blue.txt"
switch_to integration
weave_branch "g-sid-branch"

sid_red=$(commit_sid_from_status "Red commit")
sid_blue=$(commit_sid_from_status "Blue commit")

out=$(gl swap "$sid_red" "$sid_blue")
assert_exit_ok $? "branch_sid_swap_ok"
assert_contains "$out" "Swapped commits" "branch_sid_swap_msg"

tip_msg_sid=$(git -C "$WORK" log -1 --pretty=%s g-sid-branch)
assert_eq "$tip_msg_sid" "Red commit" "branch_sid_tip_is_red"

describe "swap commits preserves file content in each commit"
setup_repo_with_remote
create_feature_branch "g-content"
switch_to g-content
echo "content of apple" > "$WORK/apple.txt"
git -C "$WORK" add apple.txt
git -C "$WORK" commit -q -m "Add apple"
hash_apple=$(head_hash)

echo "content of banana" > "$WORK/banana.txt"
git -C "$WORK" add banana.txt
git -C "$WORK" commit -q -m "Add banana"
hash_banana=$(head_hash)
switch_to integration
weave_branch "g-content"

out=$(gl swap "$hash_apple" "$hash_banana")
assert_exit_ok $? "content_swap_ok"

# Files should both still exist with correct content
assert_file_content "apple.txt"  "content of apple"  "content_apple_preserved"
assert_file_content "banana.txt" "content of banana" "content_banana_preserved"


# ══════════════════════════════════════════════════════════════════════════════
# CONTINUE / ABORT
# ══════════════════════════════════════════════════════════════════════════════
# Both commits modify the same line of shared.txt, so swapping them causes
# two consecutive conflicts: first when replaying B (from A→from B) onto
# base, then when replaying A (base→from A) onto the resolved result.

describe "swap: conflict pauses with state file, resolve, continue → success"
setup_repo_with_remote
git -C "$WORK" config rerere.enabled false
echo "base" > "$WORK/shared.txt"
git -C "$WORK" add shared.txt
git -C "$WORK" commit -q -m "Add shared file"

# Commit A: change shared.txt to "from A"
echo "from A" > "$WORK/shared.txt"
git -C "$WORK" add shared.txt
git -C "$WORK" commit -q -m "Change to A"
hash_change_a=$(head_hash)

# Commit B: change shared.txt to "from B" (applied on top of A)
echo "from B" > "$WORK/shared.txt"
git -C "$WORK" add shared.txt
git -C "$WORK" commit -q -m "Change to B"
hash_change_b=$(head_hash)

old_head=$(head_hash)
gl_capture swap "$hash_change_a" "$hash_change_b"
assert_exit_ok  "$CODE" "cont_conflict_paused"
assert_state_file            "cont_state_file_exists"
assert_contains "$OUT" "loom continue" "cont_hint_continue"
assert_contains "$OUT" "loom abort"    "cont_hint_abort"

# Conflict: B (from A→from B) applied onto base. Resolve to "base" (keep ours)
# so cherry-picking A (base→from A) onto "base" is a clean forward application.
echo "base" > "$WORK/shared.txt"
git -C "$WORK" add shared.txt

out=$(gl continue)
assert_exit_ok $? "cont_continue_ok"
assert_no_state_file   "cont_state_removed"
assert_contains "$out" "Swapped commits" "cont_success_msg"

describe "swap: conflict pauses, abort → original HEAD restored"
setup_repo_with_remote
git -C "$WORK" config rerere.enabled false
echo "base" > "$WORK/shared.txt"
git -C "$WORK" add shared.txt
git -C "$WORK" commit -q -m "Add shared file"

echo "from X" > "$WORK/shared.txt"
git -C "$WORK" add shared.txt
git -C "$WORK" commit -q -m "Change to X"
hash_change_x=$(head_hash)

echo "from Y" > "$WORK/shared.txt"
git -C "$WORK" add shared.txt
git -C "$WORK" commit -q -m "Change to Y"
hash_change_y=$(head_hash)

old_head=$(head_hash)
gl_capture swap "$hash_change_x" "$hash_change_y"
assert_exit_ok  "$CODE" "abort_conflict_paused"
assert_state_file            "abort_state_file_exists"

gl_capture abort
assert_exit_ok  "$CODE" "abort_ok"
assert_contains "$OUT" "Aborted"  "abort_msg"
assert_no_state_file   "abort_state_removed"
assert_eq "$old_head" "$(head_hash)" "abort_head_restored"

pass
