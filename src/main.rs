mod branch;
mod commit;
mod completions;
mod drop;
mod fold;
mod git;
mod git_commands;
mod graph;
mod init;
mod reword;
mod shortid;
mod status;
mod update;
mod weave;

#[cfg(test)]
mod test_helpers;

use std::io::IsTerminal;

use clap::{Parser, Subcommand};
use colored::control;

#[derive(Parser)]
#[command(name = "git-loom", about = "Supercharge your Git workflow")]
struct Cli {
    /// Disable colored output
    #[arg(long)]
    no_color: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Show the branch-aware status
    Status,
    /// Initialize a new integration branch tracking a remote
    Init {
        /// Branch name (defaults to "loom")
        name: Option<String>,
    },
    /// Create a new feature branch
    Branch {
        /// Branch name (if not provided, will prompt interactively)
        name: Option<String>,
        /// Target commit, branch, or shortID (defaults to upstream base)
        #[arg(short = 't', long = "target")]
        target: Option<String>,
    },
    /// Reword a commit message or rename a branch
    Reword {
        /// Branch name, shortID, or commit hash
        target: String,
        /// New message or branch name (if not provided, opens editor for commits)
        #[arg(short, long)]
        message: Option<String>,
    },
    /// Create a commit on a feature branch without leaving integration
    Commit {
        /// Target feature branch (name or short ID)
        #[arg(short = 'b', long = "branch")]
        branch: Option<String>,
        /// Commit message (if not provided, opens editor)
        #[arg(short, long)]
        message: Option<String>,
        /// Files to stage (short IDs, filenames, or 'zz' for all)
        files: Vec<String>,
    },
    /// Drop a commit or a branch from history
    Drop {
        /// Commit hash, branch name, or short ID to drop
        target: String,
    },
    /// Fold source(s) into a target (amend files, fixup commits, move commits)
    Fold {
        /// Source(s) and target: files, commits, or branches (last arg is the target)
        #[arg(required = true, num_args = 2..)]
        args: Vec<String>,
    },
    /// Pull-rebase the integration branch and update submodules
    Update,
    /// Generate shell completions (powershell, clink)
    Completions {
        /// Shell to generate completions for (powershell, clink)
        shell: String,
    },
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
            eprintln!("error: {:#}", e);
            std::process::exit(1);
        }
        return;
    }

    if let Err(e) = git_commands::check_git_version() {
        eprintln!("error: {:#}", e);
        std::process::exit(1);
    }

    let result = match cli.command {
        None | Some(Command::Status) => status::run(),
        Some(Command::Init { name }) => init::run(name),
        Some(Command::Branch { name, target }) => branch::run(name, target),
        Some(Command::Reword { target, message }) => reword::run(target, message),
        Some(Command::Commit {
            branch,
            message,
            files,
        }) => commit::run(branch, message, files),
        Some(Command::Drop { target }) => drop::run(target),
        Some(Command::Update) => update::run(),
        Some(Command::Fold { args }) => fold::run(args),
        Some(Command::Completions { .. }) => unreachable!(),
        Some(Command::InternalWriteTodo { source, todo_file }) => {
            handle_write_todo(&source, &todo_file)
        }
    };

    if let Err(e) = result {
        eprintln!("error: {:#}", e);
        std::process::exit(1);
    }
}

fn handle_write_todo(source: &str, todo_file: &str) -> Result<(), Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(source)
        .map_err(|e| format!("Failed to read source file '{}': {}", source, e))?;
    std::fs::write(todo_file, content)
        .map_err(|e| format!("Failed to write todo file '{}': {}", todo_file, e))?;
    Ok(())
}
