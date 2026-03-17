#!/usr/bin/env bash
# Integration tests for: gl fold (staged / amend / amend-all / fixup / move / uncommit / move-file / create)
set -euo pipefail
source "$(dirname "$0")/helpers.sh"
trap 'rm -rf "$TMPROOT"' EXIT

# ══════════════════════════════════════════════════════════════════════════════
# PRECONDITIONS
# ══════════════════════════════════════════════════════════════════════════════

describe "precond: not in a git repository"
TMP_NOGIT=$(mktemp -d)
CODE=0
(cd "$TMP_NOGIT" && NO_COLOR=1 GIT_TERMINAL_PROMPT=0 "$GL_BIN" fold HEAD >/dev/null 2>&1) || CODE=$?
assert_exit_fail "$CODE" "precond_not_git_repo"
rm -rf "$TMP_NOGIT"

describe "precond: single-arg with nothing staged"
setup_repo_with_remote
commit_file "Clean commit" "clean.txt"
gl_capture fold HEAD
assert_exit_fail "$CODE" "precond_nothing_staged_fail"
assert_contains "$OUT" "Nothing to commit" "precond_nothing_staged_msg"

describe "precond: single-arg with branch as target (non-commit)"
setup_repo_with_remote
commit_file "Staged source" "staged.txt"
write_file "staged.txt" "changed"
git -C "$WORK" add staged.txt
create_feature_branch "g-notcommit"
gl_capture fold g-notcommit
assert_exit_fail "$CODE" "precond_single_branch_fail"
assert_contains "$OUT" "did not resolve to a commit" "precond_single_branch_msg"

describe "precond: file + branch target is rejected"
setup_repo_with_remote
commit_file "Base commit" "base.txt"
write_file "base.txt" "changed"
create_feature_branch "g-file-branch"
gl_capture fold base.txt g-file-branch
assert_exit_fail "$CODE" "precond_file_branch_fail"
assert_contains "$OUT" "Cannot fold files into a branch" "precond_file_branch_msg"

describe "precond: branch as source is rejected"
setup_repo_with_remote
commit_file "Base commit" "base.txt"
create_feature_branch "g-src-branch"
commit_file "Target commit" "target.txt"
target_hash="$(head_hash)"
gl_capture fold g-src-branch "$target_hash"
assert_exit_fail "$CODE" "precond_branch_src_fail"
assert_contains "$OUT" "did not resolve" "precond_branch_src_msg"

describe "precond: zz + branch target is rejected"
setup_repo_with_remote
commit_file "Base commit" "base.txt"
write_file "base.txt" "changed"
create_feature_branch "g-zz-branch"
gl_capture fold zz g-zz-branch
assert_exit_fail "$CODE" "precond_zz_branch_fail"
assert_contains "$OUT" "Cannot fold files into a branch" "precond_zz_branch_msg"

describe "precond: zz with clean working tree"
setup_repo_with_remote
commit_file "Clean commit" "clean.txt"
target_hash="$(head_hash)"
gl_capture fold zz "$target_hash"
assert_exit_fail "$CODE" "precond_zz_clean_fail"
assert_contains "$OUT" "No changes to fold" "precond_zz_clean_msg"

describe "precond: multiple commit sources are rejected"
setup_repo_with_remote
commit_file "Commit A" "a.txt"
hash_a="$(head_hash)"
commit_file "Commit B" "b.txt"
hash_b="$(head_hash)"
commit_file "Commit C" "c.txt"
hash_c="$(head_hash)"
gl_capture fold "$hash_b" "$hash_a" "$hash_c"
assert_exit_fail "$CODE" "precond_multi_commit_src_fail"
assert_contains "$OUT" "Only one commit source" "precond_multi_commit_src_msg"

describe "precond: mixed file and commit sources are rejected"
setup_repo_with_remote
commit_file "Commit A" "mix-a.txt"
hash_a="$(head_hash)"
commit_file "Commit B" "mix-b.txt"
hash_b="$(head_hash)"
write_file "mix-a.txt" "changed"
gl_capture fold mix-a.txt "$hash_a" "$hash_b"
assert_exit_fail "$CODE" "precond_mixed_src_fail"
assert_contains "$OUT" "Cannot mix" "precond_mixed_src_msg"

describe "precond: commitfile + branch target is rejected"
setup_repo_with_remote
create_feature_branch "g-cf-branch"
echo "cf content" > "$WORK/cf.txt"
git -C "$WORK" add cf.txt
git -C "$WORK" commit -q -m "cf commit"
cf_ref=$(gl status -f | grep "cf.txt" | grep -oE '[0-9a-z]+:[0-9]+' | head -1)
gl_capture fold "$cf_ref" g-cf-branch
assert_exit_fail "$CODE" "precond_commitfile_branch_fail"
assert_contains "$OUT" "Cannot fold a commit file into a branch" "precond_commitfile_branch_msg"

describe "precond: file with no changes to fold"
setup_repo_with_remote
commit_file "Base commit" "nochange.txt"
target_hash="$(head_hash)"
gl_capture fold nochange.txt "$target_hash"
assert_exit_fail "$CODE" "precond_no_file_changes_fail"
assert_contains "$OUT" "has no changes" "precond_no_file_changes_msg"

# ══════════════════════════════════════════════════════════════════════════════
# CASE 0: STAGED FILES + COMMIT (SINGLE-ARGUMENT)
# ══════════════════════════════════════════════════════════════════════════════

describe "staged: fold staged file into HEAD using full hash"
setup_repo_with_remote
commit_file "Staged target" "staged-head.txt"
old_hash="$(head_hash)"
write_file "staged-head.txt" "amended staged content"
git -C "$WORK" add staged-head.txt
out=$(gl fold "$old_hash")
assert_exit_ok $? "staged_full_hash_ok"
assert_ne "$old_hash" "$(head_hash)" "staged_full_hash_rewrote"
assert_head_msg "Staged target" "staged_full_hash_msg"
assert_file_content "staged-head.txt" "amended staged content" "staged_full_hash_content"

describe "staged: fold staged file into HEAD using short ID"
setup_repo_with_remote
commit_file "Staged short" "staged-short.txt"
old_hash="$(head_hash)"
target_sid=$(commit_sid_from_status "Staged short")
write_file "staged-short.txt" "short id staged content"
git -C "$WORK" add staged-short.txt
out=$(gl fold "$target_sid")
assert_exit_ok $? "staged_short_id_ok"
assert_ne "$old_hash" "$(head_hash)" "staged_short_id_rewrote"
assert_head_msg "Staged short" "staged_short_id_msg"

describe "staged: fold staged file into non-HEAD commit"
setup_repo_with_remote
commit_file "Base commit" "staged-base.txt"
base_hash="$(head_hash)"
commit_file "Top commit" "staged-top.txt"
old_top_hash="$(head_hash)"
write_file "staged-base.txt" "staged into base"
git -C "$WORK" add staged-base.txt
out=$(gl fold "$base_hash")
assert_exit_ok $? "staged_non_head_ok"
assert_head_msg "Top commit" "staged_non_head_top_msg"
assert_ne "$old_top_hash" "$(head_hash)" "staged_non_head_top_rewrote"
assert_file_content "staged-base.txt" "staged into base" "staged_non_head_content"

describe "staged: unstaged changes to same file are preserved"
setup_repo_with_remote
commit_file "Preserve unstaged" "preserve.txt"
target_hash="$(head_hash)"
# Stage one change, then modify the file again (unstaged)
write_file "preserve.txt" "staged line"
git -C "$WORK" add preserve.txt
write_file "preserve.txt" "unstaged line"
out=$(gl fold "$target_hash")
assert_exit_ok $? "staged_preserve_unstaged_ok"
assert_file_content "preserve.txt" "unstaged line" "staged_preserve_unstaged_wt"

# ══════════════════════════════════════════════════════════════════════════════
# CASE 1: FILE(S) + COMMIT (AMEND)
# ══════════════════════════════════════════════════════════════════════════════

describe "amend: fold single file into HEAD using full hash"
setup_repo_with_remote
commit_file "First commit" "file.txt"
old_hash="$(head_hash)"
write_file "file.txt" "amended content"
out=$(gl fold file.txt "$old_hash")
assert_exit_ok $? "amend_single_head_ok"
assert_ne "$old_hash" "$(head_hash)" "amend_single_head_rewrote"
assert_head_msg "First commit" "amend_single_head_msg"
assert_file_content "file.txt" "amended content" "amend_single_head_content"

describe "amend: fold file into HEAD using HEAD reference"
setup_repo_with_remote
commit_file "Head ref commit" "headref.txt"
old_hash="$(head_hash)"
write_file "headref.txt" "head ref amended"
out=$(gl fold headref.txt HEAD)
assert_exit_ok $? "amend_head_ref_ok"
assert_ne "$old_hash" "$(head_hash)" "amend_head_ref_rewrote"
assert_head_msg "Head ref commit" "amend_head_ref_msg"

describe "amend: fold file into HEAD using short ID"
setup_repo_with_remote
commit_file "Short ID target" "shortid.txt"
old_hash="$(head_hash)"
target_sid=$(commit_sid_from_status "Short ID target")
write_file "shortid.txt" "short id amended"
out=$(gl fold shortid.txt "$target_sid")
assert_exit_ok $? "amend_short_id_ok"
assert_ne "$old_hash" "$(head_hash)" "amend_short_id_rewrote"
assert_head_msg "Short ID target" "amend_short_id_msg"

describe "amend: fold file into non-HEAD commit using full hash"
setup_repo_with_remote
commit_file "Base commit" "base.txt"
target_hash="$(head_hash)"
commit_file "Top commit" "top.txt"
write_file "base.txt" "amended base"
out=$(gl fold base.txt "$target_hash")
assert_exit_ok $? "amend_non_head_ok"
assert_head_msg "Top commit" "amend_non_head_top_msg"
assert_file_content "base.txt" "amended base" "amend_non_head_content"

describe "amend: fold multiple files into HEAD"
setup_repo_with_remote
commit_file "Multi file commit" "multi-a.txt"
echo "multi-b content" > "$WORK/multi-b.txt"
git -C "$WORK" add multi-b.txt
git -C "$WORK" commit -q -m "Multi file commit"
old_hash="$(head_hash)"
write_file "multi-a.txt" "amended multi-a"
write_file "multi-b.txt" "amended multi-b"
out=$(gl fold multi-a.txt multi-b.txt HEAD)
assert_exit_ok $? "amend_multi_ok"
assert_ne "$old_hash" "$(head_hash)" "amend_multi_rewrote"
assert_file_content "multi-a.txt" "amended multi-a" "amend_multi_content_a"
assert_file_content "multi-b.txt" "amended multi-b" "amend_multi_content_b"

describe "amend: other uncommitted files are preserved"
setup_repo_with_remote
commit_file "Amend preserve" "amend-target.txt"
target_hash="$(head_hash)"
commit_file "Preserve this" "preserve-other.txt"
write_file "amend-target.txt" "amended"
write_file "preserve-other.txt" "should stay dirty"
out=$(gl fold amend-target.txt "$target_hash")
assert_exit_ok $? "amend_preserve_other_ok"
assert_file_content "preserve-other.txt" "should stay dirty" "amend_preserve_other_content"

# ══════════════════════════════════════════════════════════════════════════════
# CASE 1b: UNSTAGED (zz) + COMMIT (AMEND ALL)
# ══════════════════════════════════════════════════════════════════════════════

describe "amend-all: fold all working tree changes into HEAD"
setup_repo_with_remote
commit_file "All changes target" "all-a.txt"
echo "all-b content" > "$WORK/all-b.txt"
git -C "$WORK" add all-b.txt
git -C "$WORK" commit -q -m "All changes target"
old_hash="$(head_hash)"
write_file "all-a.txt" "modified a"
write_file "all-b.txt" "modified b"
out=$(gl fold zz HEAD)
assert_exit_ok $? "amend_all_ok"
assert_ne "$old_hash" "$(head_hash)" "amend_all_rewrote"
assert_head_msg "All changes target" "amend_all_msg"
assert_file_content "all-a.txt" "modified a" "amend_all_content_a"
assert_file_content "all-b.txt" "modified b" "amend_all_content_b"

describe "amend-all: fold all changes into non-HEAD commit using short ID"
setup_repo_with_remote
commit_file "ZZ base" "zz-base.txt"
base_sid=$(commit_sid_from_status "ZZ base")
commit_file "ZZ top" "zz-top.txt"
write_file "zz-base.txt" "zz modified base"
out=$(gl fold zz "$base_sid")
assert_exit_ok $? "amend_all_non_head_ok"
assert_head_msg "ZZ top" "amend_all_non_head_top_msg"
assert_file_content "zz-base.txt" "zz modified base" "amend_all_non_head_content"

# ══════════════════════════════════════════════════════════════════════════════
# CASE 2: COMMIT + COMMIT (FIXUP)
# ══════════════════════════════════════════════════════════════════════════════

describe "fixup: source commit absorbed into target using full hashes"
setup_repo_with_remote
commit_file "Target commit" "target.txt"
target_hash="$(head_hash)"
commit_file "Source commit" "source.txt"
source_hash="$(head_hash)"
out=$(gl fold "$source_hash" "$target_hash")
assert_exit_ok $? "fixup_full_hash_ok"
assert_commit_not_in_log "$source_hash" "fixup_full_hash_source_gone"
assert_log_contains "Target commit" "fixup_full_hash_target_present"
assert_head_msg "Target commit" "fixup_full_hash_msg_preserved"

describe "fixup: source commit absorbed into target using short IDs"
setup_repo_with_remote
commit_file "Fixup target" "fixup-t.txt"
commit_file "Fixup source" "fixup-s.txt"
source_sid=$(commit_sid_from_status "Fixup source")
target_sid=$(commit_sid_from_status "Fixup target")
source_hash="$(head_hash)"
out=$(gl fold "$source_sid" "$target_sid")
assert_exit_ok $? "fixup_short_id_ok"
assert_commit_not_in_log "$source_hash" "fixup_short_id_source_gone"
assert_head_msg "Fixup target" "fixup_short_id_msg_preserved"

describe "fixup: source must be newer than target"
setup_repo_with_remote
commit_file "Old commit" "old.txt"
old_hash="$(head_hash)"
commit_file "New commit" "new.txt"
new_hash="$(head_hash)"
# Try to fold older commit INTO newer one (wrong direction)
gl_capture fold "$old_hash" "$new_hash"
assert_exit_fail "$CODE" "fixup_wrong_order_fail"
assert_contains "$OUT" "Source commit must be newer than target" "fixup_wrong_order_msg"

describe "fixup: target commit message is preserved, source disappears"
setup_repo_with_remote
commit_file "Keep this message" "keep.txt"
target_hash="$(head_hash)"
commit_file "Discard this message" "discard.txt"
source_hash="$(head_hash)"
out=$(gl fold "$source_hash" "$target_hash")
assert_exit_ok $? "fixup_msg_ok"
assert_head_msg "Keep this message" "fixup_msg_preserved"
assert_log_not_contains "Discard this message" "fixup_source_msg_gone"

describe "fixup: uncommitted changes are preserved"
setup_repo_with_remote
commit_file "Fixup preserve target" "fp-target.txt"
target_hash="$(head_hash)"
commit_file "Fixup preserve source" "fp-source.txt"
source_hash="$(head_hash)"
write_file "fp-target.txt" "uncommitted dirty"
out=$(gl fold "$source_hash" "$target_hash")
assert_exit_ok $? "fixup_preserve_wt_ok"
assert_file_content "fp-target.txt" "uncommitted dirty" "fixup_preserve_wt_content"

# ══════════════════════════════════════════════════════════════════════════════
# CASE 3: COMMIT + BRANCH (MOVE)
# ══════════════════════════════════════════════════════════════════════════════

describe "move: commit relocated to target branch by branch name"
setup_repo_with_remote
create_feature_branch "g-move-src"
switch_to g-move-src
commit_file "Move me" "move-me.txt"
switch_to integration
weave_branch "g-move-src"
create_feature_branch "h-move-dst"
switch_to h-move-dst
commit_file "Dst existing" "dst-existing.txt"
switch_to integration
weave_branch "h-move-dst"
move_sid=$(commit_sid_from_status "Move me")
out=$(gl fold "$move_sid" h-move-dst)
assert_exit_ok $? "move_branch_name_ok"
# Commit should now be on h-move-dst
assert_contains "$(git -C "$WORK" log h-move-dst --oneline)" "Move me" "move_branch_name_on_dst"
# Commit should no longer be in g-move-src's unique commits (not between src and its base)
upstream_oid=$(upstream_oid)
assert_not_contains "$(git -C "$WORK" log g-move-src ^"$upstream_oid" --oneline 2>/dev/null)" "Move me" "move_branch_name_not_in_src"

describe "move: commit relocated to target branch by short ID"
setup_repo_with_remote
create_feature_branch "g-sid-src"
switch_to g-sid-src
commit_file "Move by sid" "sid-move.txt"
switch_to integration
weave_branch "g-sid-src"
create_feature_branch "h-sid-dst"
switch_to h-sid-dst
commit_file "Dst sid existing" "dst-sid.txt"
switch_to integration
weave_branch "h-sid-dst"
move_csid=$(commit_sid_from_status "Move by sid")
dst_bsid=$(branch_sid_from_status "h-sid-dst")
out=$(gl fold "$move_csid" "$dst_bsid")
assert_exit_ok $? "move_short_id_ok"
assert_contains "$(git -C "$WORK" log h-sid-dst --oneline)" "Move by sid" "move_short_id_on_dst"

describe "move: co-located target branches — only the target advances"
setup_repo_with_remote
# g-move-to-coloc is the commit source
create_feature_branch "g-move-to-coloc"
switch_to g-move-to-coloc
commit_file "Will be moved" "will-be-moved.txt"
switch_to integration
weave_branch "g-move-to-coloc"
# h-coloc-dst and i-coloc-dst share the same tip (co-located)
create_feature_branch "h-coloc-dst"
switch_to h-coloc-dst
commit_file "Coloc target existing" "coloc-target.txt"
switch_to integration
git -C "$WORK" branch i-coloc-dst h-coloc-dst
weave_branch "h-coloc-dst"
# i-coloc-dst-oid should NOT change after we move the commit to h-coloc-dst
coloc_oid_before=$(branch_oid "i-coloc-dst")
move_sid=$(commit_sid_from_status "Will be moved")
out=$(gl fold "$move_sid" h-coloc-dst)
assert_exit_ok $? "move_coloc_target_ok"
assert_contains "$(git -C "$WORK" log h-coloc-dst --oneline)" "Will be moved" "move_coloc_target_on_dst"
assert_eq "$coloc_oid_before" "$(branch_oid i-coloc-dst)" "move_coloc_target_colocated_unaffected"

describe "move: uncommitted changes are preserved"
setup_repo_with_remote
create_feature_branch "g-move-preserve"
switch_to g-move-preserve
commit_file "Preserve move" "preserve-move.txt"
switch_to integration
weave_branch "g-move-preserve"
create_feature_branch "h-move-preserve-dst"
switch_to h-move-preserve-dst
commit_file "Preserve dst" "preserve-dst.txt"
switch_to integration
weave_branch "h-move-preserve-dst"
write_file "preserve-move.txt" "dirty during move"
move_sid=$(commit_sid_from_status "Preserve move")
out=$(gl fold "$move_sid" h-move-preserve-dst)
assert_exit_ok $? "move_preserve_wt_ok"
assert_file_content "preserve-move.txt" "dirty during move" "move_preserve_wt_content"

# ══════════════════════════════════════════════════════════════════════════════
# CASE 4: COMMIT + zz (UNCOMMIT)
# ══════════════════════════════════════════════════════════════════════════════

describe "uncommit: HEAD commit removed, changes land in working directory"
setup_repo_with_remote
commit_file "Uncommit me" "uncommit.txt"
old_hash="$(head_hash)"
commit_file "Stay in history" "stay.txt"
uncommit_hash="$(git -C "$WORK" rev-parse HEAD~1)"
uncommit_sid=$(commit_sid_from_status "Uncommit me")
out=$(gl fold "$uncommit_sid" zz)
assert_exit_ok $? "uncommit_non_head_ok"
assert_commit_not_in_log "$uncommit_hash" "uncommit_non_head_gone"
assert_log_contains "Stay in history" "uncommit_non_head_stay"
# The uncommitted change should be in the working directory
status_out=$(gl status)
assert_contains "$status_out" "uncommit.txt" "uncommit_non_head_in_wt"

describe "uncommit: HEAD commit (mixed reset path)"
setup_repo_with_remote
commit_file "Uncommit HEAD" "uncommit-head.txt"
old_hash="$(head_hash)"
head_sid=$(commit_sid_from_status "Uncommit HEAD")
out=$(gl fold "$head_sid" zz)
assert_exit_ok $? "uncommit_head_ok"
assert_commit_not_in_log "$old_hash" "uncommit_head_gone"
# File should be unstaged in working directory
status_out=$(gl status)
assert_contains "$status_out" "uncommit-head.txt" "uncommit_head_in_wt"

describe "uncommit: existing uncommitted changes are preserved"
setup_repo_with_remote
commit_file "Uncommit preserve" "uncommit-preserve.txt"
uncommit_sid=$(commit_sid_from_status "Uncommit preserve")
commit_file "Stay commit" "stay-commit.txt"
write_file "stay-commit.txt" "dirty during uncommit"
out=$(gl fold "$uncommit_sid" zz)
assert_exit_ok $? "uncommit_preserve_ok"
assert_file_content "stay-commit.txt" "dirty during uncommit" "uncommit_preserve_wt"

# ══════════════════════════════════════════════════════════════════════════════
# CASE 5: COMMITFILE + zz (UNCOMMIT FILE)
# ══════════════════════════════════════════════════════════════════════════════

describe "uncommit-file: remove one file from a commit to working directory"
setup_repo_with_remote
# Create a commit touching two files
echo "auth content" > "$WORK/auth.txt"
echo "main content" > "$WORK/main.txt"
git -C "$WORK" add auth.txt main.txt
git -C "$WORK" commit -q -m "Two file commit"
# Get the commitfile ref for main.txt (index 1)
cf_ref=$(gl status -f | grep "main.txt" | grep -oE '[0-9a-z]+:[0-9]+' | head -1)
old_hash="$(head_hash)"
out=$(gl fold "$cf_ref" zz)
assert_exit_ok $? "uncommit_file_ok"
# Commit should still exist but without main.txt
assert_ne "$old_hash" "$(head_hash)" "uncommit_file_commit_rewrote"
assert_head_msg "Two file commit" "uncommit_file_msg_preserved"
# main.txt changes should be in working directory
status_out=$(gl status)
assert_contains "$status_out" "main.txt" "uncommit_file_in_wt"

describe "uncommit-file: non-HEAD commit, file removed and returned to working dir"
setup_repo_with_remote
echo "feat-a" > "$WORK/feat-a.txt"
echo "feat-b" > "$WORK/feat-b.txt"
git -C "$WORK" add feat-a.txt feat-b.txt
git -C "$WORK" commit -q -m "Two feat commit"
# Make a descendant commit
commit_file "Descendant commit" "desc.txt"
cf_ref=$(gl status -f | grep "feat-b.txt" | grep -oE '[0-9a-z]+:[0-9]+' | head -1)
out=$(gl fold "$cf_ref" zz)
assert_exit_ok $? "uncommit_file_non_head_ok"
assert_head_msg "Descendant commit" "uncommit_file_non_head_top_msg"
status_out=$(gl status)
assert_contains "$status_out" "feat-b.txt" "uncommit_file_non_head_in_wt"

describe "uncommit-file: other files in the same commit are preserved"
setup_repo_with_remote
echo "keep content" > "$WORK/keep.txt"
echo "extract content" > "$WORK/extract.txt"
git -C "$WORK" add keep.txt extract.txt
git -C "$WORK" commit -q -m "Keep and extract commit"
cf_ref=$(gl status -f | grep "extract.txt" | grep -oE '[0-9a-z]+:[0-9]+' | head -1)
out=$(gl fold "$cf_ref" zz)
assert_exit_ok $? "uncommit_file_other_ok"
# keep.txt must still be part of the commit (not in working dir as modified)
status_out=$(gl status)
assert_not_contains "$status_out" "keep.txt" "uncommit_file_keep_in_commit"

# ══════════════════════════════════════════════════════════════════════════════
# CASE 6: COMMITFILE + COMMIT (MOVE FILE)
# ══════════════════════════════════════════════════════════════════════════════

describe "move-file: move a file's changes from one commit to another"
setup_repo_with_remote
commit_file "Commit Alpha" "alpha.txt"
echo "other content" > "$WORK/other.txt"
git -C "$WORK" add other.txt
git -C "$WORK" commit -q -m "Commit Beta"
# other.txt is in Commit Beta but should be in Commit Alpha
cf_ref=$(gl status -f | grep "other.txt" | grep -oE '[0-9a-z]+:[0-9]+' | head -1)
alpha_sid=$(commit_sid_from_status "Commit Alpha")
out=$(gl fold "$cf_ref" "$alpha_sid")
assert_exit_ok $? "move_file_ok"
# other.txt should now be in Commit Alpha
assert_contains "$(git -C "$WORK" show HEAD~1 --name-only)" "other.txt" "move_file_in_target"
# Commit Beta should no longer contain other.txt
assert_not_contains "$(git -C "$WORK" show HEAD --name-only)" "other.txt" "move_file_not_in_src"
assert_head_msg "Commit Beta" "move_file_src_msg_preserved"

describe "move-file: source and target must be different commits"
setup_repo_with_remote
commit_file "Same commit" "same.txt"
cf_ref=$(gl status -f | grep "same.txt" | grep -oE '[0-9a-z]+:[0-9]+' | head -1)
same_sid=$(commit_sid_from_status "Same commit")
gl_capture fold "$cf_ref" "$same_sid"
assert_exit_fail "$CODE" "move_file_same_commit_fail"
assert_contains "$OUT" "Source and target are the same commit" "move_file_same_commit_msg"

describe "move-file: uncommitted changes are preserved"
setup_repo_with_remote
commit_file "Move file preserve src" "mfp-src.txt"
echo "mfp other" > "$WORK/mfp-other.txt"
git -C "$WORK" add mfp-other.txt
git -C "$WORK" commit -q -m "Move file preserve dst"
write_file "mfp-src.txt" "dirty during move file"
cf_ref=$(gl status -f | grep "mfp-other.txt" | grep -oE '[0-9a-z]+:[0-9]+' | head -1)
target_sid=$(commit_sid_from_status "Move file preserve src")
out=$(gl fold "$cf_ref" "$target_sid")
assert_exit_ok $? "move_file_preserve_wt_ok"
assert_file_content "mfp-src.txt" "dirty during move file" "move_file_preserve_wt_content"

# ══════════════════════════════════════════════════════════════════════════════
# --CREATE FLAG (NEW BRANCH FROM COMMIT)
# ══════════════════════════════════════════════════════════════════════════════

describe "create: move commit to a new branch"
setup_repo_with_remote
commit_file "Loose commit" "loose.txt"
loose_sid=$(commit_sid_from_status "Loose commit")
out=$(gl fold --create "$loose_sid" g-new-branch)
assert_exit_ok $? "create_ok"
assert_branch_exists "g-new-branch" "create_branch_exists"
# The commit should be on the new branch
assert_contains "$(git -C "$WORK" log g-new-branch --oneline)" "Loose commit" "create_commit_on_branch"

describe "create: target branch already exists — warns and still moves the commit"
setup_repo_with_remote
commit_file "Existing branch commit" "existing.txt"
loose_sid=$(commit_sid_from_status "Existing branch commit")
create_feature_branch "g-already-exists"
out=$(gl fold --create "$loose_sid" g-already-exists)
assert_exit_ok $? "create_branch_exists_ok"
assert_contains "$out" "already exists" "create_branch_exists_warn"
assert_contains "$(git -C "$WORK" log g-already-exists --oneline)" "Existing branch commit" "create_branch_exists_moved"

describe "create: using full hash as source"
setup_repo_with_remote
commit_file "Create from hash" "hash-create.txt"
loose_hash="$(head_hash)"
out=$(gl fold --create "$loose_hash" h-from-hash)
assert_exit_ok $? "create_full_hash_ok"
assert_branch_exists "h-from-hash" "create_full_hash_branch_exists"
assert_contains "$(git -C "$WORK" log h-from-hash --oneline)" "Create from hash" "create_full_hash_on_branch"

# ══════════════════════════════════════════════════════════════════════════════
# CONTINUE / ABORT
# ══════════════════════════════════════════════════════════════════════════════
# Shared conflict setup: C1 changes A→B, C2 changes B→C.
# Uncommitting C1 (fold zz) drops it from history, forcing C2 to cherry-pick
# onto A → 3-way merge conflict.

describe "fold uncommit: conflict → continue → commit removed from history"
setup_repo_with_remote
printf "A\n" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt; git -C "$WORK" commit -q -m "Base"
printf "B\n" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt; git -C "$WORK" commit -q -m "C1"
printf "C\n" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt; git -C "$WORK" commit -q -m "C2"
c1_hash=$(git -C "$WORK" rev-parse HEAD~1)
gl_capture fold "$c1_hash" zz
assert_state_file   "fold_cont_state"
assert_contains "$OUT" "loom continue" "fold_cont_hint"
printf "resolved\n" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt
gl_capture continue
assert_exit_ok  "$CODE" "fold_cont_ok"
assert_no_state_file   "fold_cont_state_removed"
assert_contains "$OUT" "Uncommitted" "fold_cont_msg"
assert_log_not_contains "C1" "fold_cont_c1_gone"

describe "fold uncommit: conflict → abort → original state restored"
setup_repo_with_remote
printf "A\n" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt; git -C "$WORK" commit -q -m "Base"
printf "B\n" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt; git -C "$WORK" commit -q -m "C1"
printf "C\n" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt; git -C "$WORK" commit -q -m "C2"
c1_hash=$(git -C "$WORK" rev-parse HEAD~1)
old_head=$(head_hash)
gl_capture fold "$c1_hash" zz
assert_state_file "fold_abort_state"
gl_capture abort
assert_exit_ok  "$CODE" "fold_abort_ok"
assert_contains "$OUT" "Aborted" "fold_abort_msg"
assert_no_state_file   "fold_abort_state_removed"
assert_eq "$old_head" "$(head_hash)" "fold_abort_head_restored"
assert_log_contains "C1" "fold_abort_c1_preserved"

pass
