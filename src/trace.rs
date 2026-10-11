use std::path::Path;

use colored::Colorize;

use crate::core::trace_log;

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

/// One line of a log file, by what it says.
#[derive(Debug, PartialEq, Eq)]
pub enum TraceLine<'a> {
    Blank,
    /// `[timestamp] command`: the log's own, or one an appending `init` added.
    Header(&'a str),
    /// The rule under a header.
    Rule(&'a str),
    /// A logged command; `failed` when it ended with ` FAILED`, cut off `text`.
    Command {
        text: &'a str,
        failed: bool,
    },
    /// `[stderr]` and the lines under it.
    Stderr(&'a str),
    /// Any other `[label]`.
    Label(&'a str),
    /// What follows a label.
    Content(&'a str),
}

/// Classify every line of a log file's content, by the layout
/// [`trace_log`] writes: a change there must be mirrored here.
pub fn classify_log(content: &str) -> Vec<TraceLine<'_>> {
    let is_rule = |line: &str| {
        line.len() == 80 && (line.bytes().all(|b| b == b'=') || line.bytes().all(|b| b == b'-'))
    };
    let lines: Vec<&str> = content.lines().collect();
    // Logged content can start with `[` or hold a rule, so a header is told by
    // where the writer puts one: first or after a blank line, over a rule.
    let is_header = |i: usize| {
        lines[i].starts_with('[')
            && (i == 0 || lines[i - 1].is_empty())
            && lines.get(i + 1).is_some_and(|next| is_rule(next))
    };
    // A blank line leaves `in_stderr` alone: stderr can hold one, and the
    // writer's own come before a command or header, which reset it.
    let mut in_stderr = false;
    lines
        .iter()
        .enumerate()
        .map(|(i, &line)| {
            if line.is_empty() {
                TraceLine::Blank
            } else if is_header(i) {
                in_stderr = false;
                TraceLine::Header(line)
            } else if i > 0 && is_header(i - 1) {
                TraceLine::Rule(line)
            } else if line.starts_with("  [") && !line.starts_with("    [") {
                in_stderr = false;
                match line.strip_suffix(" FAILED") {
                    Some(text) => TraceLine::Command { text, failed: true },
                    None => TraceLine::Command {
                        text: line,
                        failed: false,
                    },
                }
            } else if line.starts_with("    [stderr]") {
                in_stderr = true;
                TraceLine::Stderr(line)
            } else if line.starts_with("    [") {
                in_stderr = false;
                TraceLine::Label(line)
            } else if in_stderr {
                TraceLine::Stderr(line)
            } else {
                TraceLine::Content(line)
            }
        })
        .collect()
}

/// Print a log file's content with colored output.
fn print_log_colored(content: &str) {
    for line in classify_log(content) {
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

#[cfg(test)]
#[path = "trace_test.rs"]
mod tests;
