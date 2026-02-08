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

- Entry point: `src/main.rs`
- The project wraps and extends Git commands, calling Git under the hood.

## Build & Run Commands

- **Build:** `cargo build`
- **Run:** `cargo run`
- **Test:** `cargo test`
- **Run single test:** `cargo test <test_name>`
- **Lint:** `cargo clippy`
- **Format:** `cargo fmt`
- **Check (fast compile check):** `cargo check`
