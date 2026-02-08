mod git;
mod graph;
mod log;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "git-loom", about = "Supercharge your Git workflow")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Show the branch-aware commit log
    Log,
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        None | Some(Command::Log) => log::run(),
    };

    if let Err(e) = result {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}
