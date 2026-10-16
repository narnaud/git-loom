# Spec 021: Forwarding Arguments to Git

## Overview

Several loom commands are thin wrappers around a single git command: they
resolve short IDs, work out the right revisions, and let git do the rest. Those
commands accept a `--` separator, and everything after it is handed to git
untouched.

```bash
git-loom <command> [loom args...] -- [git args...]
```

## Why a Separator?

Loom cannot mirror git's option surface. Copying it would guarantee drift, and
guessing — treating any option loom doesn't define as git's — costs more than it
buys:

- Loom's own flags shadow git's. With sniffing, `loom diff -a` can only ever be
  loom's `--all`, so `git diff --text` is unreachable.
- An option's value is indistinguishable from a target. `-S x` splits into an
  option and a token loom then tries to resolve as a commit.
- A typo becomes a git error instead of a loom one: `loom diff --stagd` would be
  forwarded rather than corrected.

A separator has none of those problems, and it is the same convention git
itself uses. The cost is one extra `--`.

## Expert Mode

The separator is for someone who knows what they are asking git to do. Loom
forwards the tokens and never checks what git did with them. An argument that
makes git print instead of commit (`--dry-run`, a status format), amend instead
of commit, commit something other than what loom staged (`-a`, `-i`, `--only`,
a pathspec), or reword (`-m`, `-F`, `-c`/`-C`) leaves loom going on from
whatever git did, history rewrite included. Data Safety covers loom's own
arguments, not what a forwarded one makes git do: a forwarded `--amend` on a
fold into an older commit squashes the user's HEAD commit into the target.

Loom owns only two things here: its own arguments come last, so a boolean git
resolves last-wins keeps loom's value, and a form that runs no git command for
the arguments to reach rejects the separator rather than drop them.

## Which Commands

| Command | Underlying git command |
|---------|------------------------|
| `show`  | `git show`   |
| `diff`  | `git diff`   |
| `commit`| `git commit` |
| `add`   | `git add`    |
| `fold`  | `git commit` |
| `split` | `git commit`, both of them |
| `absorb`| `git commit`, every `fixup!` commit |
| `reword`| `git commit --amend` |

The last four drive a rebase like the commands below, but they also make
commits of their own along the way, and those run the user's commit hooks. See
[Fold Commits Too](#fold-commits-too) and [Other Commands That
Commit](#other-commands-that-commit).

Every other command either renders its output itself (`status`, `tui`, `trace`)
or only rebases (`swap`, `drop`, `branch`, `update`, `init`), where there is no
single git command the arguments could belong to.

## What Happens

### Before the Separator

Loom parses strictly. An option loom does not define is an error naming the
token, with a tip to pass it after `--`.

**What changes:** nothing.

**What stays the same:** everything.

### After the Separator

Every token is forwarded verbatim, in order, including tokens that look like
options. Loom does not validate them: an option git rejects produces git's own
diagnostic.

A forwarded `--` reaches git as written too, but only `show` leaves it alone:
`diff` appends a `--` of its own when the user named file targets, and `add`
always appends one, so a second separator lands in a command line that already
has one and git rejects the result. The commands that commit place loom's own
arguments after the forwarded ones, so a forwarded `--` turns those into
pathspecs and git fails on them instead.

They are placed **after the revisions loom resolved and before any `--`
pathspec loom builds itself**, so a forwarded option is still read as an option
and a forwarded path is still read as a path:

```bash
git-loom show ab -- --stat        # git show <rev> --stat
git-loom show ab -- -- README.md  # git show <rev> -- README.md
git-loom diff f1 -- --stat        # git diff --stat -- file1.txt
git-loom commit -m x -- -S        # git commit -S -m x
git-loom add f1 -- -f             # git add -f -- file1.txt
git-loom fold f1 ab -- -n         # git commit -n --amend --no-edit --allow-empty
git-loom split ab -m x f1 -- -n   # git commit -n -m x, twice
git-loom absorb -- -n             # git commit -n -m 'fixup! …', per target
git-loom reword ab -m x -- -n     # git commit --quiet -n --allow-empty --amend --only -m x
```

Because the tokens are never inspected, an option's value may be attached or
detached — `-U5`, `--unified=5` and `-S x` all reach git as written, and git
decides which forms it accepts.

### Loom Steps Back When It Forwards

This is about `add` and `commit`, the two that normally run their git command
captured so loom can read its output and keep the display its own. Once the
user forwards arguments, that stops being right: an option may exist purely to
print (`--dry-run`, `-v`), or to open something (`-e`, `--interactive`), and a
captured command sends the report to the trace log and answers the editor with
`true`.

So `add` and `commit` run uncaptured when they carry forwarded arguments, and
git's output goes straight to the user. `add` also drops its own *"Staged N
file(s)"* line there: what was staged is now the forwarded option's business,
and git says nothing on success by its own convention.

Agent mode is the exception for those two: there is nobody to close an editor,
so they stay captured, which answers `-e` with `true` rather than hanging the
agent on a pty — the failure `commit`'s own agent-mode guard exists to prevent
(spec 019).

`show` and `diff` need none of this. They exist to display, so they always run
uncaptured, forwarded arguments or not; capturing them would swallow the user's
pager and colors along with everything else.

`fold`, `split`, `absorb` and `reword` do not step back either, for a different
reason: each commit is one step of a rewrite that goes on from what git did
(below), so loom keeps the run to itself. A forwarded `--dry-run` therefore
prints into the trace log, and loom goes on as if git had committed.
Where loom opens the editor itself — `split` or `reword` without `-m` — that
commit runs uncaptured as it always does.

### Fold Commits Too

The forms that make a commit take the separator: amending files, staged
changes or `zz` into a commit, moving a file's changes between commits or back
to the working tree, and every `-p` form, whose amend runs at a rebase pause. A
whole-commit form only rebases — fixup, move, uncommit, `-c`,
`--above`/`--below` — and rejects the separator: *"\<operation\> runs no `git
commit`, so it takes no arguments after `--`"*.

Nothing is inspected here either. Loom's own arguments are placed **after** the
forwarded ones, so a boolean git resolves last-wins keeps the value loom asked
for: `--no-amend` loses to `--amend`, and `--edit` to `--no-edit`.

That is all ordering buys. Git does not resolve a message source last-wins, so
`-m`, `-F`, `-c`/`-C`, `--fixup` and `--squash` all reword every commit fold
makes — the target itself when it is HEAD, which is `reword`'s job, and the
`fixup!` commit otherwise, where the squash throws the message away. Moving a
file or hunks *between* two commits amends both, so a message source rewords
the source commit as well as the target. A pathspec
restricts the commit and takes `--only` semantics with it, committing the
working tree's copy of that path rather than the index's. `-a`/`--all` and
`-i`/`--include` go the other way and sweep in tracked changes fold was never
given; on a non-HEAD target that buries them in a rewritten historical commit.
Fold's own unstaging feeds them: the user's other staged files are moved out of
the index and left in the working tree for the duration, which is exactly where
`-a` picks them up. These are the price of forwarding verbatim (Expert Mode).

**What changes:** nothing.

**What stays the same:** everything.

### Other Commands That Commit

`split`, `absorb` and `reword` follow fold: everything is forwarded verbatim and
placed before loom's own arguments, and nothing is inspected or checked (Expert
Mode).

- `split` forwards to both commits, and a forwarded message source reaches
  both.
- `absorb` forwards to every `fixup!` commit. `-a` and `-i` sweep the changes
  absorb skipped into the first fixup, and so into its target.
- `reword` forwards to the amend of a commit target; a branch target runs no
  `git commit` and rejects the separator — *"renaming a branch runs no `git
  commit`, so it takes no arguments after `--`"*. The working tree is
  autostashed at that pause, so `-a` has nothing to sweep.

**What changes:** nothing.

**What stays the same:** everything.

### When the Command Doesn't Take Them

`--` is not defined on the other commands, so the tokens after it are rejected
by the argument parser the same way any unexpected argument is.

**What changes:** nothing.

**What stays the same:** everything.

### When Interactive `add` Is Given Forwarded Arguments

`loom add -p` — and `loom add` with no files, which opens the same picker —
stages the hunks the user picked by applying a patch, so no `git add` runs and
there is nothing to forward to. The command errors: *"staging hunks
interactively takes no `git add` arguments after `--`"*. The message names the
mode rather than `-p`, which the no-files form never passed.

**What changes:** nothing.

**What stays the same:** everything.

## Examples

```bash
git-loom show -- --stat              # diffstat instead of the patch
git-loom diff -- -a                  # git's --text, which loom's own -a shadows
git-loom diff ab..d0 -- --name-only
git-loom commit -m "wip" -- --no-verify
git-loom add zz -- -f                # stage an ignored file too
git-loom fold zz ab -- --no-verify   # skip the pre-commit hook on the amend
git-loom absorb -- --no-verify       # ... on every fixup! commit
git-loom reword ab -- --reset-author
```

## Design Notes

### Loom's flags stay loom's

Before the separator a flag means what loom says it means, whatever git calls
it. `loom diff -a` is `--all` and `loom add -p` is loom's hunk picker; the git
meanings of both are one `--` away. Keeping loom's short flags consistent across
commands matters more than mirroring git's spelling on the two commands that
happen to collide.

### Forwarded arguments reach the commits loom makes

They go to the git command the loom command wraps, or to every commit it makes
itself, and nothing else. `loom commit -- --author=…` shapes the commit loom
creates, not the rebase that relocates it onto the feature branch, and `loom
fold -- --author=…` shapes the amend, not the commits the rebase replays over
it.
