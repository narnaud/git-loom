# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

git-loom is a Git CLI tool written in Rust (edition 2024) that supercharges the Git workflow. Inspired by tools like jujutsu and Git Butler, it focuses on making integration branches seamless by weaving together multiple feature branches.

### Core Concepts

- **Integration branch**: A branch that merges multiple feature branches together, allowing you to work on and test several features simultaneously.
- **Feature branches**: Independent branches that are combined into the integration branch and can be managed (reordered, amended, split) without leaving the integration context.

### Key Features (planned/in progress)

- **Enhanced git log**: A nicer, more readable log output showing the relationship between integration and feature branches.
- **Easy amending**: Amend any commit in your branch stack, not just the latest one.
- **Commit mobility**: Move commits between branches or reorder them within a branch.
- **Branch creation**: Quickly create and manage feature branches from the integration branch.
- **Branch weaving**: Merge/unmerge feature branches into/from the integration branch.

### Architecture

- Entry point: `src/main.rs` — CLI parsing via `clap`, dispatches to subcommands.
- `src/status.rs` — Branch-aware commit graph display.
- `src/shortid.rs` — Compact human-friendly identifiers for branches, commits, and files.
- `src/reword.rs` — Commit message editing / branch renaming via short IDs.
- `src/graph.rs` — Graph rendering logic for the status output.
- `src/git.rs` — Git abstraction layer (uses `git2` crate).
- `src/git_commands/` — Lower-level Git operations split by domain:
  - `git_branch.rs`, `git_commit.rs`, `git_rebase.rs`
- `src/test_helpers.rs` — Shared test utilities (temp repos, etc.).
- Tests live alongside their modules as `*_test.rs` sibling files.

### Specs

The `specs/` directory contains detailed design documents that describe each feature's behavior, edge cases, and expected output. **Always consult the relevant spec before implementing or modifying a feature.**

| Spec | Feature |
|------|---------|
| `specs/001-status.md` | Branch-aware status / commit graph display |
| `specs/002-shortid.md` | Short ID generation and collision resolution |
| `specs/003-reword.md` | Commit reword / branch rename via short IDs |
| `specs/004-internal-sequence-edit.md` | Self-invocation as `GIT_SEQUENCE_EDITOR` for portable rebase |

## Build & Run Commands

- **Build:** `cargo build`
- **Run:** `cargo run`
- **Test:** `cargo test`
- **Run single test:** `cargo test <test_name>`
- **Lint:** `cargo clippy`
- **Format:** `cargo fmt`
- **Check (fast compile check):** `cargo check`
