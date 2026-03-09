mod absorb;
mod branch;
mod commit;
mod completions;
mod drop;
mod fold;
mod git;
mod git_commands;
mod graph;
mod init;
mod msg;
mod push;
mod reword;
mod shortid;
mod show;
mod split;
mod status;
mod trace;
mod update;
mod weave;

#[cfg(test)]
mod test_helpers;

use std::io::IsTerminal;

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use colored::control;

#[derive(ValueEnum, Clone, Copy)]
enum ThemeArg {
    /// Detect from terminal background color (default to dark if undetectable)
    Auto,
    /// Dark terminal background
    Dark,
    /// Light terminal background
    Light,
}

#[derive(Parser)]
#[command(name = "git-loom", about = "Supercharge your Git workflow", version)]
struct Cli {
    /// Disable colored output
    #[arg(long)]
    no_color: bool,

    /// Color theme for graph output
    #[arg(long, default_value = "auto")]
    theme: ThemeArg,

    /// Show files changed in each commit (optionally filtered to specific commits)
    #[arg(short = 'f', long = "files", num_args = 0.., hide = true)]
    files: Option<Vec<String>>,

    /// Number of context commits to show before the base
    #[arg(default_value = "1", hide = true)]
    context: usize,

    /// Show all branches including hidden ones (those matching loom.hideBranchPattern)
    #[arg(short = 'a', long = "all", hide = true)]
    all: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Initialize a new integration branch tracking a remote
    Init {
        /// Branch name (defaults to "integration")
        name: Option<String>,
    },
    /// Show the branch-aware status
    Status {
        /// Show files changed in each commit (optionally filtered to specific commits)
        #[arg(short = 'f', long = "files", num_args = 0.., value_name = "COMMIT")]
        files: Option<Vec<String>>,
        /// Number of context commits to show before the base
        #[arg(default_value = "1")]
        context: usize,
        /// Show all branches including hidden ones (those matching loom.hideBranchPattern)
        #[arg(short = 'a', long = "all")]
        all: bool,
    },
    /// Create a commit on a feature branch without leaving integration
    Commit {
        /// Target feature branch (name or short ID)
        #[arg(short = 'b', long = "branch")]
        branch: Option<String>,
        /// Commit message (if not provided, opens editor)
        #[arg(short, long)]
        message: Option<String>,
        /// Files to stage (short IDs, filenames, or 'zz' for all), none for all tracked changes
        files: Vec<String>,
    },
    /// Fold source(s) into a target (amend files, fixup commits, move commits, move files between commits)
    Fold {
        /// Create a new branch from the source commit and move it there
        #[arg(short = 'c', long = "create")]
        create: bool,
        /// Source(s) and target: files, commits, or branches (last arg is the target)
        #[arg(required = true, num_args = 2..)]
        args: Vec<String>,
    },
    /// Reword a commit message or rename a branch
    Reword {
        /// Branch name, shortID, or commit hash
        target: String,
        /// New message or branch name (if not provided, opens editor for commits)
        #[arg(short, long)]
        message: Option<String>,
    },
    /// Drop a local change, a commit, or a branch from history
    Drop {
        /// Commit hash, branch name, or short ID to drop
        target: String,
        /// Skip confirmation prompt
        #[arg(short, long)]
        yes: bool,
    },
    /// Show the diff and metadata for a commit (like `git show`)
    Show {
        /// Commit hash, branch name, or short ID
        target: String,
    },
    /// Split a commit into two sequential commits
    Split {
        /// Commit hash, short ID, or HEAD
        target: String,
        /// Message for the first commit (prompts if omitted)
        #[arg(short, long)]
        message: Option<String>,
    },
    /// Absorb working tree changes into the commits that introduced them
    Absorb {
        /// Show what would be absorbed without making changes
        #[arg(short = 'n', long)]
        dry_run: bool,
        /// Files to restrict absorption to (default: all tracked changed files)
        files: Vec<String>,
    },
    /// Create a new feature branch, or a stacked branch
    Branch {
        /// Branch name (if not provided, will prompt interactively)
        name: Option<String>,
        /// Target commit, branch, or shortID (defaults to upstream base)
        #[arg(short = 't', long = "target")]
        target: Option<String>,
    },
    /// Push a feature branch to remote and optionally create a PR or Gerrit review
    Push {
        /// Branch name or short ID (if not provided, will prompt interactively)
        branch: Option<String>,
        /// Push branch without creating a PR or Gerrit review
        #[arg(long)]
        no_pr: bool,
    },
    /// Pull-rebase the integration branch and update submodules
    Update {
        /// Remove local branches whose upstream tracking branch was deleted on remote
        #[arg(short, long)]
        yes: bool,
    },
    /// Generate shell completions (powershell, clink)
    #[command(hide = true)]
    Completions {
        /// Shell to generate completions for (powershell, clink)
        shell: String,
    },
    /// Show the latest command trace
    Trace,
    /// Internal: used as GIT_SEQUENCE_EDITOR to write a pre-generated todo file
    #[command(hide = true)]
    InternalWriteTodo {
        /// Path to the source file containing the todo content
        #[arg(long = "source")]
        source: String,
        /// Path to the git rebase todo file (provided by git)
        todo_file: String,
    },
}

fn main() {
    let cli = Cli::parse();

    if cli.no_color
        || std::env::var_os("NO_COLOR").is_some()
        || std::env::var_os("TERM").is_some_and(|v| v == "dumb")
        || !std::io::stdout().is_terminal()
    {
        control::set_override(false);
    }

    // Completions don't need git, handle before version check
    if let Some(Command::Completions { shell }) = cli.command {
        if let Err(e) = completions::run(shell) {
            msg::error(&e.to_string());
            std::process::exit(1);
        }
        return;
    }

    if let Err(e) = git_commands::check_git_version() {
        msg::error(&e.to_string());
        std::process::exit(1);
    }

    // Initialize logger for commands that modify the repo (skip for
    // InternalWriteTodo — it runs as a subprocess — and Log/Status which are read-only).
    let should_log = !matches!(
        cli.command,
        Some(Command::InternalWriteTodo { .. }) | Some(Command::Trace) | Some(Command::Show { .. })
    );
    if should_log
        && let Ok(repo) = git::open_repo()
        && let Some(git_dir) = repo.workdir().map(|w| w.join(".git"))
    {
        let cmd_line = std::env::args().collect::<Vec<_>>().join(" ");
        trace::init(&git_dir, &cmd_line);
    }

    let theme = resolve_theme(cli.theme);

    let result = match cli.command {
        None => status::run(cli.files, cli.context, cli.all, theme),
        Some(Command::Status {
            files,
            context,
            all,
        }) => status::run(files, context, all, theme),
        Some(Command::Init { name }) => init::run(name),
        Some(Command::Branch { name, target }) => branch::run(name, target),
        Some(Command::Reword { target, message }) => reword::run(target, message),
        Some(Command::Commit {
            branch,
            message,
            files,
        }) => commit::run(branch, message, files),
        Some(Command::Drop { target, yes }) => drop::run(target, yes),
        Some(Command::Absorb { dry_run, files }) => absorb::run(dry_run, files),
        Some(Command::Show { target }) => show::run(target),
        Some(Command::Split { target, message }) => split::run(target, message),
        Some(Command::Push { branch, no_pr }) => push::run(branch, no_pr),
        Some(Command::Update { yes }) => update::run(yes),
        Some(Command::Fold { create, args }) => fold::run(create, args),
        Some(Command::Trace) => handle_trace(),
        Some(Command::Completions { .. }) => unreachable!(),
        Some(Command::InternalWriteTodo { source, todo_file }) => {
            handle_write_todo(&source, &todo_file)
        }
    };

    trace::finalize();

    if let Err(e) = result {
        msg::error(&e.to_string());
        std::process::exit(1);
    }
}

fn handle_trace() -> anyhow::Result<()> {
    let repo = git::open_repo()?;
    let git_dir = repo
        .workdir()
        .map(|w| w.join(".git"))
        .ok_or_else(|| anyhow::anyhow!("Not a working directory"))?;

    trace::print_latest_log(&git_dir)
}

fn resolve_theme(arg: ThemeArg) -> graph::Theme {
    match arg {
        ThemeArg::Dark => graph::Theme::dark(),
        ThemeArg::Light => graph::Theme::light(),
        ThemeArg::Auto => {
            if !std::io::stdout().is_terminal() {
                return graph::Theme::dark();
            }
            use terminal_colorsaurus::{QueryOptions, ThemeMode, theme_mode};
            match theme_mode(QueryOptions::default()) {
                Ok(ThemeMode::Light) => graph::Theme::light(),
                _ => graph::Theme::dark(),
            }
        }
    }
}

fn handle_write_todo(source: &str, todo_file: &str) -> anyhow::Result<()> {
    // Save the original git todo to a sidecar file (for logging)
    if let Ok(original) = std::fs::read_to_string(todo_file) {
        let sidecar = format!("{}.original", source);
        let _ = std::fs::write(sidecar, original);
    }

    let content = std::fs::read_to_string(source)
        .with_context(|| format!("Failed to read source file '{}'", source))?;
    std::fs::write(todo_file, content)
        .with_context(|| format!("Failed to write todo file '{}'", todo_file))?;
    Ok(())
}
