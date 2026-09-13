# CLAUDE.md

git-loom is a Rust 2024 CLI that combines independent feature branches on an
integration branch and rewrites/manages them without leaving that branch.

## Code Map

- `src/main.rs`: clap CLI/dispatch; command modules are `src/<command>.rs`.
- `src/core/`: graph, short IDs, repository, transaction, agent mode, weave.
- `src/git/`: low-level Git operations; `src/branch/`: new/merge/unmerge.
- `src/tui/`: status tree, shell, hunk selector, widgets, theme.
- `src/agent/`: `agent init`; embedded source is `skills/git-loom/SKILL.md`.
- Tests are sibling `*_test.rs` files; shared fixtures are in
  `src/core/test_helpers.rs`; integration tests are in `tests/integration/`.

## Specs

Specs are compact AI context. Read the relevant spec before changing behavior;
keep it terse and normative, and do not duplicate rules, docs, or examples.

| Spec | Feature |
| --- | --- |
| `specs/001-status.md` | Branch-aware status / commit graph display |
| `specs/002-shortid.md` | Short ID generation and collision resolution |
| `specs/003-reword.md` | Commit reword / branch rename via short IDs |
| `specs/004-weave.md` | Weave: structured graph model for topology-aware rebase |
| `specs/005-branch.md` | Branch creation and weaving into integration branches |
| `specs/006-commit.md` | Commit to feature branches from the integration branch |
| `specs/007-fold.md` | Fold: amend files, fixup commits, move commits between branches; hunk-level fold with `-p` |
| `specs/008-drop.md` | Drop commits or branches from history |
| `specs/009-init.md` | Initialize a new integration branch tracking a remote |
| `specs/010-update.md` | Pull-rebase integration branch and update submodules |
| `specs/011-push.md` | Push a feature branch and its stack to remote (plain, GitHub stacked PRs, GitLab, Azure, Gerrit) |
| `specs/012-absorb.md` | Absorb: auto-distribute changes into originating commits |
| `specs/013-split.md` | Split a commit into two commits by file or by hunk (`-p`) |
| `specs/014-continue-abort.md` | Continue or abort a paused loom operation |
| `specs/015-swap.md` | Swap two commits or two branch sections |
| `specs/016-diff.md` | Diff: short-ID–aware wrapper around git diff |
| `specs/017-switch.md` | Switch to any branch for testing without weaving |
| `specs/018-add.md` | Stage files using short IDs, paths, or `zz`; hunk-level staging with `-p` |
| `specs/019-agent.md` | Agent integration: `agent init` skill install and `--agent` machine-readable mode |
| `specs/020-tui.md` | Interactive status TUI: tree + diff panes, with actions |
| `specs/021-git-args.md` | Forwarding arguments to git after a `--` separator |

## Commands

`cargo build`; `cargo run`; `cargo test [name]`; `cargo clippy`; `cargo fmt`;
`cargo check`.

Keep `tests/bin_is_built.rs`: it makes `cargo test` build the binary used as the
sequence editor by rebase tests. Removing it breaks rebase tests in an unbuilt tree.

## Data Safety (Non-Negotiable)

No operation, including continue/abort, may discard staged, unstaged, or
uncommitted data.

`loom abort` first uses `git rebase --abort` (restores HEAD, `--update-refs`
branches, and autostash), then `Rollback::apply_abort()` applies populated fields:

| Field | Abort action / users |
| --- | --- |
| `reset_mixed_to` | mixed reset; `commit` |
| `reset_hard_to` | hard reset to pre-fixup HEAD; `absorb` |
| `delete_branches` | remove temp refs; `commit`, fold files/commit |
| `saved_staged_patch` | restore index; `commit`, `absorb`, fold files |
| `saved_worktree_patch` | restore worktree; `absorb` |

Every new resumable `weave::run_rebase` caller must populate `Rollback` before
saving `LoomState` and register in `transaction::dispatch_after_continue`.
There is no abort dispatcher; `Rollback::apply_abort()` owns cleanup.

Before rebase, `weave::run_rebase` must reject every moved branch checked out
in another non-prunable worktree (see `git/git_worktree.rs`, Spec 004), because
`update-ref` todo lines and rebase completion bypass Git's porcelain guard.

## Comments

Comments are AI context: keep them few and dense. A `///` doc comment is one
line saying what the item is, plus only the non-obvious contract — an
invariant, an ordering requirement, a data-safety rule, why a git flag is
there. A `//` comment explains *why*, never what the next line already says.

Do not write: comments restating the code or the signature, rustdoc
`# Arguments`/`# Returns`/`# Example` sections, multi-line ASCII banners
(one-line `// ── Name ──` if a long file needs sections), or a rationale
already given nearby or in a spec — link the spec instead.

In tests the function name, the fixture calls, and an assert's own label say
what is happening, so do not narrate them. Comment a test only to record why
it exists (the bug it guards), to draw the commit topology it builds, or to
flag a non-obvious fixture trick. Same for `tests/integration/*.sh`, where
`assert_* "..." "<name>"` already names the case.

## Error Convention

`run_git`/`run_git_stdout` failures log stderr only to trace. Never put Git
stderr in `bail!`; `main.rs` already tells users to run `loom trace`.

## Required Validation

After every code change, run `cargo fmt`, then `cargo test`; all tests must pass.
