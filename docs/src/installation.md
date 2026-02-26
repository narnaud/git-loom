# Installation

## Scoop (preferred)

Install git-loom with [Scoop](https://scoop.sh/):

```
scoop bucket add narnaud https://github.com/narnaud/scoop-bucket
scoop install git-loom
```

## Archive Files

1. Go to the [Releases](https://github.com/narnaud/git-loom/releases) page
2. Download the latest `git-loom-x86_64-pc-windows-msvc.zip` file
3. Extract the files into a directory on your `PATH`

## From Source

Requires Rust 1.90 or later.

```bash
git clone https://github.com/narnaud/git-loom.git
cd git-loom
cargo build --release
```

The binary will be available at `target/release/git-loom`. Add it to your PATH or install it globally with:

```bash
cargo install --path .
```

## Requirements

- **Git 2.38 or later** — git-loom checks the Git version at startup and will report an error if the version is too old.
- **`gh` CLI** (optional) — needed for automatic GitHub PR creation with `git loom push`. Install from [cli.github.com](https://cli.github.com).
