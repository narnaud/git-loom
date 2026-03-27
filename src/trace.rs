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

/// Initialize the logger for this invocation.
///
/// Call once from `main()` before dispatching the command.
/// No-op if already initialized (prevents double-init in subprocess).
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

/// Initialize the logger in append mode: entries will be appended to the
/// most recent existing log file rather than creating a new one.
///
/// Falls back to normal (new-file) mode if no prior log exists.
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

/// Log a command execution.
///
/// Safe to call when logger is not initialized (no-op).
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

/// Attach an annotation to the most recent log entry.
///
/// Used to record extra context like generated rebase todo content.
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

/// Write the log file and prune old logs. Returns the path written.
///
/// No-op if logger was never initialized. Consumes the logger state.
/// If initialized with `init_appending`, appends to the existing log file.
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

/// Print a log file's content with colored output.
fn print_log_colored(content: &str) {
    let mut lines = content.lines();

    // Header line: [timestamp] command
    if let Some(header) = lines.next() {
        println!("{}", header.bold());
    }
    // Separator line
    if let Some(sep) = lines.next() {
        println!("{}", sep.dimmed());
    }

    let mut in_stderr = false;

    for line in lines {
        if line.is_empty() {
            println!();
            in_stderr = false;
        } else if line.starts_with("  [") && !line.starts_with("    [") {
            // Command entry line
            in_stderr = false;
            if line.contains("FAILED") {
                let (before_failed, _) = line.rsplit_once(" FAILED").unwrap_or((line, ""));
                print!("{}", before_failed.cyan());
                println!(" {}", "FAILED".red().bold());
            } else {
                println!("{}", line.cyan());
            }
        } else if line.starts_with("    [stderr]") {
            in_stderr = true;
            println!("{}", line.red());
        } else if line.starts_with("    [") {
            // Annotation label
            in_stderr = false;
            println!("{}", line.yellow());
        } else if in_stderr {
            println!("{}", line.red());
        } else {
            // Annotation content
            println!("{}", line.dimmed());
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
        if !entry.success && !entry.stderr.is_empty() {
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

    // Header
    out.push_str(&format!(
        "[{}] {}\n",
        logger.start_time.format("%Y-%m-%d %H:%M:%S%.3f"),
        logger.command_line
    ));
    out.push_str(&"=".repeat(80));
    out.push('\n');

    for entry in &logger.entries {
        out.push('\n');

        // Command line with timing
        let status = if entry.success { "" } else { " FAILED" };
        out.push_str(&format!(
            "  [{}] {}  [{}ms]{}\n",
            entry.program, entry.args, entry.duration_ms, status
        ));

        // Annotations
        for (label, content) in &entry.annotations {
            out.push_str(&format!("    [{}]\n", label));
            for line in content.lines() {
                out.push_str(line);
                out.push('\n');
            }
        }

        // Stderr on failure
        if !entry.success && !entry.stderr.is_empty() {
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
