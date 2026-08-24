# show

Show the diff and metadata for a commit, like `git show`.

## Usage

```
git loom show [<target>] [git-options...] [-- <pathspec>...]
```

Alias: `sh`

### Arguments

| Argument | Description |
|----------|-------------|
| `<target>` | Commit hash, branch name, or short ID |

### Git Options

`show` defines no options of its own, so anything hyphenated is passed straight to `git show`:

```bash
git loom show --stat
git loom show -U5 ab
git loom show ab --stat        # options may follow the target too
git loom show ab -- src/main.rs
```

Only `-h`/`--help` is claimed by loom. Options that take a value must be attached — `-U5` or `--unified=5`, not `-U 5` — because loom has no list of git's value-taking options and would read the `5` as a second target, which `show` rejects. Everything after a `--` separator is a pathspec and is forwarded verbatim.

## What It Does

Displays the commit metadata (author, date, message) and diff for the resolved commit, exactly like `git show`. Uses git's native pager when running in a terminal.

- When given a **commit** (hash, partial hash, or short ID): shows that commit
- When given a **branch** (name or short ID): shows every commit the branch owns, newest first — the same commits `git loom status` lists under that branch, merge commits excluded. Naming a hidden branch shows it, even though `git loom status` leaves it out
- When given the **integration branch**: shows its loose commits, the ones `git loom status` puts on the integration line
- With **no target**: shows the commit at the top of `git loom status`

A branch that is not part of the integration stack — no upstream configured, a detached HEAD, or an unrelated history — falls back to plain `git show` behavior and displays only the branch tip. A branch that is part of the stack but owns no commits of its own is an error.

## Target Resolution

The target is resolved in this order:

1. **Branch names** — exact match resolves to the branch's commits
2. **Git references** — full/partial hashes, `HEAD`, etc. resolve to commits
3. **Short IDs** — branch short IDs resolve to the branch's commits, commit short IDs to commits

## Examples

### Show a commit by short ID

```bash
git loom show ab
# Displays commit info and diff for the commit with short ID "ab"
```

### Show a commit by hash

```bash
git loom show 9f484b6
```

### Show a whole branch

```bash
git loom show feature-a
# Shows every commit on feature-a, newest first
```

## Prerequisites

- Any git repository
- For short IDs, and to show a branch's full set of commits: must be on an integration branch with upstream tracking configured
