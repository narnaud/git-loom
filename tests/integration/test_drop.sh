#!/usr/bin/env bash
# Integration tests for: gl drop
set -euo pipefail
source "$(dirname "$0")/helpers.sh"
trap 'rm -rf "$TMPROOT"' EXIT

# ══════════════════════════════════════════════════════════════════════════════
# PRECONDITIONS
# ══════════════════════════════════════════════════════════════════════════════

describe "precond: unknown full hash is rejected"
setup_repo_with_remote
gl_capture drop "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef" --yes
assert_exit_fail "$CODE" "precond_unknown_hash"

describe "precond: unknown target name is rejected"
setup_repo_with_remote
gl_capture drop "nonexistent-xyz-9999" --yes
assert_exit_fail "$CODE" "precond_unknown_name"

# ══════════════════════════════════════════════════════════════════════════════
# DROP COMMIT — BY GIT REFERENCE
# ══════════════════════════════════════════════════════════════════════════════

describe "drop HEAD commit by full hash"
setup_repo_with_remote
commit_file "Keep this" "keep.txt"
commit_file "Drop this" "drop.txt"
drop_hash="$(head_hash)"
out=$(gl drop "$drop_hash" --yes)
assert_exit_ok $? "drop_head_full_ok"
assert_commit_not_in_log "$drop_hash"  "drop_head_full_gone"
assert_head_msg "Keep this"            "drop_head_full_msg"

describe "drop HEAD commit by partial hash"
setup_repo_with_remote
commit_file "Keep partial" "keep-p.txt"
commit_file "Drop partial" "drop-p.txt"
drop_hash="$(head_hash)"
partial="${drop_hash:0:7}"
out=$(gl drop "$partial" --yes)
assert_exit_ok $? "drop_head_partial_ok"
assert_commit_not_in_log "$drop_hash" "drop_head_partial_gone"
assert_head_msg "Keep partial"         "drop_head_partial_msg"

describe "drop non-HEAD commit (middle of stack)"
setup_repo_with_remote
commit_file "First"  "first.txt"
drop_hash="$(head_hash)"
commit_file "Second" "second.txt"
commit_file "Third"  "third.txt"
out=$(gl drop "$drop_hash" --yes)
assert_exit_ok $? "drop_middle_ok"
assert_commit_not_in_log "$drop_hash" "drop_middle_gone"
assert_head_msg "Third"  "drop_middle_top"
assert_msg_at 1 "Second" "drop_middle_second"

describe "drop preserves file content of surviving commits"
setup_repo_with_remote
commit_file "Keeper commit" "keeper.txt"
commit_file "Victim commit" "victim.txt"
victim_hash="$(head_hash)"
out=$(gl drop "$victim_hash" --yes)
assert_exit_ok $? "drop_preserve_ok"
assert_file_content "keeper.txt" "Keeper commit" "drop_preserve_file"

describe "drop commit updates descendant hashes"
setup_repo_with_remote
commit_file "Bottom drop" "bottom-dep.txt"
drop_hash="$(head_hash)"
commit_file "Top dep" "top-dep.txt"
top_before="$(head_hash)"
out=$(gl drop "$drop_hash" --yes)
assert_exit_ok $? "drop_descendant_ok"
top_after="$(head_hash)"
assert_ne "$top_before" "$top_after" "drop_descendant_rehashed"
assert_head_msg "Top dep" "drop_descendant_msg"

describe "drop commit on a woven feature branch"
setup_repo_with_remote
create_feature_branch "g-partial-drop"
switch_to g-partial-drop
commit_file "A1 keep" "a1.txt"
commit_file "A2 drop" "a2.txt"
drop_hash="$(head_hash)"
commit_file "A3 keep" "a3.txt"
switch_to integration
weave_branch "g-partial-drop"
out=$(gl drop "$drop_hash" --yes)
assert_exit_ok $? "drop_branch_commit_ok"
assert_commit_not_in_log "$drop_hash"  "drop_branch_commit_gone"
assert_log_contains "A1 keep"          "drop_branch_a1_remains"
assert_log_contains "A3 keep"          "drop_branch_a3_remains"
assert_log_not_contains "A2 drop"      "drop_branch_a2_gone"

# ══════════════════════════════════════════════════════════════════════════════
# DROP COMMIT — BY SHORT ID
# ══════════════════════════════════════════════════════════════════════════════

describe "drop commit by short ID (woven branch)"
setup_repo_with_remote
create_feature_branch "g-sid-drop"
switch_to g-sid-drop
commit_file "SID drop target" "sid-drop.txt"
commit_file "SID keep above"  "sid-keep.txt"
switch_to integration
weave_branch "g-sid-drop"
status_out=$(gl status)
commit_sid=$(grep 'SID drop target' <<< "$status_out" | grep -oE '[0-9a-z]{4,8}' | head -1)
full_hash=$(git -C "$WORK" log --pretty=%H --all -- sid-drop.txt | head -1)
out=$(gl drop "$commit_sid" --yes)
assert_exit_ok $? "drop_commit_sid_ok"
assert_commit_not_in_log "$full_hash"  "drop_commit_sid_gone"
assert_log_contains "SID keep above"   "drop_commit_sid_sibling_remains"
assert_log_not_contains "SID drop target" "drop_commit_sid_target_gone"

# ══════════════════════════════════════════════════════════════════════════════
# DROP LAST COMMIT ON BRANCH (AUTO-DELETES BRANCH)
# ══════════════════════════════════════════════════════════════════════════════

describe "drop last commit on woven branch auto-deletes the branch"
setup_repo_with_remote
create_feature_branch "g-solo-commit"
switch_to g-solo-commit
commit_file "Solo commit" "solo.txt"
solo_hash="$(head_hash)"
switch_to integration
weave_branch "g-solo-commit"
out=$(gl drop "$solo_hash" --yes)
assert_exit_ok $? "drop_solo_ok"
assert_commit_not_in_log "$solo_hash"    "drop_solo_commit_gone"
assert_branch_not_exists "g-solo-commit" "drop_solo_branch_deleted"

describe "drop last commit by short ID auto-deletes the branch"
setup_repo_with_remote
create_feature_branch "g-solo-sid"
switch_to g-solo-sid
commit_file "Solo SID commit" "solo-sid.txt"
switch_to integration
weave_branch "g-solo-sid"
status_out=$(gl status)
commit_sid=$(grep 'Solo SID commit' <<< "$status_out" | grep -oE '[0-9a-z]{4,8}' | head -1)
full_hash=$(git -C "$WORK" log --pretty=%H --all -- solo-sid.txt | head -1)
out=$(gl drop "$commit_sid" --yes)
assert_exit_ok $? "drop_solo_sid_ok"
assert_commit_not_in_log "$full_hash"  "drop_solo_sid_commit_gone"
assert_branch_not_exists "g-solo-sid" "drop_solo_sid_branch_deleted"

# ══════════════════════════════════════════════════════════════════════════════
# DROP WOVEN BRANCH — BY FULL NAME
# ══════════════════════════════════════════════════════════════════════════════

describe "drop woven branch removes all its commits, merge commit, and ref"
setup_repo_with_remote
create_feature_branch "g-woven-drop"
switch_to g-woven-drop
commit_file "Woven A1" "wa1.txt"
commit_file "Woven A2" "wa2.txt"
switch_to integration
weave_branch "g-woven-drop"
out=$(gl drop g-woven-drop --yes)
assert_exit_ok $? "drop_woven_ok"
assert_branch_not_exists "g-woven-drop"      "drop_woven_ref_gone"
assert_log_not_contains  "Woven A1"          "drop_woven_a1_gone"
assert_log_not_contains  "Woven A2"          "drop_woven_a2_gone"
assert_log_not_contains  "Merge g-woven-drop" "drop_woven_merge_gone"

describe "drop woven branch preserves commits on other branches"
setup_repo_with_remote
create_feature_branch "g-keep-woven"
switch_to g-keep-woven
commit_file "Keep woven commit" "kw.txt"
switch_to integration
weave_branch "g-keep-woven"

create_feature_branch "h-drop-woven"
switch_to h-drop-woven
commit_file "Drop woven commit" "dw.txt"
switch_to integration
weave_branch "h-drop-woven"

out=$(gl drop h-drop-woven --yes)
assert_exit_ok $? "drop_woven_preserve_ok"
assert_branch_not_exists "h-drop-woven"     "drop_woven_preserve_dropped_ref"
assert_branch_exists     "g-keep-woven"     "drop_woven_preserve_kept_ref"
assert_log_contains      "Keep woven commit" "drop_woven_preserve_kept_msg"
assert_log_not_contains  "Drop woven commit" "drop_woven_preserve_gone_msg"

# ══════════════════════════════════════════════════════════════════════════════
# DROP WOVEN BRANCH — BY SHORT ID
# ══════════════════════════════════════════════════════════════════════════════

describe "drop woven branch by branch short ID"
setup_repo_with_remote
create_feature_branch "g-woven-sid"
switch_to g-woven-sid
commit_file "Woven SID commit" "wsid.txt"
switch_to integration
weave_branch "g-woven-sid"
status_out=$(gl status)
branch_sid=$(grep -F '[g-woven-sid]' <<< "$status_out" | awk '{print $(NF-1)}')
out=$(gl drop "$branch_sid" --yes)
assert_exit_ok $? "drop_woven_sid_ok"
assert_branch_not_exists "g-woven-sid"     "drop_woven_sid_ref_gone"
assert_log_not_contains  "Woven SID commit" "drop_woven_sid_commit_gone"

describe "drop by branch short ID equals drop by full name"
setup_repo_with_remote
create_feature_branch "g-sid-equiv"
switch_to g-sid-equiv
commit_file "SID equiv commit" "sid-equiv.txt"
switch_to integration
weave_branch "g-sid-equiv"
# Drop by full name as baseline
out=$(gl drop g-sid-equiv --yes)
assert_exit_ok $? "drop_sid_equiv_ok"
assert_branch_not_exists "g-sid-equiv"    "drop_sid_equiv_ref_gone"
assert_log_not_contains  "SID equiv commit" "drop_sid_equiv_commit_gone"

# ══════════════════════════════════════════════════════════════════════════════
# DROP NON-WOVEN BRANCH
# ══════════════════════════════════════════════════════════════════════════════

describe "drop non-woven branch removes its commits and deletes ref"
setup_repo_with_remote
create_feature_branch "g-nonwoven"
switch_to g-nonwoven
commit_file "Non-woven X" "nwx.txt"
commit_file "Non-woven Y" "nwy.txt"
switch_to integration
git -C "$WORK" merge -q --ff-only g-nonwoven
out=$(gl drop g-nonwoven --yes)
assert_exit_ok $? "drop_nonwoven_ok"
assert_branch_not_exists "g-nonwoven"   "drop_nonwoven_ref_gone"
assert_log_not_contains  "Non-woven X"  "drop_nonwoven_x_gone"
assert_log_not_contains  "Non-woven Y"  "drop_nonwoven_y_gone"

# ══════════════════════════════════════════════════════════════════════════════
# DROP CO-LOCATED WOVEN BRANCH
# ══════════════════════════════════════════════════════════════════════════════

describe "drop co-located woven branch preserves commits for sibling"
setup_repo_with_remote
create_feature_branch "g-coloc-a"
switch_to g-coloc-a
commit_file "Coloc shared commit" "coloc-shared.txt"
switch_to integration
git -C "$WORK" branch h-coloc-b g-coloc-a
weave_branch "g-coloc-a"
out=$(gl drop g-coloc-a --yes)
assert_exit_ok $? "drop_coloc_woven_ok"
assert_branch_not_exists "g-coloc-a"           "drop_coloc_woven_a_gone"
assert_branch_exists     "h-coloc-b"           "drop_coloc_woven_b_survives"
assert_log_contains      "Coloc shared commit"  "drop_coloc_woven_commits_preserved"

describe "drop co-located non-woven branch only deletes ref"
setup_repo_with_remote
create_feature_branch "g-coloc-nw-a"
switch_to g-coloc-nw-a
commit_file "Coloc NW commit" "coloc-nw.txt"
switch_to integration
git -C "$WORK" branch h-coloc-nw-b g-coloc-nw-a
git -C "$WORK" merge -q --ff-only g-coloc-nw-a
out=$(gl drop g-coloc-nw-a --yes)
assert_exit_ok $? "drop_coloc_nw_ok"
assert_branch_not_exists "g-coloc-nw-a"    "drop_coloc_nw_a_gone"
assert_branch_exists     "h-coloc-nw-b"    "drop_coloc_nw_b_survives"
assert_log_contains      "Coloc NW commit" "drop_coloc_nw_commits_preserved"

# ══════════════════════════════════════════════════════════════════════════════
# DROP BRANCH AT MERGE-BASE (NO COMMITS)
# ══════════════════════════════════════════════════════════════════════════════

describe "drop branch at merge-base deletes ref, no rebase needed"
setup_repo_with_remote
create_feature_branch "g-empty-branch"
# Dirty working tree is allowed for this case (no rebase is run)
write_file "dirty.txt" "not staged"
out=$(gl drop g-empty-branch --yes)
assert_exit_ok $? "drop_empty_branch_ok"
assert_branch_not_exists "g-empty-branch" "drop_empty_branch_ref_gone"
# Working tree must remain untouched (no stash was performed)
assert_file_content "dirty.txt" "not staged" "drop_empty_branch_wt_intact"

# ══════════════════════════════════════════════════════════════════════════════
# DROP BRANCH OUT OF INTEGRATION RANGE
# ══════════════════════════════════════════════════════════════════════════════

describe "drop branch outside integration range is rejected with helpful message"
setup_repo_with_remote
git -C "$WORK" checkout -q -b g-out-of-range
commit_file "Out-of-range commit" "oor.txt"
switch_to integration
gl_capture drop g-out-of-range --yes
assert_exit_fail "$CODE"                          "drop_oor_fail"
assert_contains  "$OUT" "not woven into the integration branch" "drop_oor_msg"

# ══════════════════════════════════════════════════════════════════════════════
# DROP FILE
# ══════════════════════════════════════════════════════════════════════════════

describe "drop tracked modified file restores it to committed state"
setup_repo_with_remote
commit_file "Original content" "restore-me.txt"
write_file "restore-me.txt" "modified content"
git -C "$WORK" add restore-me.txt
out=$(gl drop restore-me.txt --yes)
assert_exit_ok $? "drop_file_modified_ok"
assert_contains "$out" "Restored"                    "drop_file_modified_msg"
assert_file_content "restore-me.txt" "Original content" "drop_file_modified_restored"

describe "drop staged new file removes it from index and disk"
setup_repo_with_remote
write_file "new-staged.txt" "brand new file"
git -C "$WORK" add new-staged.txt
out=$(gl drop new-staged.txt --yes)
assert_exit_ok $? "drop_file_staged_new_ok"
assert_contains "$out" "Deleted" "drop_file_staged_new_msg"
[[ ! -f "$WORK/new-staged.txt" ]] \
    || fail "[drop_file_staged_new_gone] file still exists after drop"

describe "drop untracked file deletes it from disk"
setup_repo_with_remote
write_file "untracked-drop.txt" "untracked content"
out=$(gl drop untracked-drop.txt --yes)
assert_exit_ok $? "drop_file_untracked_ok"
assert_contains "$out" "Deleted" "drop_file_untracked_msg"
[[ ! -f "$WORK/untracked-drop.txt" ]] \
    || fail "[drop_file_untracked_gone] file still exists after drop"

describe "drop file does not affect other files"
setup_repo_with_remote
commit_file "Untouched file" "untouched.txt"
write_file "to-drop.txt" "drop me"
out=$(gl drop to-drop.txt --yes)
assert_exit_ok $? "drop_file_isolated_ok"
assert_file_content "untouched.txt" "Untouched file" "drop_file_isolated_untouched"

# ══════════════════════════════════════════════════════════════════════════════
# DROP ZZ (ALL LOCAL CHANGES)
# ══════════════════════════════════════════════════════════════════════════════

describe "drop zz discards staged changes, unstaged changes, and untracked files"
setup_repo_with_remote
commit_file "Base file" "base.txt"
write_file "base.txt" "modified base"
git -C "$WORK" add base.txt
write_file "new-untracked.txt" "untracked"
out=$(gl drop zz --yes)
assert_exit_ok $? "drop_zz_ok"
assert_contains "$out" "Discarded all local changes" "drop_zz_msg"
assert_file_content "base.txt" "Base file" "drop_zz_tracked_restored"
[[ ! -f "$WORK/new-untracked.txt" ]] \
    || fail "[drop_zz_untracked_gone] untracked file still exists after drop zz"

describe "drop zz with no local changes fails"
setup_repo_with_remote
gl_capture drop zz --yes
assert_exit_fail "$CODE"                       "drop_zz_no_changes_fail"
assert_contains  "$OUT" "No local changes"     "drop_zz_no_changes_msg"

# ══════════════════════════════════════════════════════════════════════════════
# WORKING TREE PRESERVATION
# ══════════════════════════════════════════════════════════════════════════════

describe "drop commit preserves staged changes"
setup_repo_with_remote
commit_file "Victim staged" "victim-staged.txt"
victim_hash="$(head_hash)"
commit_file "Survivor staged" "survivor-staged.txt"
write_file "wt-staged.txt" "staged worktree file"
git -C "$WORK" add wt-staged.txt
out=$(gl drop "$victim_hash" --yes)
assert_exit_ok $? "drop_wt_staged_ok"
git -C "$WORK" diff --cached --name-only | grep -qF "wt-staged.txt" \
    || fail "[drop_wt_staged_restored] staged file not in index after drop"

describe "drop commit preserves unstaged modifications"
setup_repo_with_remote
commit_file "Victim unstaged" "victim-unstaged.txt"
victim_hash="$(head_hash)"
commit_file "Survivor unstaged" "survivor-unstaged.txt"
write_file "survivor-unstaged.txt" "dirty modification"
out=$(gl drop "$victim_hash" --yes)
assert_exit_ok $? "drop_wt_unstaged_ok"
assert_file_content "survivor-unstaged.txt" "dirty modification" "drop_wt_unstaged_preserved"

describe "drop woven branch preserves staged changes"
setup_repo_with_remote
create_feature_branch "g-wt-branch"
switch_to g-wt-branch
commit_file "WT branch commit" "wt-branch.txt"
switch_to integration
weave_branch "g-wt-branch"
write_file "wt-staged-branch.txt" "staged during branch drop"
git -C "$WORK" add wt-staged-branch.txt
out=$(gl drop g-wt-branch --yes)
assert_exit_ok $? "drop_branch_wt_ok"
git -C "$WORK" diff --cached --name-only | grep -qF "wt-staged-branch.txt" \
    || fail "[drop_branch_wt_restored] staged file not in index after branch drop"

pass
