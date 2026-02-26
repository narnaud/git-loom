# branch

Create a new feature branch at a specified commit.

## Usage

```
git-loom branch [name] [-t <target>]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `[name]` | Branch name (optional; prompts interactively if omitted) |

### Options

| Option | Description |
|--------|-------------|
| `-t, --target <target>` | Commit hash, short ID, or branch name (defaults to upstream merge-base) |

## What It Does

1. **Name resolution** — if no name is provided, an interactive prompt asks for one
2. **Validation** — the name is trimmed, checked for emptiness, validated against git's naming rules, and checked for duplicates
3. **Target resolution** — the target is resolved to a commit via the shared resolution system, or defaults to the merge-base
4. **Creation** — the branch is created at the resolved commit

### Automatic Weaving

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

## Target Resolution

The `-t` flag accepts:

- **Branch names** — resolves to the branch's tip commit
- **Git hashes** — full or partial commit hashes
- **Short IDs** — the compact IDs shown in `git loom status`
- **Default** — the merge-base between HEAD and upstream

## Examples

### Interactive

```bash
git-loom branch
# ? Branch name ›
# User types: feature-authentication
# Created branch 'feature-authentication' at abc1234
```

### At merge-base (default)

```bash
git-loom branch feature-auth
# Created branch 'feature-auth' at abc1234 (merge-base)
```

### At a specific commit by short ID

```bash
git-loom branch feature-auth -t ab
# Created branch 'feature-auth' at 72f9d3a
```

### At another branch's tip

```bash
git-loom branch feature-b -t feature-a
# Created branch 'feature-b' at feature-a's tip commit
```

## Prerequisites

- Must be in a git repository with a working tree
- For the default target: must have upstream tracking configured
