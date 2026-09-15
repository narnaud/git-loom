# Spec 015: Swap

This specification is normative.

## Command

```bash
git-loom swap <a> <b>
```

`<a>` and `<b>` each accept a full or partial commit OID or a two-character
loom short ID. They are resolved with `resolve_arg()` and `accept = [Commit]`;
branch names are not accepted. See Spec 002.

## Behavior

Both commits must be woven into the current integration topology and occupy
the same sequence: either the same branch section or the direct integration
line. Loom exchanges their positions in the rebase todo, replays descendants,
and updates affected branch refs. Other commits retain their relative order;
commit content and messages are preserved; unrelated branches are unaffected.

Successful immediate completion prints:

```text
Swapped commits '<a>' and '<b>'
```

The displayed values are the command's resolved display identifiers.
Uncommitted worktree changes must be preserved through `git rebase
--autostash`.

## Errors

The following messages are required:

| Condition | Error |
| --- | --- |
| Both arguments resolve to one OID | `Cannot swap a commit with itself` |
| Commits belong to different branch sections | `Cannot swap commits from different branch sections` |
| One commit is in a branch section and one is on the integration line | `Cannot swap commits from different locations (branch section vs integration line)` |
| A resolved commit is outside the weave graph | `Commit <oid> not found in weave graph` |

Cross-section relocation and arbitrary reordering are not swaps; use `loom fold <commit> --above|--below <commit>` (Spec 007).

## Conflict, Continue, and Abort

On a rebase conflict, loom saves `.git/loom/state.json` and pauses. The user
must resolve and stage conflicts, then run `loom continue`, or run `loom abort`
to restore the original state. `LoomState.context` must contain string fields
`display_a` and `display_b`, holding the short hashes of the first and second
commits. Successful completion through `after_continue` prints:

```text
Swapped '<a>' and '<b>'
```

See Spec 014 for the normative continue/abort and data-restoration contract.

## Example

```bash
git-loom swap aa bb
```

This swaps commits `aa` and `bb` when both are in the same branch section or
both are direct integration-line picks. No confirmation is required.
