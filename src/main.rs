mod branch;
mod commit;
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
    /// Internal: used as GIT_SEQUENCE_EDITOR to apply rebase actions
    #[command(hide = true)]
    InternalSequenceEdit {
        /// JSON-encoded list of rebase actions
        #[arg(long = "actions-json")]
        actions_json: String,
        /// Path to the git rebase todo file
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
        Some(Command::InternalSequenceEdit {
            actions_json,
            todo_file,
        }) => handle_sequence_edit(&actions_json, &todo_file),
    };

    if let Err(e) = result {
        eprintln!("error: {:#}", e);
        std::process::exit(1);
    }
}

fn handle_sequence_edit(
    actions_json: &str,
    todo_file: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let actions: Vec<git_commands::git_rebase::RebaseAction> = serde_json::from_str(actions_json)
        .map_err(|e| {
        format!(
            "Failed to parse --actions-json: {}. Value was: {}",
            e,
            &actions_json[..actions_json.len().min(200)]
        )
    })?;

    git_commands::git_rebase::apply_actions_to_todo(&actions, todo_file)
}
