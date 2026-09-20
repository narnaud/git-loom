# Spec 006: Commit

> **Normative.** This document defines committing to a feature or integration branch without leaving the integration checkout.

## CLI

```bash
git-loom commit [-b <branch> | -i] [-m <message>] [-p] [files...] [-- <git args>...]
```

| Input | Meaning |
| --- | --- |
| `-b, --branch <branch>` | Existing woven branch name/short ID, or new branch name. |
| `-i, --integration` | Loose commit directly on integration; mutually exclusive with `-b`. |
| `-m, --message <message>` | Message; without it open the Git editor. |
| `-p, --patch` | Interactively select hunks to stage (picker defined by Spec 007). |
| `[files...]` | File short IDs, paths, or reserved `zz`. |
| `-- <git args>...` | Forward untouched to `git commit`, not relocation rebase (for example `--no-verify`, `--signoff`, `-S`, `--author=...`; Spec 021). |

Staging rules:

- no file arguments: preserve/use the existing index;
- `zz`: `git add -A` for all unstaged changes;
- IDs/paths: stage only resolved files;
- `zz` anywhere wins over every other file argument.

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

- `-i` is present; or
- both `-b` and `-i` are absent **and** the current branch name equals the local counterpart of its upstream (`main` tracking `origin/main`).

This applies even with loose commits or woven branches already present. Any `-b` forces branch-targeted mode. A custom integration name tracking another name (for example `integration` tracking `origin/main`) requires `-i` for a loose commit.

### Branch-targeted mode

Resolve `-b` with `resolve_arg(..., [Branch])` (Spec 002):

- existing woven branch name/short ID: use it;
- existing non-woven branch: error exactly `Branch '<name>' is not woven into the integration branch.`;
- no existing match: treat as a new name, validate by Spec 005, create it at the weave base, then weave it;
- commit/file target: reject with `Commit target must be a branch.`.

Without `-b`/`-i` when loose-mode name matching does not apply, show a picker of woven branches plus “create new branch.” If none exist, prompt directly to create one.

A new empty target MUST fork from the base, even when other sections exist, so branches remain parallel rather than stacking on integration merges.

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
git-loom commit -i zz -m "integration adjustment"  # explicitly loose
git-loom commit -b fa auth.rs -m "fix auth"         # existing woven branch
git-loom commit -b feature-log -m "add logging"     # create at base, weave, relocate
```
