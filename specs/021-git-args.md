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

## Which Commands

| Command | Underlying git command |
|---------|------------------------|
| `show`  | `git show`   |
| `diff`  | `git diff`   |
| `commit`| `git commit` |
| `add`   | `git add`    |

Every other command either renders its output itself (`status`, `tui`, `trace`)
or drives a rebase through Weave (`fold`, `absorb`, `split`, `swap`, `drop`,
`reword`, `branch`, `update`, `init`), where there is no single git command the
arguments could belong to.

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
has one and git rejects the result.

They are placed **after the revisions loom resolved and before any `--`
pathspec loom builds itself**, so a forwarded option is still read as an option
and a forwarded path is still read as a path:

```bash
git-loom show ab -- --stat        # git show <rev> --stat
git-loom show ab -- -- README.md  # git show <rev> -- README.md
git-loom diff f1 -- --stat        # git diff --stat -- file1.txt
git-loom commit -m x -- -S        # git commit -S -m x
git-loom add f1 -- -f             # git add -f -- file1.txt
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
```

## Design Notes

### Loom's flags stay loom's

Before the separator a flag means what loom says it means, whatever git calls
it. `loom diff -a` is `--all` and `loom add -p` is loom's hunk picker; the git
meanings of both are one `--` away. Keeping loom's short flags consistent across
commands matters more than mirroring git's spelling on the two commands that
happen to collide.

### Forwarded arguments reach one git command

They go to the git command the loom command wraps, and nothing else. `loom
commit -- --author=…` shapes the commit loom creates, not the rebase that
relocates it onto the feature branch.
