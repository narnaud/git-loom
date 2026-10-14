# Spec 006: Commit

> **Normative.** This document defines committing to a feature or integration branch without leaving the integration checkout.

## CLI

```bash
git-loom commit [-b <branch> [--on <branch>] | -l] [-m <message>] [-p] [files...] [-- <git args>...]
git-loom commit [-b <branch> [--on <branch>] | -l] -m <message> -p [files...] --hunks <id>... --hunks-from <fingerprint>
```

| Input | Meaning |
| --- | --- |
| `-b, --branch <branch>` | Existing woven branch name/short ID, or new branch name. |
| `--on <branch>` | Create the new `-b` branch stacked on this woven branch's tip; requires `-b`. |
| `-l, --loose` | Loose commit directly on integration; mutually exclusive with `-b`. `-i, --integration` are hidden aliases (pre-#373 spelling). |
| `-m, --message <message>` | Message; without it open the Git editor. |
| `-p, --patch` | Select hunks to stage (picker defined by Spec 007), interactively or with `--hunks`. |
| `--hunks <id>` (repeated), `--hunks-from <fingerprint>` | Supply the `-p` selection by id; the ids are the whole selection, so a staged hunk left out is kept out of the commit and stays staged (Spec 019). Requires `-p`, and each flag requires the other. |
| `[files...]` | File short IDs, paths, or reserved `zz`. |
| `-- <git args>...` | Forward untouched to `git commit`, not relocation rebase (for example `--no-verify`, `--signoff`, `-S`, `--author=...`; Spec 021). |

Staging rules:

- no file arguments: preserve/use the existing index;
- `zz`: `git add -A` for all unstaged changes;
- IDs/paths: stage only resolved files;
- `zz` anywhere wins over every other file argument;
- `-p`: commit only what the picker kept. A staged hunk it left out, and every other staged path — outside the filter, or with no hunk to show such as a mode-only change — is set aside and stays staged.

File paths may be relative or absolute; resolution follows Spec 002.

## Preconditions

Require Git 2.40+, a non-bare working tree, and an upstream-configured integration branch. On a plain feature branch or detached HEAD, error exactly:

```text
Must be on an integration branch to use commit. Use `git commit` directly on feature branches.
```

After staging resolution, an empty index errors exactly `Nothing to commit`.

## Mode selection and flow

1. Resolve staging.
2. Select loose or branch-targeted mode.
3. For branch-targeted mode, resolve/create target and obtain message.
4. Create the commit, with a `Change-Id` trailer per Spec 002 (on `-m` and editor paths alike; `-- --no-verify` does not disable it).
5. Move it to the feature branch and update topology/refs in one operation.

### Loose mode

Create directly at integration HEAD with no branch picker or relocation rebase when:

- `-l` is present; or
- both `-b` and `-l` are absent **and** the current branch name equals the local counterpart of its upstream (`main` tracking `origin/main`).

This applies even with loose commits or woven branches already present. Any `-b` forces branch-targeted mode. A custom integration name tracking another name (for example `integration` tracking `origin/main`) requires `-l` for a loose commit.

### Branch-targeted mode

Resolve `-b` with `resolve_arg(..., [Branch])` (Spec 002):

- existing woven branch name/short ID: use it;
- existing non-woven branch: error exactly `Branch '<name>' is not woven into the integration branch.`;
- no existing match: treat as a new name, validate by Spec 005, create it at the weave base, then weave it;
- commit/file target: reject with `Commit target must be a branch.`.

`--on` creates the new `-b` branch at the tip of a woven branch instead of the base; the commit then splits it off as a stacked section (Spec 004), as `loom branch <new> --on <branch>` followed by `commit -b <new>` would. It resolves like Spec 005's `--on`, and refuses:

- an existing `-b` branch;
- a non-woven branch, as `-b` does;
- an empty branch, having nothing to stack on;
- any commit, with a hint to run `loom branch <new> --on <commit>` first: starting there inserts into the branch holding it.

Every `-b` and `--on` refusal comes before staging, so it leaves the index untouched; a new branch is created only after staging.

Without `-b`/`-l` when loose-mode name matching does not apply, show a picker of woven branches plus “create new branch.” If none exist, prompt directly to create one.

Without `--on`, a new empty target MUST fork from the base, even when other sections exist, so branches remain parallel rather than stacking on integration merges.

Create the commit at HEAD, then relocate it to the target via one Weave operation (Spec 004). The checkout remains on integration and all affected refs advance correctly.

## Conflicts and recovery

If relocation conflicts, pause and save `.git/loom/state.json`. The new commit remains recoverable in the working tree through mixed reset; original staged changes MUST be restored on abort.

Staged files set aside so they cannot join this commit MUST come back on every
failure path, including the ones after the commit is created but before
`state.json` exists, where no rollback can return them.

- `loom continue` resolves/completes relocation.
- `loom abort` cancels and restores original history and staged state.

While paused, block other loom commands except `loom show`, `loom diff`, `loom trace`, `loom continue`, and `loom abort` (Spec 014).

Minimal examples:

```bash
git-loom commit zz -m "quick fix"                   # loose on main -> origin/main
git-loom commit -l zz -m "integration adjustment"  # explicitly loose
git-loom commit -b fa auth.rs -m "fix auth"         # existing woven branch
git-loom commit -b feature-log -m "add logging"     # create at base, weave, relocate
git-loom commit -b stk --on fa -m "next step"      # create stacked on fa
```
