#!/usr/bin/env bash
# Integration tests for: gl split
set -euo pipefail
source "$(dirname "$0")/helpers.sh"
trap 'rm -rf "$TMPROOT"' EXIT

# ══════════════════════════════════════════════════════════════════════════════
# PRECONDITIONS
# ══════════════════════════════════════════════════════════════════════════════

describe "precond: not in a git repository"
TMP_NOGIT=$(mktemp -d)
CODE=0
(cd "$TMP_NOGIT" && NO_COLOR=1 GIT_TERMINAL_PROMPT=0 "$GL_BIN" split HEAD -m "msg" >/dev/null 2>&1) || CODE=$?
assert_exit_fail "$CODE" "precond_not_git_repo"
rm -rf "$TMP_NOGIT"

describe "precond: unknown full hash is rejected"
setup_repo_with_remote
gl_capture split "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef" -m "msg" file.txt
assert_exit_fail "$CODE" "precond_unknown_hash"

describe "precond: unknown short string is rejected"
setup_repo_with_remote
gl_capture split "zzzz9999" -m "msg" file.txt
assert_exit_fail "$CODE" "precond_unknown_short"

# ══════════════════════════════════════════════════════════════════════════════
# VALIDATION — SINGLE-FILE COMMIT
# ══════════════════════════════════════════════════════════════════════════════

describe "single-file HEAD commit is rejected with clear message"
setup_repo_with_remote
commit_file "Single file commit" "only.txt"
gl_capture split HEAD -m "First part" only.txt
assert_exit_fail "$CODE" "single_file_head_fail"
assert_contains "$OUT" "only one file" "single_file_head_msg"

describe "single-file non-HEAD commit is rejected (full hash)"
setup_repo_with_remote
commit_file "Single file target" "only.txt"
target_hash="$(head_hash)"
commit_file "Later commit" "later.txt"
gl_capture split "$target_hash" -m "First part" only.txt
assert_exit_fail "$CODE" "single_file_nonhead_fail"
assert_contains "$OUT" "only one file" "single_file_nonhead_msg"

describe "single-file commit is rejected via short ID"
setup_repo_with_remote
commit_file "Short ID target" "only.txt"
commit_file "Later for stack" "later2.txt"
short_id=$(commit_sid_from_status "Short ID target")
gl_capture split "$short_id" -m "First part" only.txt
assert_exit_fail "$CODE" "single_file_shortid_fail"
assert_contains "$OUT" "only one file" "single_file_shortid_msg"

# ══════════════════════════════════════════════════════════════════════════════
# VALIDATION — OTHER ERRORS
# ══════════════════════════════════════════════════════════════════════════════

describe "merge commit is rejected with clear message"
setup_repo_with_remote
create_feature_branch "g-side"
switch_to g-side
commit_file "Side branch commit" "side.txt"
switch_to integration
git -C "$WORK" merge -q --no-ff g-side -m "Merge g-side"
merge_hash="$(head_hash)"
gl_capture split "$merge_hash" -m "First part" side.txt
assert_exit_fail "$CODE" "merge_commit_fail"
assert_contains "$OUT" "merge commit" "merge_commit_err_msg"

describe "selecting all files for first commit is rejected"
setup_repo_with_remote
echo "content a" > "$WORK/qa.txt"
echo "content b" > "$WORK/qb.txt"
git -C "$WORK" add qa.txt qb.txt
git -C "$WORK" commit -q -m "Two files to exhaust"
gl_capture split HEAD -m "First part" qa.txt qb.txt
assert_exit_fail "$CODE" "all_files_fail"
assert_contains "$OUT" "at least one file for the second commit" "all_files_err_msg"

# ══════════════════════════════════════════════════════════════════════════════
# HEAD SPLIT
# ══════════════════════════════════════════════════════════════════════════════

describe "split HEAD: commit messages are assigned correctly"
setup_repo_with_remote
echo "content a" > "$WORK/ha.txt"
echo "content b" > "$WORK/hb.txt"
git -C "$WORK" add ha.txt hb.txt
git -C "$WORK" commit -q -m "HEAD two files"
out=$(gl split HEAD -m "First part" ha.txt)
assert_exit_ok $? "head_split_ok"
assert_contains "$out" "Split" "head_split_success_msg"
# Second commit (HEAD) keeps original message
assert_head_msg "HEAD two files" "head_split_second_msg"
# First commit (HEAD~1) gets new message
assert_msg_at 1 "First part" "head_split_first_msg"

describe "split HEAD: files are distributed between the two commits"
setup_repo_with_remote
echo "content a" > "$WORK/ia.txt"
echo "content b" > "$WORK/ib.txt"
git -C "$WORK" add ia.txt ib.txt
git -C "$WORK" commit -q -m "Files to verify"
gl split HEAD -m "First part" ia.txt
first_oid=$(git -C "$WORK" rev-parse "HEAD~1")
second_oid=$(git -C "$WORK" rev-parse "HEAD")
files_in_first=$(git -C "$WORK" diff-tree --no-commit-id -r --name-only "$first_oid")
files_in_second=$(git -C "$WORK" diff-tree --no-commit-id -r --name-only "$second_oid")
assert_contains "$files_in_first"  "ia.txt" "head_split_first_has_ia"
assert_not_contains "$files_in_first"  "ib.txt" "head_split_first_no_ib"
assert_contains "$files_in_second" "ib.txt" "head_split_second_has_ib"
assert_not_contains "$files_in_second" "ia.txt" "head_split_second_no_ia"

describe "split HEAD by full hash (equivalent to HEAD)"
setup_repo_with_remote
echo "content a" > "$WORK/ja.txt"
echo "content b" > "$WORK/jb.txt"
git -C "$WORK" add ja.txt jb.txt
git -C "$WORK" commit -q -m "Full hash HEAD split"
head_oid="$(head_hash)"
out=$(gl split "$head_oid" -m "Extracted" ja.txt)
assert_exit_ok $? "head_split_full_hash_ok"
assert_head_msg "Full hash HEAD split" "head_split_full_hash_second_msg"
assert_msg_at 1 "Extracted" "head_split_full_hash_first_msg"

describe "split HEAD by short ID"
setup_repo_with_remote
echo "content a" > "$WORK/ka.txt"
echo "content b" > "$WORK/kb.txt"
git -C "$WORK" add ka.txt kb.txt
git -C "$WORK" commit -q -m "Short ID HEAD split"
short_id=$(commit_sid_from_status "Short ID HEAD split")
out=$(gl split "$short_id" -m "Extracted part" ka.txt)
assert_exit_ok $? "head_split_shortid_ok"
assert_head_msg "Short ID HEAD split" "head_split_shortid_second_msg"
assert_msg_at 1 "Extracted part" "head_split_shortid_first_msg"

describe "split HEAD: three-file commit splits correctly by selecting two files"
setup_repo_with_remote
echo "content a" > "$WORK/ta.txt"
echo "content b" > "$WORK/tb.txt"
echo "content c" > "$WORK/tc.txt"
git -C "$WORK" add ta.txt tb.txt tc.txt
git -C "$WORK" commit -q -m "Three file commit"
out=$(gl split HEAD -m "Two files part" ta.txt tb.txt)
assert_exit_ok $? "three_file_split_ok"
assert_head_msg "Three file commit" "three_file_second_msg"
assert_msg_at 1 "Two files part" "three_file_first_msg"
first_oid=$(git -C "$WORK" rev-parse "HEAD~1")
files_in_first=$(git -C "$WORK" diff-tree --no-commit-id -r --name-only "$first_oid")
assert_contains "$files_in_first" "ta.txt" "three_file_first_has_ta"
assert_contains "$files_in_first" "tb.txt" "three_file_first_has_tb"
assert_not_contains "$files_in_first" "tc.txt" "three_file_first_no_tc"

# ══════════════════════════════════════════════════════════════════════════════
# NON-HEAD SPLIT
# ══════════════════════════════════════════════════════════════════════════════

describe "split non-HEAD commit by full hash: messages assigned correctly"
setup_repo_with_remote
echo "content a" > "$WORK/na.txt"
echo "content b" > "$WORK/nb.txt"
git -C "$WORK" add na.txt nb.txt
git -C "$WORK" commit -q -m "Non-HEAD two files"
target_hash="$(head_hash)"
commit_file "Later commit" "later.txt"
out=$(gl split "$target_hash" -m "Non-HEAD first part" na.txt)
assert_exit_ok $? "nonhead_split_hash_ok"
assert_contains "$out" "Split" "nonhead_split_hash_success_msg"
# HEAD is still the later commit
assert_head_msg "Later commit" "nonhead_split_later_preserved"
# HEAD~1 is the second split commit (original message)
assert_msg_at 1 "Non-HEAD two files" "nonhead_split_second_msg"
# HEAD~2 is the first split commit (new message)
assert_msg_at 2 "Non-HEAD first part" "nonhead_split_first_msg"

describe "split non-HEAD commit by full hash: files distributed correctly"
setup_repo_with_remote
echo "content a" > "$WORK/oa.txt"
echo "content b" > "$WORK/ob.txt"
git -C "$WORK" add oa.txt ob.txt
git -C "$WORK" commit -q -m "Non-HEAD file verify"
target_hash="$(head_hash)"
commit_file "Stack top" "stack.txt"
gl split "$target_hash" -m "Extracted oa" oa.txt
second_oid=$(git -C "$WORK" rev-parse "HEAD~1")
first_oid=$(git -C "$WORK" rev-parse "HEAD~2")
files_in_first=$(git -C "$WORK" diff-tree --no-commit-id -r --name-only "$first_oid")
files_in_second=$(git -C "$WORK" diff-tree --no-commit-id -r --name-only "$second_oid")
assert_contains "$files_in_first" "oa.txt" "nonhead_first_has_oa"
assert_not_contains "$files_in_first" "ob.txt" "nonhead_first_no_ob"
assert_contains "$files_in_second" "ob.txt" "nonhead_second_has_ob"

describe "split non-HEAD commit by short ID"
setup_repo_with_remote
echo "content a" > "$WORK/pa.txt"
echo "content b" > "$WORK/pb.txt"
git -C "$WORK" add pa.txt pb.txt
git -C "$WORK" commit -q -m "Short ID non-HEAD split"
commit_file "Stacked above target" "stacked.txt"
short_id=$(commit_sid_from_status "Short ID non-HEAD split")
out=$(gl split "$short_id" -m "Short ID first part" pa.txt)
assert_exit_ok $? "nonhead_split_shortid_ok"
assert_head_msg "Stacked above target" "nonhead_split_shortid_top_preserved"
assert_msg_at 1 "Short ID non-HEAD split" "nonhead_split_shortid_second_msg"
assert_msg_at 2 "Short ID first part" "nonhead_split_shortid_first_msg"

describe "split non-HEAD preserves all commits above target"
setup_repo_with_remote
echo "content a" > "$WORK/ra.txt"
echo "content b" > "$WORK/rb.txt"
git -C "$WORK" add ra.txt rb.txt
git -C "$WORK" commit -q -m "Target to split"
target_hash="$(head_hash)"
commit_file "Above 1" "above1.txt"
commit_file "Above 2" "above2.txt"
gl split "$target_hash" -m "Split first" ra.txt
# History from top: "Above 2", "Above 1", "Target to split" (second), "Split first"
assert_head_msg "Above 2" "preserve_above2"
assert_msg_at 1 "Above 1" "preserve_above1"
assert_msg_at 2 "Target to split" "preserve_second_split"
assert_msg_at 3 "Split first" "preserve_first_split"

# ══════════════════════════════════════════════════════════════════════════════
# WOVEN BRANCH PRESERVATION
# ══════════════════════════════════════════════════════════════════════════════

describe "split commit in woven branch: branch ref and merge topology preserved"
setup_repo_with_remote
create_feature_branch "g-woven"
switch_to g-woven
echo "content a" > "$WORK/wa.txt"
echo "content b" > "$WORK/wb.txt"
git -C "$WORK" add wa.txt wb.txt
git -C "$WORK" commit -q -m "Woven two files"
target_hash="$(head_hash)"
switch_to integration
weave_branch "g-woven"
out=$(gl split "$target_hash" -m "Woven first part" wa.txt)
assert_exit_ok $? "woven_split_ok"
assert_branch_exists "g-woven" "woven_branch_still_exists"
# Integration HEAD should still be a merge commit
assert_head_parent_count 2 "woven_merge_topology_preserved"

describe "split commit in woven branch: g-woven ref points to second commit"
setup_repo_with_remote
create_feature_branch "g-woven2"
switch_to g-woven2
echo "content a" > "$WORK/xa.txt"
echo "content b" > "$WORK/xb.txt"
git -C "$WORK" add xa.txt xb.txt
git -C "$WORK" commit -q -m "Woven split target"
target_hash="$(head_hash)"
switch_to integration
weave_branch "g-woven2"
gl split "$target_hash" -m "Woven extracted" xa.txt
# g-woven2 should point to the second commit (original message)
branch_tip_msg=$(git -C "$WORK" log -1 --pretty=%s g-woven2)
assert_eq "$branch_tip_msg" "Woven split target" "woven_branch_tip_is_second_commit"

# ══════════════════════════════════════════════════════════════════════════════
# STAGED CHANGES PRESERVATION
# ══════════════════════════════════════════════════════════════════════════════

describe "pre-existing staged changes are preserved after HEAD split"
setup_repo_with_remote
echo "content a" > "$WORK/sa.txt"
echo "content b" > "$WORK/sb.txt"
git -C "$WORK" add sa.txt sb.txt
git -C "$WORK" commit -q -m "Files for staged-preserve test"
write_file "staged_extra.txt" "staged content"
git -C "$WORK" add staged_extra.txt
out=$(gl split HEAD -m "First part" sa.txt)
assert_exit_ok $? "staged_preserved_ok"
staged=$(git -C "$WORK" diff --cached --name-only)
assert_contains "$staged" "staged_extra.txt" "staged_extra_still_staged"

describe "pre-existing staged changes are preserved after non-HEAD split"
setup_repo_with_remote
echo "content a" > "$WORK/ua.txt"
echo "content b" > "$WORK/ub.txt"
git -C "$WORK" add ua.txt ub.txt
git -C "$WORK" commit -q -m "Files for non-head staged test"
target_hash="$(head_hash)"
commit_file "Top commit" "top.txt"
write_file "staged_nonhead.txt" "staged nonhead content"
git -C "$WORK" add staged_nonhead.txt
out=$(gl split "$target_hash" -m "First part" ua.txt)
assert_exit_ok $? "staged_nonhead_preserved_ok"
staged=$(git -C "$WORK" diff --cached --name-only)
assert_contains "$staged" "staged_nonhead.txt" "staged_nonhead_still_staged"

pass
