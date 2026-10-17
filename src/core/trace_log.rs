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

/// Log a command execution; a no-op when the logger is not initialized. URL
/// credentials in `args` are left out: bug reports quote the log.
pub fn log_command(program: &str, args: &str, duration_ms: u128, success: bool, stderr: &str) {
    LOGGER.with(|cell| {
        let mut logger = cell.borrow_mut();
        if let Some(ref mut l) = *logger {
            l.entries.push(LogEntry {
                program: program.to_string(),
                args: args
                    .split(' ')
                    .map(without_credentials)
                    .collect::<Vec<_>>()
                    .join(" "),
                duration_ms,
                success,
                stderr: stderr.to_string(),
                annotations: Vec::new(),
            });
        }
    });
}

/// `url` for display or the log, without the user and password of an
/// `http(s)://` URL: either can be a token. A github.com URL carries
/// credentials no other way.
pub fn without_credentials(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return url.to_string();
    }
    let authority = rest.find('/').map_or(rest, |slash| &rest[..slash]);
    match authority.rfind('@') {
        Some(at) => format!("{}://{}", scheme, &rest[at + 1..]),
        None => url.to_string(),
    }
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
/// [`format_log`] writes: a change there must be mirrored here.
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

#[cfg(test)]
#[path = "trace_log_test.rs"]
mod tests;
