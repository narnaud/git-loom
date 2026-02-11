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
        /// Short hashes of commits to mark as 'edit'
        #[arg(long = "edit")]
        edit_hashes: Vec<String>,
        /// Path to the git rebase todo file
        todo_file: String,
    },
}

fn main() {
    let cli = Cli::parse();

    if cli.no_color || std::env::var_os("NO_COLOR").is_some() {
        control::set_override(false);
    }

    let result = match cli.command {
        None | Some(Command::Status) => status::run(),
        Some(Command::Reword { target, message }) => reword::run(target, message),
        Some(Command::InternalSequenceEdit {
            edit_hashes,
            todo_file,
        }) => handle_sequence_edit(&edit_hashes, &todo_file),
    };

    if let Err(e) = result {
        eprintln!("error: {:#}", e);
        std::process::exit(1);
    }
}

fn handle_sequence_edit(
    edit_hashes: &[String],
    todo_file: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let actions: Vec<git_commands::git_rebase::RebaseAction> = edit_hashes
        .iter()
        .map(|h| git_commands::git_rebase::RebaseAction::Edit {
            short_hash: h.clone(),
        })
        .collect();

    git_commands::git_rebase::apply_actions_to_todo(&actions, todo_file)
}
