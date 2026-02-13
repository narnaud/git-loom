mod branch;
mod git;
mod git_commands;
mod graph;
mod reword;
mod shortid;
mod status;

#[cfg(test)]
mod test_helpers;

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

    if cli.no_color || std::env::var_os("NO_COLOR").is_some() {
        control::set_override(false);
    }

    if let Err(e) = git_commands::check_git_version() {
        eprintln!("error: {:#}", e);
        std::process::exit(1);
    }

    let result = match cli.command {
        None | Some(Command::Status) => status::run(),
        Some(Command::Branch { name, target }) => branch::run(name, target),
        Some(Command::Reword { target, message }) => reword::run(target, message),
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
    let actions: Vec<git_commands::git_rebase::RebaseAction> = serde_json::from_str(actions_json)?;

    git_commands::git_rebase::apply_actions_to_todo(&actions, todo_file)
}
