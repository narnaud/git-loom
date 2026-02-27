use std::io::{self, Write};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use colored::{ColoredString, Colorize};
use inquire::validator::Validation;

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
        eprintln!("{} {}", "✗".red(), colorize_backticks(first));
        for line in lines {
            eprintln!("  {} {}", "›".blue(), colorize_backticks(line));
        }
    }
}

// --- Interactive prompts ---

/// Prompt the user for text input with a validation function.
///
/// The validator receives the input string and returns `Ok(())` if valid,
/// or `Err("message")` to show an error and re-prompt.
pub fn input<F>(prompt: &str, validator: F) -> Result<String>
where
    F: Fn(&str) -> std::result::Result<(), &'static str> + Clone + 'static,
{
    let answer = inquire::Text::new(prompt)
        .with_validator(move |input: &str| match validator(input) {
            Ok(()) => Ok(Validation::Valid),
            Err(msg) => Ok(Validation::Invalid(msg.into())),
        })
        .prompt()?;
    Ok(answer)
}

/// Prompt the user for text input with a default value and validation.
///
/// The default value is pre-filled in the input; pressing Enter accepts it.
pub fn input_with_placeholder<F>(prompt: &str, placeholder: &str, validator: F) -> Result<String>
where
    F: Fn(&str) -> std::result::Result<(), &'static str> + Clone + 'static,
{
    let answer = inquire::Text::new(prompt)
        .with_default(placeholder)
        .with_validator(move |input: &str| match validator(input) {
            Ok(()) => Ok(Validation::Valid),
            Err(msg) => Ok(Validation::Invalid(msg.into())),
        })
        .prompt()?;
    Ok(answer)
}

/// Prompt the user to select one item from a list.
pub fn select(prompt: &str, items: Vec<String>) -> Result<String> {
    let answer = inquire::Select::new(prompt, items).prompt()?;
    Ok(answer)
}

/// Prompt the user to select from suggestions or type a new value.
///
/// Shows a text input with autocomplete suggestions. The user can pick
/// a suggestion or type a new value. The validator is applied to the
/// final input.
pub fn select_or_input<F>(prompt: &str, suggestions: Vec<String>, validator: F) -> Result<String>
where
    F: Fn(&str) -> std::result::Result<(), &'static str> + Clone + 'static,
{
    let answer = inquire::Text::new(prompt)
        .with_autocomplete(SuggestionsHelper(suggestions))
        .with_validator(move |input: &str| match validator(input) {
            Ok(()) => Ok(Validation::Valid),
            Err(msg) => Ok(Validation::Invalid(msg.into())),
        })
        .prompt()?;
    Ok(answer)
}

#[derive(Clone)]
struct SuggestionsHelper(Vec<String>);

impl inquire::autocompletion::Autocomplete for SuggestionsHelper {
    fn get_suggestions(
        &mut self,
        input: &str,
    ) -> std::result::Result<Vec<String>, inquire::CustomUserError> {
        let matches = self
            .0
            .iter()
            .filter(|s| s.contains(input))
            .cloned()
            .collect();
        Ok(matches)
    }

    fn get_completion(
        &mut self,
        _input: &str,
        highlighted_suggestion: Option<String>,
    ) -> std::result::Result<inquire::autocompletion::Replacement, inquire::CustomUserError> {
        Ok(highlighted_suggestion)
    }
}
