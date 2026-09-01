# continue

Resume a paused loom operation after resolving rebase conflicts.

## Usage

```
git loom continue
```

## When to Use It

When an in-scope loom command (commit, update, absorb, drop, fold) encounters a
rebase conflict, it **pauses** instead of aborting. The operation is saved to
`.git/loom/state.json` and the process exits with code 0.

The terminal output shows which files are conflicted and what to do next:

```
! Conflicts detected — resolve them with git, then run:
  loom continue   to complete the commit
  loom abort      to cancel and restore original state
```

Once you've resolved conflicts and staged the resolution:

```bash
# resolve conflicts in your editor, then:
git add <resolved-files>
git loom continue
```

## What It Does

1. Loads the saved state from `.git/loom/state.json`
2. If a rebase is still in progress, runs `git rebase --continue`
   - If that stops again (a **conflict**, or anything else): stays paused, keeps the state file, exits successfully
   - If it only reaches the next **`edit` step**: stays paused, keeps the state file, says so
   - If it completes: moves on
3. If no rebase is in progress (e.g. you already ran `git rebase --continue` manually): skips to dispatch
4. Dispatches to the interrupted command's post-rebase work (restoring staged patches, printing the success message, etc.)
5. Deletes the state file on success

## Double Conflicts

If your branch has multiple conflicting commits, each `loom continue` may hit
a new conflict at the next commit. Repeat the resolve-and-continue cycle as
many times as needed:

```bash
git loom commit -b feature-auth -m "add auth" zz
# ! Conflicts detected...

git add auth.rs && git loom continue
# ! Conflicts remain — resolve them and run `loom continue` again

git add shared.rs && git loom continue
# ✓ Created commit `a1b2c3d` on branch `feature-auth`
```

## Which Commands Are Paused

| Command | Pauseable |
|---------|-----------|
| `update` | ✓ |
| `commit` | ✓ |
| `absorb` | ✓ |
| `drop <commit>` | ✓ |
| `reword <commit>` | ✓ |
| `fold` (simple paths) | ✓ |
| `drop <branch>` | — (aborts immediately) |
| `reword <branch>` | — (a rename never rebases) |
| `split` | — (aborts immediately) |
| `fold` (edit/multi-phase paths) | — (aborts immediately) |

## Commands Allowed While Paused

While a loom operation is paused, most commands are blocked. The following are still available:

- `show` — inspect commits
- `trace` — check recent command output
- `continue` — resume the paused operation
- `abort` — cancel the paused operation

## Without a Saved State File

If git has a rebase or merge in progress that no loom state file describes,
`loom continue` finishes it (`git rebase --continue`) and reports the outcome —
including a rebase that only reached the next `edit` step, below.

## Pausing at an `edit` Step

`git rebase --continue` exits successfully when it only advances to the next
`edit` step, so a completed run and a rebase that is still half-done look the
same from outside. Loom checks and says which one happened:

```
! The `loom drop` is paused at an `edit` step — finish the work there, then run:
  `loom continue`   to carry on
  `loom abort`      to cancel and restore original state
```

With no state file there is no command to name, so the line opens with
"The rebase is paused at an `edit` step" instead.

## Error: No Operation in Progress

With neither a state file nor a rebase or merge in progress:

```bash
git loom continue
# error: No loom operation is in progress
```

## See Also

- [`abort`](abort.md) — cancel instead of resuming
