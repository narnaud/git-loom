# git-loom

> Supercharge your Git workflow by weaving together multiple feature branches

**git-loom** is a Git CLI tool that makes working with integration branches seamless. Inspired by tools like [jujutsu](https://github.com/martinvonz/jj) and [Git Butler](https://gitbutler.com/), git-loom helps you work on multiple features simultaneously while keeping your branches organized and independent.

## Status

🚧 **Early Development** - git-loom is currently in active development. The core infrastructure is in place, with more features being added.

- [x] Git repository analysis and graph building
- [x] Enhanced `status` command with branch-aware output

## What is git-loom?

git-loom introduces the concept of **integration branches** - a special branch that weaves together multiple feature branches, allowing you to:

- Work on several features at once in a single branch
- Test how features interact with each other
- Keep feature branches independent and manageable
- Easily amend, reorder, or move commits between branches
- See a clear relationship between your integration and feature branches

Think of it as a loom that weaves multiple threads (feature branches) into a single fabric (integration branch).

## Installation

### From Source

Requires Rust 1.90 or later.

```bash
git clone https://github.com/yourusername/git-loom.git
cd git-loom
cargo build --release
```

The binary will be available at `target/release/git-loom`.

Add it to your PATH or use `cargo install --path .` to install it globally.

## Usage

```
Usage: git-loom.exe [OPTIONS] [COMMAND]

Commands:
  status  Show the branch-aware status
  help    Print this message or the help of the given subcommand(s)

Options:
      --no-color  Disable colored output
  -h, --help      Print help
```

## Core Concepts

### Integration Branch

A branch that merges multiple feature branches together. This allows you to:

- Work on multiple features in a single context
- Test how features work together
- See the combined state of your work

### Feature Branches

Independent branches that are combined into the integration branch. You can manage them (reorder, amend, split) without leaving the integration context.

## Contributing

Contributions are welcome! This project is in early development, so there's plenty of room for new ideas and improvements.

## License

MIT License - Copyright (c) Nicolas Arnaud-Cormos

See [LICENSE](LICENSE) file for details.

## Acknowledgments

Inspired by:
- [jujutsu](https://github.com/martinvonz/jj) - A Git-compatible VCS with powerful features for managing complex workflows
- [Git Butler](https://gitbutler.com/) - A Git client that makes working with virtual branches easy
