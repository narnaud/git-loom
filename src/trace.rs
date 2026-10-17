use std::path::Path;

use colored::Colorize;

use crate::core::trace_log::{self, TraceLine};

/// Print the latest log file to stdout with colors (opens repo internally).
pub fn run() -> anyhow::Result<()> {
    let repo = crate::core::repo::open_repo()?;
    let git_dir = repo.path().to_path_buf();
    print_latest_log(&git_dir)
}

/// Print the latest log file to stdout with colors.
pub fn print_latest_log(git_dir: &Path) -> anyhow::Result<()> {
    let path = trace_log::latest_log_path(git_dir).ok_or_else(|| {
        anyhow::anyhow!("No log files found\nRun a command first to generate a log")
    })?;
    let content = std::fs::read_to_string(&path)?;
    print_log_colored(&content);
    let display_path = path.display().to_string().replace('\\', "/");
    println!("\nLog path: {}", display_path);
    Ok(())
}

/// Print a log file's content with colored output.
fn print_log_colored(content: &str) {
    for line in trace_log::classify_log(content) {
        match line {
            TraceLine::Blank => println!(),
            TraceLine::Header(text) => println!("{}", text.bold()),
            TraceLine::Rule(text) | TraceLine::Content(text) => println!("{}", text.dimmed()),
            TraceLine::Command { text, failed } => {
                print!("{}", text.cyan());
                if failed {
                    print!(" {}", "FAILED".red().bold());
                }
                println!();
            }
            TraceLine::Stderr(text) => println!("{}", text.red()),
            TraceLine::Label(text) => println!("{}", text.yellow()),
        }
    }
}
