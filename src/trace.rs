use std::cell::RefCell;
use std::path::{Path, PathBuf};

use chrono::Local;
use colored::Colorize;

/// A single logged command execution.
struct LogEntry {
    program: String,
    args: String,
    duration_ms: u128,
    success: bool,
    stderr: String,
    annotations: Vec<(String, String)>,
}

/// Per-invocation logger that records git/external commands to a log file.
struct LoomLogger {
    git_dir: PathBuf,
    command_line: String,
    start_time: chrono::DateTime<Local>,
    entries: Vec<LogEntry>,
    /// If set, append to this existing log file instead of creating a new one.
    append_to: Option<PathBuf>,
}

thread_local! {
    static LOGGER: RefCell<Option<LoomLogger>> = const { RefCell::new(None) };
}

/// Initialize the logger for this invocation. Call once from `main()` before
/// dispatching; a no-op if already initialized (a subprocess would double-init).
pub fn init(git_dir: &Path, command_line: &str) {
    LOGGER.with(|cell| {
        let mut logger = cell.borrow_mut();
        if logger.is_some() {
            return;
        }
        *logger = Some(LoomLogger {
            git_dir: git_dir.to_path_buf(),
            command_line: command_line.to_string(),
            start_time: Local::now(),
            entries: Vec::new(),
            append_to: None,
        });
    });
}

/// Initialize the logger in append mode: entries go to the most recent existing
/// log file, or to a new one when there is none.
pub fn init_appending(git_dir: &Path, command_line: &str) {
    let append_to = latest_log_path(git_dir);
    LOGGER.with(|cell| {
        let mut logger = cell.borrow_mut();
        if logger.is_some() {
            return;
        }
        *logger = Some(LoomLogger {
            git_dir: git_dir.to_path_buf(),
            command_line: command_line.to_string(),
            start_time: Local::now(),
            entries: Vec::new(),
            append_to,
        });
    });
}

/// Log a command execution; a no-op when the logger is not initialized.
pub fn log_command(program: &str, args: &str, duration_ms: u128, success: bool, stderr: &str) {
    LOGGER.with(|cell| {
        let mut logger = cell.borrow_mut();
        if let Some(ref mut l) = *logger {
            l.entries.push(LogEntry {
                program: program.to_string(),
                args: args.to_string(),
                duration_ms,
                success,
                stderr: stderr.to_string(),
                annotations: Vec::new(),
            });
        }
    });
}

/// Attach an annotation to the most recent log entry, e.g. generated rebase
/// todo content.
pub fn annotate(label: &str, content: &str) {
    LOGGER.with(|cell| {
        let mut logger = cell.borrow_mut();
        if let Some(ref mut l) = *logger
            && let Some(entry) = l.entries.last_mut()
        {
            entry
                .annotations
                .push((label.to_string(), content.to_string()));
        }
    });
}

/// Write the log file and prune old logs, returning the path written. Consumes
/// the logger state; a no-op if it was never initialized. In `init_appending`
/// mode, appends to the existing file.
pub fn finalize() -> Option<PathBuf> {
    LOGGER.with(|cell| {
        let logger = cell.borrow_mut().take()?;

        if logger.entries.is_empty() {
            return None;
        }

        let logs_dir = logger.git_dir.join("loom").join("logs");
        std::fs::create_dir_all(&logs_dir).ok()?;

        if let Some(ref path) = logger.append_to {
            let suffix = format_log_suffix(&logger);
            let mut file = std::fs::OpenOptions::new().append(true).open(path).ok()?;
            use std::io::Write;
            file.write_all(suffix.as_bytes()).ok()?;
            Some(path.clone())
        } else {
            let filename = logger
                .start_time
                .format("%Y-%m-%d_%H-%M-%S_%3f.log")
                .to_string();
            let path = logs_dir.join(&filename);
            let content = format_log(&logger);
            std::fs::write(&path, content).ok()?;
            prune_logs(&logs_dir, 10);
            Some(path)
        }
    })
}

/// Find the newest log file in the logs directory.
pub fn latest_log_path(git_dir: &Path) -> Option<PathBuf> {
    let logs_dir = git_dir.join("loom").join("logs");
    let mut entries: Vec<_> = std::fs::read_dir(&logs_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "log"))
        .collect();

    entries.sort_by_key(|e| e.file_name());
    entries.last().map(|e| e.path())
}

/// Print the latest log file to stdout with colors (opens repo internally).
pub fn run() -> anyhow::Result<()> {
    let repo = crate::core::repo::open_repo()?;
    let git_dir = repo.path().to_path_buf();
    print_latest_log(&git_dir)
}

/// Print the latest log file to stdout with colors.
pub fn print_latest_log(git_dir: &Path) -> anyhow::Result<()> {
    let path = latest_log_path(git_dir).ok_or_else(|| {
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
    /// `[timestamp] command`: the log's own, or one `init_appending` added.
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

/// Classify every line of a log file's content.
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

/// Format log entries to append to an existing log (adds a sub-header for the new command).
fn format_log_suffix(logger: &LoomLogger) -> String {
    let mut out = String::new();
    out.push('\n');
    out.push_str(&format!(
        "[{}] {}\n",
        logger.start_time.format("%Y-%m-%d %H:%M:%S%.3f"),
        logger.command_line
    ));
    out.push_str(&"-".repeat(80));
    out.push('\n');
    for entry in &logger.entries {
        out.push('\n');
        let status = if entry.success { "" } else { " FAILED" };
        out.push_str(&format!(
            "  [{}] {}  [{}ms]{}\n",
            entry.program, entry.args, entry.duration_ms, status
        ));
        for (label, content) in &entry.annotations {
            out.push_str(&format!("    [{}]\n", label));
            for line in content.lines() {
                out.push_str(line);
                out.push('\n');
            }
        }
        if !entry.stderr.is_empty() {
            out.push_str("    [stderr]\n");
            for line in entry.stderr.lines() {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

/// Format the entire log content (plain text for file storage).
fn format_log(logger: &LoomLogger) -> String {
    let mut out = String::new();

    out.push_str(&format!(
        "[{}] {}\n",
        logger.start_time.format("%Y-%m-%d %H:%M:%S%.3f"),
        logger.command_line
    ));
    out.push_str(&"=".repeat(80));
    out.push('\n');

    for entry in &logger.entries {
        out.push('\n');

        let status = if entry.success { "" } else { " FAILED" };
        out.push_str(&format!(
            "  [{}] {}  [{}ms]{}\n",
            entry.program, entry.args, entry.duration_ms, status
        ));

        for (label, content) in &entry.annotations {
            out.push_str(&format!("    [{}]\n", label));
            for line in content.lines() {
                out.push_str(line);
                out.push('\n');
            }
        }

        // Stderr (on success too — servers print MR/review links there)
        if !entry.stderr.is_empty() {
            out.push_str("    [stderr]\n");
            for line in entry.stderr.lines() {
                out.push_str(line);
                out.push('\n');
            }
        }
    }

    out
}

/// Keep only the newest `max_count` log files, removing the rest.
fn prune_logs(logs_dir: &Path, max_count: usize) {
    let mut entries: Vec<_> = std::fs::read_dir(logs_dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "log"))
        .collect();

    if entries.len() <= max_count {
        return;
    }

    // Sort by name ascending (oldest first due to timestamp format)
    entries.sort_by_key(|e| e.file_name());

    let to_remove = entries.len() - max_count;
    for entry in entries.into_iter().take(to_remove) {
        let _ = std::fs::remove_file(entry.path());
    }
}

#[cfg(test)]
#[path = "trace_test.rs"]
mod tests;
