use std::io::{self, Write};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;

use colored::{ColoredString, Colorize};

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// A spinner that shows progress and resolves to a success or error state.
pub struct Spinner {
    running: Arc<AtomicBool>,
    thread: Mutex<Option<thread::JoinHandle<()>>>,
}

/// Create a new spinner. Call `.start()` to begin, then `.stop()` or `.error()`.
pub fn spinner() -> Spinner {
    Spinner {
        running: Arc::new(AtomicBool::new(false)),
        thread: Mutex::new(None),
    }
}

impl Spinner {
    /// Start the spinner with the given message.
    pub fn start(&self, msg: &str) {
        let running = Arc::clone(&self.running);
        running.store(true, Ordering::SeqCst);
        let msg = msg.to_string();
        let handle = thread::spawn(move || {
            let mut i = 0usize;
            while running.load(Ordering::SeqCst) {
                print!(
                    "\r{} {}",
                    SPINNER_FRAMES[i % SPINNER_FRAMES.len()].cyan(),
                    msg
                );
                let _ = io::stdout().flush();
                i += 1;
                thread::sleep(Duration::from_millis(80));
            }
        });
        *self.thread.lock().unwrap() = Some(handle);
    }

    fn finish(&self, symbol: ColoredString, msg: &str) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.thread.lock().unwrap().take() {
            let _ = handle.join();
        }
        // \r returns to line start; \x1b[K clears to end of line
        println!("\r{} {}\x1b[K", symbol, msg);
    }

    /// Stop the spinner with a success message.
    pub fn stop(&self, msg: &str) {
        self.finish("✓".green(), msg);
    }

    /// Stop the spinner with an error message.
    pub fn error(&self, msg: &str) {
        self.finish("✗".red(), msg);
    }
}

/// Replace text between backticks with yellow-colored text.
fn colorize_backticks(message: &str) -> String {
    let mut result = String::new();
    let mut rest = message;
    while let Some(start) = rest.find('`') {
        result.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        if let Some(end) = after.find('`') {
            result.push_str(&format!("{}", after[..end].yellow()));
            rest = &after[end + 1..];
        } else {
            result.push_str(rest);
            return result;
        }
    }
    result.push_str(rest);
    result
}

/// Print a success message with a green checkmark.
/// Text between backticks is highlighted in yellow.
pub fn success(message: &str) {
    println!("{} {}", "✓".green(), colorize_backticks(message));
}

/// Print an error message with a red cross to stderr.
/// Additional lines are treated as hints and prefixed with a blue arrow.
/// Text between backticks is highlighted in yellow.
pub fn error(message: &str) {
    let mut lines = message.lines();
    if let Some(first) = lines.next() {
        eprintln!("{} {}", "×".red(), colorize_backticks(first));
        for line in lines {
            eprintln!("  {} {}", "›".blue(), colorize_backticks(line));
        }
    }
}
