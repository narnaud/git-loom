# branch

Manage feature branches: create new branches, weave existing branches into the integration topology, or remove them.

**Alias:** `br`

## Subcommands

| Subcommand | Description |
|------------|-------------|
| `new` (alias: `create`) | Create a new feature branch at a specified commit |
| `merge` | Weave an existing branch into the integration branch |
| `unmerge` | Remove a branch from integration (keeps the branch ref) |

Running `git loom branch` without a subcommand defaults to `new`.

---

## branch new

Create a new feature branch at a specified commit.

### Usage

```
git-loom branch [name] [-t <target>]
git-loom branch new [name] [-t <target>]
git-loom branch create [name] [-t <target>]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `[name]` | Branch name (optional; prompts interactively if omitted) |

### Options

| Option | Description |
|--------|-------------|
| `-t, --target <target>` | Commit hash, short ID, or branch name (defaults to upstream merge-base) |

### What It Does

1. **Name resolution** — if no name is provided, an interactive prompt asks for one
2. **Validation** — the name is trimmed, checked for emptiness, validated against git's naming rules, and checked for duplicates
3. **Target resolution** — the target is resolved to a commit via the shared resolution system, or defaults to the merge-base
4. **Creation** — the branch is created at the resolved commit

#### Automatic Weaving

When a branch is created at a commit between the merge-base and HEAD, git-loom automatically **weaves** it into the integration branch — restructuring the linear history into a merge-based topology.

**Before** (linear):
```
origin/main → A1 → A2 → A3 → HEAD
```

**After** `git-loom branch feature-a -t A2`:
```
origin/main → A1 → A2 (feature-a)
                         ↘
              A3' -----→ merge (HEAD)
```

Weaving does **not** trigger when branching at HEAD or at the merge-base.

If the working tree has uncommitted changes, they are automatically stashed and restored after the operation.

### Target Resolution

The `-t` flag accepts:

- **Branch names** — resolves to the branch's tip commit
- **Git hashes** — full or partial commit hashes
- **Short IDs** — the compact IDs shown in `git loom status`
- **Default** — the merge-base between HEAD and upstream

### Examples

#### Interactive

```bash
git-loom branch
# ? Branch name ›
# User types: feature-authentication
# Created branch 'feature-authentication' at abc1234
```

#### At merge-base (default)

```bash
git-loom branch feature-auth
# Created branch 'feature-auth' at abc1234 (merge-base)
```

#### At a specific commit by short ID

```bash
git-loom branch feature-auth -t ab
# Created branch 'feature-auth' at 72f9d3a
```

#### At another branch's tip

```bash
git-loom branch feature-b -t feature-a
# Created branch 'feature-b' at feature-a's tip commit
```

---

## branch merge

Weave an existing branch into the integration branch using a merge commit.

### Usage

```
git-loom branch merge [branch] [--all]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `[branch]` | Branch name (optional; shows interactive picker if omitted) |

### Options

| Option | Description |
|--------|-------------|
| `-a, --all` | Also show remote branches without a local counterpart |

### What It Does

1. **Branch selection** — uses the provided name, or shows an interactive picker listing non-woven local branches
2. **Validation** — checks that the branch exists and is not already woven into integration
3. **Remote handling** — if a remote branch is selected (with `--all`), creates a local tracking branch automatically
4. **Merge** — performs a `git merge --no-ff` to weave the branch into the integration topology

### Examples

#### Merge a specific branch

```bash
git-loom branch merge feature-auth
# ✔ Woven 'feature-auth' into integration branch
```

#### Interactive picker

```bash
git-loom branch merge
# ? Select branch to weave ›
#   feature-auth
#   feature-logging
# ✔ Woven 'feature-auth' into integration branch
```

#### Include remote branches

```bash
git-loom branch merge --all
# ? Select branch to weave ›
#   feature-auth
#   origin/feature-logging
```

---

## branch unmerge

Remove a branch from the integration topology without deleting the branch ref.

### Usage

```
git-loom branch unmerge [branch]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `[branch]` | Branch name or short ID (optional; shows interactive picker if omitted) |

### What It Does

1. **Branch selection** — uses the provided name/short ID, or shows an interactive picker listing woven branches
2. **Validation** — checks that the branch is actually woven into the integration branch
3. **Unweave** — rebases the integration branch to remove the branch's merge topology
4. **Preserve** — the branch ref is kept intact, pointing at its original commits

This is different from `drop`, which deletes the branch entirely.

### Examples

#### Unmerge a specific branch

```bash
git-loom branch unmerge feature-auth
# ✔ Unwoven 'feature-auth' from integration branch
```

#### Interactive picker

```bash
git-loom branch unmerge
# ? Select branch to unmerge ›
#   feature-auth
#   feature-logging
# ✔ Unwoven 'feature-auth' from integration branch
```

---

## Hidden Branch Warning

If the branch name matches the configured hidden prefix (default: `local-`), git-loom prints a warning before the success message:

```
! Branch `local-secrets` is hidden from status by default. Use `--all` to show it.
✓ Created branch `local-secrets` at abc1234
```

See [Configuration](../configuration.md#loomhidebranchpattern) to customize the prefix.

## Reserved Names

The subcommand names `new`, `create`, `merge`, and `unmerge` are reserved and cannot be used as branch names.

## Prerequisites

- Must be in a git repository with a working tree
- For the default target: must have upstream tracking configured
