use std::cell::RefCell;
use std::path::{Path, PathBuf};

use chrono::Local;

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

/// Initialize this thread's logger; a no-op if the thread already has one.
/// With `append`, entries go to the most recent existing log file, or to a new
/// one when there is none.
pub fn init(git_dir: &Path, command_line: &str, append: bool) {
    let append_to = if append {
        latest_log_path(git_dir)
    } else {
        None
    };
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
/// the logger state; a no-op if it was never initialized. An appending `init`
/// appends to the existing file.
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
#[path = "trace_log_test.rs"]
mod tests;
