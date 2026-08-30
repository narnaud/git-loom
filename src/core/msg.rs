use std::io::{self, IsTerminal, Write};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use colored::{ColoredString, Colorize};
use inquire::validator::Validation;

use crate::core::agent_mode::{self, InputKind};

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
    ///
    /// The animation only runs when stdout is a terminal — in a pipeline or in
    /// agent mode only the final line from `stop`/`error` is printed.
    pub fn start(&self, msg: &str) {
        if !io::stdout().is_terminal() || agent_mode::enabled() {
            return;
        }
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
        let animated = self.thread.lock().unwrap().take();
        if let Some(handle) = animated {
            let _ = handle.join();
            // \r returns to line start; \x1b[K clears to end of line
            println!("\r{} {}\x1b[K", symbol, msg);
        } else if agent_mode::enabled() {
            eprintln!("{} {}", symbol, msg);
        } else {
            println!("{} {}", symbol, msg);
        }
    }

    /// Stop the spinner with a success message.
    pub fn stop(&self, msg: &str) {
        agent_mode::record_message(msg);
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

/// Print a symbol-prefixed message; hint lines get the blue arrow prefix.
///
/// In agent mode everything goes to stderr so stdout stays pure payload and
/// the final JSON status is the last line of stderr (see spec 019).
fn print_message(symbol: ColoredString, message: &str, to_stderr: bool) {
    let to_stderr = to_stderr || agent_mode::enabled();
    let mut lines = message.lines();
    if let Some(first) = lines.next() {
        let head = format!("{} {}", symbol, colorize_backticks(first));
        if to_stderr {
            eprintln!("{}", head);
        } else {
            println!("{}", head);
        }
        for line in lines {
            let cont = format!("  {} {}", "›".blue(), colorize_backticks(line));
            if to_stderr {
                eprintln!("{}", cont);
            } else {
                println!("{}", cont);
            }
        }
    }
}

/// Print a success message with a green checkmark.
/// Additional lines are treated as hints and prefixed with a blue arrow.
/// Text between backticks is highlighted in yellow.
pub fn success(message: &str) {
    agent_mode::record_message(message);
    print_message("✓".green(), message, false);
}

/// Print a warning message with a yellow exclamation mark.
/// Additional lines are treated as hints and prefixed with a blue arrow.
/// Text between backticks is highlighted in yellow.
pub fn warn(message: &str) {
    agent_mode::record_message(message);
    print_message("!".yellow(), message, false);
}

/// Print an error message with a red cross to stderr.
/// Additional lines are treated as hints and prefixed with a blue arrow.
/// Text between backticks is highlighted in yellow.
pub fn error(message: &str) {
    print_message("✗".red(), message, true);
}

// --- Interactive prompts ---
//
// Every prompt takes an `agent_hint`: the command to re-run with the answer
// supplied. In agent mode the prompt is not rendered — the choices and the
// hint are returned as a structured `needs_input`/`needs_confirmation`
// response instead (see spec 019).

/// Prompt the user for a yes/no confirmation. Returns `true` if confirmed.
pub fn confirm(prompt: &str, agent_hint: &str) -> Result<bool> {
    if agent_mode::enabled() {
        return Err(agent_mode::respond_needs_confirmation(prompt, agent_hint));
    }
    let answer = inquire::Confirm::new(prompt).with_default(false).prompt()?;
    Ok(answer)
}

/// Prompt the user for text input with a validation function.
///
/// The validator receives the input string and returns `Ok(())` if valid,
/// or `Err("message")` to show an error and re-prompt.
pub fn input<F>(prompt: &str, agent_hint: &str, validator: F) -> Result<String>
where
    F: Fn(&str) -> std::result::Result<(), &'static str> + Clone + 'static,
{
    if agent_mode::enabled() {
        return Err(agent_mode::respond_needs_input(
            InputKind::Text,
            prompt,
            vec![],
            false,
            agent_hint,
        ));
    }
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
pub fn input_with_placeholder<F>(
    prompt: &str,
    placeholder: &str,
    agent_hint: &str,
    validator: F,
) -> Result<String>
where
    F: Fn(&str) -> std::result::Result<(), &'static str> + Clone + 'static,
{
    if agent_mode::enabled() {
        return Err(agent_mode::respond_needs_input(
            InputKind::Text,
            prompt,
            vec![],
            false,
            agent_hint,
        ));
    }
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
pub fn select(prompt: &str, items: Vec<String>, agent_hint: &str) -> Result<String> {
    if agent_mode::enabled() {
        return Err(agent_mode::respond_needs_input(
            InputKind::Select,
            prompt,
            items,
            false,
            agent_hint,
        ));
    }
    let answer = inquire::Select::new(prompt, items).prompt()?;
    Ok(answer)
}

/// Prompt the user to select from suggestions or type a new value.
///
/// Shows a text input with autocomplete suggestions. The user can pick
/// a suggestion or type a new value. The validator is applied to the
/// final input.
pub fn select_or_input<F>(
    prompt: &str,
    suggestions: Vec<String>,
    agent_hint: &str,
    validator: F,
) -> Result<String>
where
    F: Fn(&str) -> std::result::Result<(), &'static str> + Clone + 'static,
{
    if agent_mode::enabled() {
        return Err(agent_mode::respond_needs_input(
            InputKind::Select,
            prompt,
            suggestions,
            true,
            agent_hint,
        ));
    }
    let answer = inquire::Text::new(prompt)
        .with_autocomplete(SuggestionsHelper(suggestions))
        .with_validator(move |input: &str| match validator(input) {
            Ok(()) => Ok(Validation::Valid),
            Err(msg) => Ok(Validation::Invalid(msg.into())),
        })
        .prompt()?;
    Ok(answer)
}

/// Prompt the user to select one or more items from a list.
///
/// At least one item must be selected.
pub fn multi_select(prompt: &str, items: Vec<String>, agent_hint: &str) -> Result<Vec<String>> {
    if agent_mode::enabled() {
        return Err(agent_mode::respond_needs_input(
            InputKind::Multiselect,
            prompt,
            items,
            false,
            agent_hint,
        ));
    }
    let selected = inquire::MultiSelect::new(prompt, items)
        .with_validator(|selection: &[inquire::list_option::ListOption<&String>]| {
            if selection.is_empty() {
                return Ok(Validation::Invalid("Must select at least one item".into()));
            }
            Ok(Validation::Valid)
        })
        .prompt()?;
    Ok(selected)
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
