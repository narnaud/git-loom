mod git;
mod graph;
mod reword;
mod shortid;
mod status;

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
        /// Short ID or commit hash to reword
        target: String,
        /// New message (if not provided, opens editor)
        #[arg(short, long)]
        message: Option<String>,
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
    };

    if let Err(e) = result {
        eprintln!("error: {:#}", e);
        std::process::exit(1);
    }
}
