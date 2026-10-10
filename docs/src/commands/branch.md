# branch

Create a feature branch at a specified commit, weaving it into the integration branch when needed.

**Alias:** `br`

To merge an existing branch into the integration branch, or take one out, see [`merge`](merge.md) and [`unmerge`](unmerge.md).

## Usage

```
git loom branch [name] [-t <target>]
```

## Arguments

| Argument | Description |
|----------|-------------|
| `[name]` | Branch name (optional; prompts interactively if omitted) |

## Options

| Option | Description |
|--------|-------------|
| `-t, --target <target>` | Commit hash, short ID, or branch name (defaults to upstream merge-base) |

## What It Does

1. **Name resolution** — if no name is provided, an interactive prompt asks for one
2. **Validation** — the name is trimmed, checked for emptiness, validated against git's naming rules, and checked for duplicates
3. **Target resolution** — the target is resolved to a commit via the shared resolution system, or defaults to the merge-base
4. **Creation** — the branch is created at the resolved commit

#### Automatic Weaving

When a branch is created at a commit on the **first-parent line** from HEAD to the merge-base, *git-loom* automatically **weaves** it into the integration branch — restructuring the linear history into a merge-based topology.

**Before** (linear):

```
origin/main → A1 → A2 → A3 → HEAD
```

**After** `git loom branch feature-a -t A2`:

```
              A1 → A2 (feature-a)
             /          \
origin/main               merge → A3' (HEAD)
```

All first-parent commits from the start up to (and including) the target move into the new branch section, which starts from the upstream base. Commits after the target are replayed where they were, always above the new merge since they were built on the target. When none sits between the target and a later merge, the new merge goes above every branch already woven, so the newest branch is on top.

**No-op cases** — weaving does not trigger and only the branch ref is created:

- **Branch at merge-base** — no commits to move into the branch.
- **Branch inside an existing side branch** — the target commit is already part of a merge topology (reachable through a merge second-parent), so no restructuring is needed.

**Branching at HEAD** weaves all current first-parent commits into the new branch:

```
git loom branch feature-a    # target = HEAD (all commits go into feature-a)
```

If the working tree has uncommitted changes, they are automatically stashed and restored after the operation, staged ones as staged.

If a weave rebase encounters conflicts, it aborts automatically and reports an error — no state is saved and no `loom continue` is available. Resolve the situation and retry.

## Target Resolution

The `-t` flag accepts:

1. **Branch names** — resolves to the branch's tip commit
2. **Git hashes** — full or partial commit hashes
3. **Short IDs** — the compact IDs shown in `git loom status`
4. **Default** — the merge-base between HEAD and upstream

## Examples

### Interactive

```bash
git loom branch
# ? Branch name ›
# User types: feature-authentication
# ✓ Created branch `feature-authentication` at abc1234
```

### At merge-base (default)

```bash
git loom branch feature-auth
# ✓ Created branch `feature-auth` at abc1234
```

### At a specific commit by short ID

```bash
git loom branch feature-auth -t osy
# ✓ Created branch `feature-auth` at `72f9d3a`
# ✓ Woven `feature-auth` into integration branch
```

### At another branch's tip

```bash
git loom branch feature-b -t feature-a
# ✓ Created branch `feature-b` at feature-a's tip commit
```

### Branching at HEAD (weaves all commits)

```bash
git loom branch feature-a
# ✓ Created branch `feature-a` at HEAD
# ✓ Woven `feature-a` into integration branch
```

## Hidden Branch Warning

If the branch name matches the configured hidden prefix (default: `local-`), *git-loom* prints a warning before the success message:

```
! Branch `local-secrets` is hidden from status by default. Use `--all` to show it.
✓ Created branch `local-secrets` at abc1234
```

See [Configuration](../configuration.md#loomhidebranchpattern) to customize the prefix.

## Prerequisites

- Must be in a git repository with a working tree
- For the default target: must have upstream tracking configured
- For short ID targets: must have upstream tracking configured
