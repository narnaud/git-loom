# Spec 013 — Split

## Overview

Split a single commit into two sequential commits by selecting which files belong in the first commit. The remaining files stay in the second commit, which keeps the original message.

This is the inverse of `fold` (commit + commit fixup). While file-level splitting is technically achievable with multiple `fold` commands, a dedicated `split` command with an interactive file picker provides real ergonomic value.

## CLI

```
git-loom split <target> [-m <message>]
```

- **`<target>`** — Commit hash, short ID, or `HEAD`.
- **`-m <message>`** — Message for the **first** (new) commit. If omitted, prompts interactively via `msg::input()`.

The **second** commit keeps the original commit message.

## What Happens

1. **Resolve target** — Uses the shared resolution strategy (Spec 002). Must resolve to a commit (not a branch, file, or unstaged).
2. **Validate** — The commit must:
   - Not be a merge commit (parent count must be ≤ 1).
   - Touch at least 2 files (a single-file commit cannot be split).
3. **File picker** — Display an `inquire::MultiSelect` prompt listing all files changed in the commit. The user selects files for the **first** commit; the rest go into the **second** commit.
4. **Get first commit message** — From `-m` flag or interactive prompt.
5. **Perform the split** — Two paths depending on whether the target is HEAD:

### HEAD path (no rebase)

```
reset_mixed(HEAD~1)
→ stage selected files → commit(msg1)
→ stage remaining files → commit(original_msg)
```

### Non-HEAD path (edit-and-continue rebase)

```
Weave::from_repo → edit_commit(oid) → run_rebase (pauses at target)
→ reset_mixed(HEAD~1)
→ stage selected files → commit(msg1)
→ stage remaining files → commit(original_msg)
→ continue_rebase
```

On error after the rebase starts: `git_rebase::abort(workdir)`.

6. **Print success** — `✓ Split \`<short_hash>\` into 2 commits`.

## Target Resolution

Uses the shared `git::resolve_target()` strategy. Only `Target::Commit` is accepted. All other target types produce clear error messages:

| Target type | Error |
|---|---|
| `Branch` | Cannot split a branch |
| `File` | Cannot split a file |
| `Unstaged` | Cannot split unstaged changes |
| `CommitFile` | Cannot split a commit file |

## Validation Errors

| Condition | Error |
|---|---|
| Merge commit | Cannot split a merge commit |
| Single-file commit | Cannot split a commit with only one file |
| No files selected | Must select at least one file for the first commit |
| All files selected | Must leave at least one file for the second commit |

## Prerequisites

- **Git ≥ 2.38** (for `--update-refs` in interactive rebase).
- **Working tree required** (not a bare repository).

## Examples

```bash
# Split HEAD into two commits
git loom split HEAD -m "Extract config changes"

# Split a commit by short ID
git loom split 0a

# Split a commit by hash
git loom split abc1234
```

## Design Decisions

1. **File-level granularity** — The split operates at the file level, not the hunk level. Hunk-level splitting would require a more complex UI and is a potential future enhancement.

2. **First commit gets the new message** — The second commit keeps the original message because it represents the "remainder" of the original work. The first commit is the extracted piece that needs a new description.

3. **Interactive picker by default** — No `--files` flag to pre-select files. The interactive picker ensures the user sees and confirms the split. The testable core (`split_commit_with_selection`) bypasses the picker for automated testing.

4. **Reuses edit-and-continue pattern** — Same approach as `reword` and `fold`: Weave-based rebase with `edit` command, manual commit manipulation, then `continue`. Falls back to linear todo for non-integration repos.

5. **Atomic operation** — If the rebase or any step fails, the operation is aborted and the repository is left unchanged.
