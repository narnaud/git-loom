#!/usr/bin/env bash
# Integration tests: loom abort preserves working state for all commands that
# support conflict recovery (drop, swap, fold, commit).
#
# After a conflicting operation pauses and is aborted, three bystander states
# must survive intact:
#   other-staged.txt   — new file staged (git add) but not committed
#   other-unstaged.txt — new file written to disk, not staged
#   new-file.txt       — new untracked file
#
# Note: absorb is covered by Rust unit tests (src/absorb_test.rs); engineering
# a genuine absorb rebase conflict via shell is architecturally infeasible.
set -euo pipefail
source "$(dirname "$0")/helpers.sh"
trap 'rm -rf "$TMPROOT"' EXIT

# ── Assertion helper ──────────────────────────────────────────────────────────

assert_bystanders_preserved() {
    local label="$1"
    git -C "$WORK" diff --cached --name-only | grep -qF "other-staged.txt" \
        || fail "[$label] other-staged.txt not staged after abort"
    assert_file_content "other-staged.txt"   "staged-content"   "${label}_staged"
    assert_file_content "other-unstaged.txt" "unstaged-content" "${label}_unstaged"
    [[ -f "$WORK/new-file.txt" ]] \
        || fail "[$label] new-file.txt missing after abort"
    assert_file_content "new-file.txt" "new-content" "${label}_new_file"
}

# ══════════════════════════════════════════════════════════════════════════════
# DROP
# ══════════════════════════════════════════════════════════════════════════════
# Conflict: C1 changes A→B, C2 changes B→C. Dropping C1 forces C2 to replay
# "B→C" against "A" → 3-way merge conflict.

describe "drop abort: staged, unstaged, and new files are preserved"
setup_repo_with_remote
printf "A\n" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt; git -C "$WORK" commit -q -m "Base"
printf "B\n" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt; git -C "$WORK" commit -q -m "C1"
printf "C\n" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt; git -C "$WORK" commit -q -m "C2"
c1_hash=$(git -C "$WORK" rev-parse HEAD~1)
# Bystander working state
write_file "other-staged.txt"   "staged-content";   git -C "$WORK" add other-staged.txt
write_file "other-unstaged.txt" "unstaged-content"
write_file "new-file.txt"       "new-content"
gl_capture drop "$c1_hash" --yes
assert_state_file "drop_wsp_state"
gl_capture abort
assert_exit_ok   "$CODE" "drop_wsp_abort_ok"
assert_no_state_file     "drop_wsp_no_state"
assert_bystanders_preserved "drop_wsp"

# ══════════════════════════════════════════════════════════════════════════════
# SWAP
# ══════════════════════════════════════════════════════════════════════════════
# Conflict: swap A↔B where A changes "base"→"from A" and B changes "from A"→"from B".
# Swapping puts B first → B's diff ("from A"→"from B") applied to "base" → conflict.

describe "swap abort: staged, unstaged, and new files are preserved"
setup_repo_with_remote
git -C "$WORK" config rerere.enabled false
echo "base"   > "$WORK/shared.txt"; git -C "$WORK" add shared.txt; git -C "$WORK" commit -q -m "Base"
echo "from A" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt; git -C "$WORK" commit -q -m "Commit A"
hash_a=$(git -C "$WORK" rev-parse HEAD)
echo "from B" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt; git -C "$WORK" commit -q -m "Commit B"
hash_b=$(git -C "$WORK" rev-parse HEAD)
# Bystander working state
write_file "other-staged.txt"   "staged-content";   git -C "$WORK" add other-staged.txt
write_file "other-unstaged.txt" "unstaged-content"
write_file "new-file.txt"       "new-content"
gl_capture swap "$hash_a" "$hash_b"
assert_state_file "swap_wsp_state"
gl_capture abort
assert_exit_ok   "$CODE" "swap_wsp_abort_ok"
assert_no_state_file     "swap_wsp_no_state"
assert_bystanders_preserved "swap_wsp"

# ══════════════════════════════════════════════════════════════════════════════
# FOLD (uncommit)
# ══════════════════════════════════════════════════════════════════════════════
# Same A→B→C conflict: uncommitting C1 (fold <C1> zz) forces C2 to replay
# without C1 → 3-way merge conflict.

describe "fold abort: staged, unstaged, and new files are preserved"
setup_repo_with_remote
printf "A\n" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt; git -C "$WORK" commit -q -m "Base"
printf "B\n" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt; git -C "$WORK" commit -q -m "C1"
printf "C\n" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt; git -C "$WORK" commit -q -m "C2"
c1_hash=$(git -C "$WORK" rev-parse HEAD~1)
# Bystander working state
write_file "other-staged.txt"   "staged-content";   git -C "$WORK" add other-staged.txt
write_file "other-unstaged.txt" "unstaged-content"
write_file "new-file.txt"       "new-content"
gl_capture fold "$c1_hash" zz
assert_state_file "fold_wsp_state"
gl_capture abort
assert_exit_ok   "$CODE" "fold_wsp_abort_ok"
assert_no_state_file     "fold_wsp_no_state"
assert_bystanders_preserved "fold_wsp"

# ══════════════════════════════════════════════════════════════════════════════
# COMMIT
# ══════════════════════════════════════════════════════════════════════════════
# Conflict: feature branch has "feature\n" in shared.txt; integration has
# "integration\n". The new commit changes shared.txt to "feature-v2\n". When
# moved to the feature branch section (which has "feature\n"), the cherry-pick
# conflicts because "integration\n" ≠ "feature\n" in shared.txt context.
#
# other-staged.txt is saved aside by the commit command (save_and_unstage_other_staged)
# before the rebase runs, and is restored by rollback.saved_staged_patch on abort.

describe "commit abort: staged, unstaged, and new files are preserved"
setup_repo_with_remote
create_feature_branch "g-wsp-commit"
switch_to g-wsp-commit
printf "feature\n" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt; git -C "$WORK" commit -q -m "Feature base"
switch_to integration
weave_branch "g-wsp-commit"
printf "integration\n" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt; git -C "$WORK" commit -q -m "Integration base"
# Stage shared.txt for the new commit, then set up bystanders
printf "feature-v2\n" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt
write_file "other-staged.txt"   "staged-content";   git -C "$WORK" add other-staged.txt
write_file "other-unstaged.txt" "unstaged-content"
write_file "new-file.txt"       "new-content"
gl_capture commit -b g-wsp-commit -m "Feature v2" shared.txt
assert_state_file "commit_wsp_state"
gl_capture abort
assert_exit_ok   "$CODE" "commit_wsp_abort_ok"
assert_no_state_file     "commit_wsp_no_state"
assert_bystanders_preserved "commit_wsp"

# ══════════════════════════════════════════════════════════════════════════════
# FOLD (fold two commits — fixup path)
# ══════════════════════════════════════════════════════════════════════════════
# Conflict: A→B→C all modify shared.txt. Folding C (source) into A (target)
# inserts C's fixup right after A. C's diff "B→C" is applied when the file is
# still in state "A" → 3-way merge conflict.

describe "fold (fixup) abort: staged, unstaged, and new files are preserved"
setup_repo_with_remote
printf "A\n" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt; git -C "$WORK" commit -q -m "Commit A"
hash_a=$(git -C "$WORK" rev-parse HEAD)
printf "B\n" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt; git -C "$WORK" commit -q -m "Commit B"
printf "C\n" > "$WORK/shared.txt"; git -C "$WORK" add shared.txt; git -C "$WORK" commit -q -m "Commit C"
hash_c=$(git -C "$WORK" rev-parse HEAD)
# Bystander working state
write_file "other-staged.txt"   "staged-content";   git -C "$WORK" add other-staged.txt
write_file "other-unstaged.txt" "unstaged-content"
write_file "new-file.txt"       "new-content"
# fold C into A → C's diff "B→C" applied when file is "A" → conflict
gl_capture fold "$hash_c" "$hash_a"
assert_state_file "fold_fixup_wsp_state"
gl_capture abort
assert_exit_ok   "$CODE" "fold_fixup_wsp_abort_ok"
assert_no_state_file     "fold_fixup_wsp_no_state"
assert_bystanders_preserved "fold_fixup_wsp"

pass
