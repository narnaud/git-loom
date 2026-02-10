mod git;
mod graph;
mod reword;
mod shortid;
mod status;

#[cfg(test)]
mod test_helpers;

use clap::{Parser, Subcommand};
use colored::control;
use std::io::Write;

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
    /// Internal: used as GIT_SEQUENCE_EDITOR to mark a commit for editing
    #[command(hide = true)]
    InternalSequenceEdit {
        /// Short hash of the commit to mark as 'edit'
        short_hash: String,
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
            short_hash,
            todo_file,
        }) => handle_sequence_edit(&short_hash, &todo_file),
    };

    if let Err(e) = result {
        eprintln!("error: {:#}", e);
        std::process::exit(1);
    }
}

fn handle_sequence_edit(
    short_hash: &str,
    todo_file: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(todo_file)?;
    let mut found = false;
    let mut output = String::with_capacity(content.len());

    for line in content.lines() {
        if !found && line.starts_with(&format!("pick {}", short_hash)) {
            output.push_str(&format!("edit {}", &line["pick".len()..]));
            found = true;
        } else {
            output.push_str(line);
        }
        output.push('\n');
    }

    if !found {
        writeln!(
            std::io::stderr(),
            "warning: commit {} not found in rebase todo",
            short_hash
        )?;
    }

    std::fs::write(todo_file, output)?;
    Ok(())
}
