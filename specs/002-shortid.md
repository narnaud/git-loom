# Spec 002: Short IDs

> **Normative.** This document defines short-ID allocation, display, and argument resolution.

## Entities and candidates

All displayed actionable entities share one global ID namespace.

| Entity | Source | First/default candidate |
| --- | --- | --- |
| Unstaged/local changes | fixed | `zz` |
| Commit | full hexadecimal hash | first two hex characters |
| Branch | branch name words | first character of each of the first two words |
| File | filename stem (not extension/path) | first character of each of the first two words |

IDs are at least two characters. For branch/file sources:

- Split multi-word names on `-`, `_`, and `/`; generate, in order, every pair containing one character from the first word and one from the second. Example: `feature-alpha` starts `fa, fe, ft, fu, fr, ea, ...`.
- For a single word, generate every character pair `(i,j)` where `i < j`. `main` gives `ma, mi, mn, ai, an, in, ...`.
- Double a single character: `a` gives `aa`.
- After all two-character candidates, fall back to prefixes of length three or more.

Commit candidates are successive hash prefixes of length 2, 3, 4, and so on.

## Allocation and collisions

Allocate greedily in stable original order within these priority groups:

1. unstaged (`zz`);
2. commits;
3. branches and files.

Give each entity its first globally unused candidate. Only exact full-ID equality collides; `fa` and `fb` do not. If every candidate is exhausted, append numeric suffixes to the first candidate: `ab`, `ab1`, `ab2`, ... . `zz` is reserved and excluded from generated IDs.

Allocation MUST be deterministic and recomputed per invocation without persistence. Filename rather than path makes file IDs stable across directory moves; names/hashes provide branch/commit stability. A displayed ID and later resolution against the same repository state MUST identify the same entity.

## Display

IDs use blue underline (`COLOR_SHORTID`). Placement is:

```text
╭─ zz [local changes]
│   ma M src/main.rs
│
│╭─ fa [feature-a]
│●   d072f9 Fix bug
├╯
```

For a commit, the ID replaces/styles the matching initial hash characters; the rest of the abbreviated hash is dimmed. ANSI-stripped output still contains the full abbreviated hash. The upstream/common-base marker gets no ID.

## Persistent commit identity

Commits that `commit`, `split`, and `reword` create carry a Gerrit `Change-Id`
trailer so that a commit keeps one identity across every rewrite (rebase
replays copy the message verbatim); merges and `fixup!` commits get none.

- Format: `Change-Id: I<40 lowercase hex>`. When `gerrit.reviewUrl` is set, the
  trailer is instead `Link: <url>/id/I<hex>`, as Gerrit's `commit-msg` hook
  writes it. Both forms are read; the last matching trailer in the trailer
  block wins; hex is matched case-insensitively.
- Value: the current Gerrit hook's recipe — the blob hash of
  `<git var GIT_COMMITTER_IDENT>`, a newline, `HEAD`'s hash (the empty-tree
  hash on an unborn branch; after an editor-path commit, the new commit
  itself), a newline, and the message text. The value is opaque: Gerrit never
  re-derives it, only the shape is checked.
- Generation is on unless git config `loom.changeId` is `false` or
  `gerrit.createChangeId` is `false`. A message that already carries a
  Change-Id, or whose subject is an autosquash marker (`fixup! `, `squash! `),
  is never stamped. Loom's own `fixup!` commits (fold, absorb) get none.
- Loom writes the trailer into the message text itself, never through
  `git commit --trailer`, so `trailer.*` git config cannot rename, move, or
  suppress it. With `-m` the trailer is appended before committing (joined to
  an existing trailer block, otherwise as a new last paragraph). Without `-m`
  the commit is made with the editor first, then amended once (message only,
  `--no-verify`: the `pre-commit`/`commit-msg` hooks ran on the commit itself)
  when the final message has no Change-Id, so an editor that dropped it cannot
  leave the commit without one. Other hooks (`prepare-commit-msg`,
  `post-commit`, and `post-rewrite` for the amend) run for both commits. The
  amend follows git config, not the options forwarded after `--`: a commit
  signed because `commit.gpgsign` is set stays signed, one signed only by a
  forwarded `-S` does not. If that amend fails, the commit stands and a
  warning says it has no Change-Id (`reword` aborts instead: the id it keeps
  is the point).
- A rewrite that replaces the message (`reword`) re-stamps the commit's
  existing Change-Id; a commit without one receives a fresh one when
  generation is enabled. `split` keeps the original message, and so its id,
  on the second commit and stamps a fresh one on the first.
- Which commits carry a Change-Id is per repository, not enforced: commits made
  with raw git or received from elsewhere may have none.

## Argument resolution

Commands use `git::resolve_arg(repo, arg, accept)`, where ordered `accept: &[TargetKind]` limits both target kinds and their priority. Available kinds:

| Kind | Meaning/rules |
| --- | --- |
| `File` | Working-tree path, CWD-relative or absolute, converted to repo-relative. An absolute outside path errors exactly `'<path>' is outside repository`. |
| `Branch` | Local branch name. |
| `Commit` | Non-merge commit; merge commits are automatically rejected. Branch names are excluded when resolving this kind. |
| `CommitFile` | Commit-file reference such as `02:0`. |
| `Unstaged` | Working directory, ID `zz`. |

For accepted kinds, resolution proceeds as follows:

1. Try Git-native revision resolution first: full hashes, partial hashes of at least four characters, symbolic refs (`HEAD`, `HEAD~2`, `main`, `origin/main`), and other valid revision syntax. A successful result is a commit, subject to accepted kind and the non-merge rule.
2. If Git resolution fails, build the same full graph/status entities and look up an exact short ID in this order: branch, commit, file (restricted by `accept`).
3. If no match exists, error and suggest `git-loom status` to list IDs.

Git-native resolution works without an upstream. Short-ID resolution requires an upstream-configured current branch, successful `gather_repo_info()`, and an ID currently shown by status.
