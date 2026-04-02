# Spec 013 — Split

## Overview

Split a single commit into two sequential commits by selecting which files (or
which hunks) belong in the first commit. The remaining content stays in the
second commit, which keeps the original message.

## Why Split?

As a change grows during development it is common to realise mid-way that the
commit contains two distinct ideas. With raw git, separating them requires an
interactive rebase (`edit`), `git reset HEAD~1`, hand-selecting the right files
or hunks, committing twice, and then continuing the rebase — a multi-step
sequence with no room for error. `loom split` compresses that into a single
command with an interactive picker.

## CLI

```bash
git-loom split <target> [-m <message>] [-p] [<files>...]
```

**Arguments:**

- `<target>` — Commit hash, short ID, or `HEAD`.
- `<files>...` — Files for the **first** commit (file-level split only). If
  omitted and `-p` is not set, an interactive file picker is shown.

**Flags:**

- `-m <message>` — Message for the **first** (new) commit. If omitted, opens
  the git editor.
- `-p` / `--patch` — Hunk-level split: open a commit-diff hunk picker and
  assign selected hunks to the first commit instead of whole files. When `-p`
  is given, the `<files>` arguments filter which files appear in the picker.

The **second** commit keeps the original commit message in both modes.

## What Happens

### File-level split (default)

1. **Resolve target** — Uses the shared resolution strategy (see Spec 002).
   Must resolve to a commit.
2. **Validate** — The commit must:
   - Not be a merge commit (parent count must be ≤ 1).
   - Touch at least 2 files (a single-file commit cannot be file-split; use
     `-p` to split at the hunk level instead).
3. **Save pre-existing staged changes** — Any staged changes are saved aside
   so a `reset --mixed` does not discard them; they are restored regardless of
   outcome.
4. **File selection** — If `<files>` are provided they are used directly.
   Otherwise an `inquire::MultiSelect` prompt lists all files changed in the
   commit; the user picks files for the **first** commit.
5. **Validate selection** — At least one file must remain for the second
   commit; error if all files are selected.
6. **Perform the split**:
   - **HEAD path** (no rebase):
     ```
     reset_mixed(HEAD~1) → stage selected → commit(msg1) → stage remaining → commit(original_msg)
     ```
   - **Non-HEAD path** (edit-and-continue rebase):
     ```
     start_edit_rebase(target) → reset_mixed(HEAD~1)
     → stage selected → commit(msg1) → stage remaining → commit(original_msg)
     → continue_rebase_or_abort
     ```
7. **Print success** — `Split \`<hash>\` into \`<hash1>\` and \`<hash2>\``.

**What changes:**

- The target commit is replaced by two new sequential commits with new hashes.
- All descendant commits get new hashes (same content and messages).

**What stays the same:**

- The second commit's message (original).
- Other branches not in the ancestry chain.
- Pre-existing staged and unstaged working-tree changes.

### Hunk-level split (`-p`)

1. **Resolve target** — Same as above; must resolve to a commit.
2. **Validate** — The commit must not be a merge commit.
3. **Save pre-existing staged changes** — Same as file-level split.
4. **Hunk selection** — Opens the commit-diff hunk picker for the target
   commit, optionally filtered to `<files>`. The user selects hunks for the
   **first** commit.
5. **Validate selection**:
   - Error if no hunks selected: `"Must select at least one hunk for the first commit"`.
   - Error if all hunks selected: `"Must leave at least one hunk for the second commit"`.
6. **Perform the split**:
   - **HEAD path**:
     ```
     reset_mixed(HEAD~1) → apply selected hunks (git apply --cached) → commit(msg1)
     → stage remaining changes → commit(original_msg)
     ```
   - **Non-HEAD path** (edit-and-continue rebase):
     ```
     start_edit_rebase(target) → reset_mixed(HEAD~1)
     → apply selected hunks → commit(msg1) → stage remaining → commit(original_msg)
     → continue_rebase_or_abort
     ```
7. **Print success** — Same format as file-level split.

**What changes / What stays the same:** same as file-level split.

**Limitations with `-p`:**

- Binary files and deleted files are handled at file granularity within the
  hunk picker (the entire file is included or excluded together).
- The `-p` mode does not save `LoomState` and does not support `loom continue`;
  any conflict causes an immediate auto-abort.

## Target Resolution

Uses `resolve_arg()` with `accept = [Commit]`. All other target types produce
error messages:

| Target type | Error |
|---|---|
| `Branch` | `"Cannot split a branch"` |
| `File` | `"Cannot split a file"` |
| `Unstaged` | `"Cannot split unstaged changes"` |
| `CommitFile` | `"Cannot split a commit file"` |

See Spec 002 for the resolution algorithm.

## Validation Errors

| Condition | Error |
|---|---|
| Merge commit | `"Cannot split a merge commit"` |
| Single-file commit (file mode) | `"Cannot split a commit with only one file"` |
| No files selected | `"Must select at least one file for the second commit"` |
| All files selected | `"Must leave at least one file for the second commit"` |
| No hunks selected (`-p`) | `"Must select at least one hunk for the first commit"` |
| All hunks selected (`-p`) | `"Must leave at least one hunk for the second commit"` |
| Picker cancelled | `"Cancelled"` |

## Conflict Recovery

Split uses **hard-fail** conflict handling. If a conflict occurs during the
non-HEAD rebase, the rebase is aborted automatically and the repository is
returned to its original state. `loom continue` / `loom abort` are not
supported for split.

Pre-existing staged changes are always restored regardless of outcome.

## Prerequisites

- Git ≥ 2.38 (for `--update-refs` in interactive rebase).
- Working tree required (not a bare repository).

## Examples

### Split HEAD by file

```
git-loom status
# ● ab  3f2a1c Add auth and config changes
#   (touches auth.rs and config.rs)

git-loom split HEAD -m "Extract config changes"
# → opens file picker; user selects config.rs
# ✓ Split `3f2a1c` into `1a2b3c` and `4d5e6f`
```

### Split a non-HEAD commit by file

```
git-loom status
# ● c1  aaa111 Add feature X and its tests
# ● c2  bbb222 Fix typo

git-loom split c1
# → opens file picker listing src/feature.rs, tests/feature_test.rs
# User selects src/feature.rs → prompted for message → "Add feature X"
# Second commit keeps "Add feature X and its tests"
```

### Split HEAD by hunk

```
git-loom split HEAD -p -m "Refactor loop"
# → opens hunk picker showing the diff of HEAD
# User selects hunks that belong in the first commit
# ✓ Split `3f2a1c` into `1a2b3c` and `4d5e6f`
```

### Split a non-HEAD commit by hunk

```
git-loom split 0a -p
# → opens hunk picker for commit 0a
# User selects hunks; editor opens for first commit message
# ✓ Split `0a3b5c` into `1a2b3c` and `4d5e6f`
```

### Provide files on the command line (bypass picker)

```
git-loom split abc1234 -m "Add config" config.rs
# Splits abc1234: config.rs → new commit "Add config";
# remaining files → commit with original message
```

## Design Decisions

### File-level vs hunk-level granularity

The default (file-level) split is simpler and sufficient for most cases: a
commit that accidentally bundled two features in different files. Hunk-level
splitting (`-p`) handles the harder case where both changes live in the same
file. Keeping them as two modes avoids forcing hunk-picker complexity on the
common case.

### First commit gets the new message

The second commit keeps the original message because it represents the
"remainder" of the original work. The first commit is the extracted piece that
needs a new description.

### Optional file arguments bypass the picker

Files for the first commit can be passed directly on the command line, bypassing
the interactive picker. This enables scripting and integration testing. When no
files are provided (and `-p` is not set), the interactive picker is shown.

### Hard-fail on conflict

Split does not save `LoomState` and does not support `loom continue`. If a
conflict arises during the non-HEAD rebase, the operation is aborted
automatically. This was chosen because the split is always performed just after
`edit` pauses the rebase; the user can re-run `loom split` once they have
resolved whatever caused the conflict.

### Reuses edit-and-continue pattern

Same approach as `reword` and `fold`: Weave-based rebase with an `edit`
command, manual commit manipulation, then `continue`. Falls back to a linear
todo for non-integration repos.
