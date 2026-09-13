# Spec 005: Branch

> **Normative.** This document defines feature-branch creation, weaving, and unweaving.

## CLI

`branch` has alias `br`. Subcommand names `new`, `create`, `merge`, and `unmerge` are reserved and cannot be branch names because clap resolves them first.

### New

```bash
git-loom branch [name] [-t <target>]
git-loom branch new [name] [-t <target>]
git-loom branch create [name] [-t <target>]
```

The first form implicitly means `new`; `create` aliases `new`. If `[name]` is absent, prompt interactively. `-t, --target` accepts a commit hash/revision, commit or branch short ID, or branch name; without it, use the upstream merge-base.

Creation MUST trim and reject empty/whitespace names, validate Git branch-name rules (including no spaces, control characters, or `..`), and reject existing local names before calling `git branch`.

Target resolution uses Spec 002 in this order: exact local branch name (its tip), Git revision, then branch/commit short ID. Reject file targets. Default/short-ID resolution requires upstream tracking; a directly resolvable target does not replace the integration context required when weaving.

### Merge

```bash
git-loom branch merge [branch] [-a|--all]
```

Weave an existing non-woven branch with `git merge --no-ff`. Without `[branch]`, show an interactive picker. `--all` adds remote branches lacking local counterparts; selecting one first creates its local tracking branch. Error if the branch does not exist or is already woven.

### Unmerge

```bash
git-loom branch unmerge [branch]
```

Accept a branch name/short ID or show a picker. Rebase the branch's commits out of integration while preserving its ref at its original commits. Error unless it is currently woven.

## Creation and ownership

For `new`:

1. obtain and validate the name;
2. resolve `-t`, or find the upstream merge-base;
3. create the branch at that full commit hash;
4. weave when required below.

Status ownership walks from a branch tip to the next branch boundary or base. Creating a ref between commits splits ownership; a ref at the base owns none and displays as an empty section (Spec 001).

### Automatic weaving

If the target lies on the first-parent line from HEAD through, but not including, the base, convert those integration commits to a side section and add a merge commit (Spec 004). HEAD is included: branching there moves all first-parent integration commits into the section.

Do not restructure when:

- target equals the base (empty branch);
- target is already inside a side branch reached through a merge second parent. Create the ref and split ownership only.

Automatically stash and restore uncommitted changes. Branch refs created by loom MUST survive later integration rebases and advance to rewritten commits.

## Hidden-name warning

After `branch new` succeeds, if the name starts with configured `loom.hideBranchPattern` (default `local-`), print exactly:

```text
⚠ Branch `local-secrets` is hidden from status by default. Use `--all` to show it.
```

The setting is a prefix, not glob/regex; an empty setting disables hiding. Only `new` warns, not merge/unmerge (Spec 001).

## Conflicts and state

`branch new` automatic weaving is hard-fail: abort its rebase, restore the repository, save no state, and require retry.

`branch merge` is resumable. On conflict, save `.git/loom/state.json` with `branch_name`, report the pause, and permit:

- `loom continue`: complete the merge and print success;
- `loom abort`: run `git merge --abort` and restore original state.

While paused, block other loom commands except `loom show`, `loom diff`, `loom trace`, `loom continue`, and `loom abort` (Spec 014).

## Prerequisites

- Git 2.38+ and a non-bare repository working tree.
- Upstream tracking for a default target or short-ID target.
- An upstream-configured integration context for operations that weave/unweave.

Minimal examples:

```bash
git-loom branch feature-auth                 # at base
git-loom branch feature-b -t feature-a       # at feature-a tip
git-loom branch merge --all                  # picker includes remote-only refs
git-loom branch unmerge feature-auth         # ref survives
```
