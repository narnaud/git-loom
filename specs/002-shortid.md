# Spec 002: Short IDs

> **Normative.** This document defines short-ID allocation, display, and argument resolution.

## Entities and candidates

All displayed actionable entities share one global ID namespace.

| Entity | Source | First/default candidate |
| --- | --- | --- |
| Unstaged/local changes | fixed | `zz` |
| Commit | Change-Id letters (see Persistent commit identity), else full hexadecimal hash | first three letters, else first two hex characters |
| Branch | branch name words | first character of each of the first two words |
| File | filename stem (not extension/path) | first character of each of the first two words |

Branch and file IDs are at least two characters, commit IDs with a Change-Id at least three. For branch/file sources:

- Split multi-word names on `-`, `_`, and `/`; generate, in order, every pair containing one character from the first word and one from the second. Example: `feature-alpha` starts `fa, fl, fp, fh, ea, el, ...`.
- For a single word, generate every character pair `(i,j)` where `i < j`. `main` gives `ma, mi, mn, ai, an, in, ...`.
- Double a single character: `a` gives `aa`.
- After all two-character candidates, fall back to prefixes of length three or more.

Commit candidates depend on whether the commit carries a Change-Id:

- With one, its 40 hex digits are encoded as letters — jujutsu's reverse hex, `0→z, 1→y, … f→k` — and the candidates are prefixes of that string from the shortest length, at least three, at which no other displayed commit's letters share the prefix. This minimum is computed against every other Change-Id commit, so it is symmetric: a new commit whose letters share a prefix with an older one lengthens both IDs and never takes the older one; the old shorter ID then resolves to neither (see Argument resolution). A Change-Id displayed by more than one commit (cherry-pick twins) identifies none of them; those commits use hash candidates.
- Without one, successive hash prefixes of length 2, 3, 4, and so on.

The two alphabets are disjoint: letters mean a persistent ID, hex an ephemeral one.

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
│●    mqt d072f9a Fix bug
│●    3a  3a6f21c Cherry-picked
├╯
```

A commit line is `<id> <abbreviated hash> <subject>`: the ID first, padded to the widest commit ID in the output, then the dimmed abbreviated hash. ANSI-stripped output still contains the full abbreviated hash. The upstream/common-base marker gets no ID.

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
3. For `Commit`/`CommitFile` only, then match a persistent commit ID: a canonical `I<40 hex>` Change-Id (any case), or a prefix of at least three letters of a commit's Change-Id letters. Exactly one commit resolves; several (a prefix shorter than the displayed IDs, or twins sharing a Change-Id) error, listing each as `<id> <hash> <subject>`; a listed commit may be on a hidden branch. This pass runs after step 2 for every accepted kind, so a prefix never shadows a branch or file whose exact ID starts the same way. `status -f` accepts the same prefixes and skips ambiguous ones.
4. If no match exists, error and suggest `git-loom status` to list IDs.

A ref literally named like a persistent ID wins in step 1, exactly as a ref named like a two-character hex ID does today.

Git-native resolution works without an upstream. Short-ID resolution requires an upstream-configured current branch, successful `gather_repo_info()`, and an ID currently shown by status.
