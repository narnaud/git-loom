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
use crate::core::ui::{self, Answer, Level, PromptKind};

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
    /// agent mode only the final line from `stop`/`error` is printed; in TUI
    /// mode the status bar animates instead.
    pub fn start(&self, msg: &str) {
        if ui::active() {
            ui::spinner(Some(msg));
            return;
        }
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

    fn finish(&self, symbol: ColoredString, msg: &str, level: Level) {
        if ui::active() {
            ui::spinner(None);
            ui::message(level, msg);
            return;
        }
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
        self.finish("✓".green(), msg, Level::Success);
    }

    /// Stop the spinner with an error message.
    pub fn error(&self, msg: &str) {
        self.finish("✗".red(), msg, Level::Error);
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
/// In agent mode everything goes to stderr, the human stream: stdout carries
/// the JSON status and nothing else (see spec 019). In TUI mode the line goes
/// to the TUI log instead.
fn print_message(symbol: ColoredString, message: &str, to_stderr: bool, level: Level) {
    if ui::active() {
        ui::message(level, message);
        return;
    }
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

/// Whether the stream human output lands on is a terminal: stderr in agent
/// mode, where stdout is the machine stream (spec 019), stdout otherwise.
/// Color and terminal width follow the text, so they must probe the stream it
/// is written to rather than stdout always.
pub fn human_stream_is_terminal() -> bool {
    if agent_mode::enabled() {
        io::stderr().is_terminal()
    } else {
        io::stdout().is_terminal()
    }
}

/// Width of the stream [`human_stream_is_terminal`] names, so a caller never
/// has to know which one that is; `None` when it is not a terminal.
pub fn human_stream_width() -> Option<u16> {
    let (terminal_size::Width(w), _) = if agent_mode::enabled() {
        terminal_size::terminal_size_of(io::stderr())?
    } else {
        terminal_size::terminal_size()?
    };
    Some(w)
}

/// Print text meant only for a person, which the JSON already carries or an
/// agent has no use for: the status tree, `update`'s fetch summaries. Stderr in
/// agent mode (spec 019); never reached from the TUI.
pub fn human(text: impl std::fmt::Display) {
    if agent_mode::enabled() {
        eprint!("{}", text);
    } else {
        print!("{}", text);
    }
}

/// Like [`human`], for a single line.
pub fn human_line(text: impl std::fmt::Display) {
    if agent_mode::enabled() {
        eprintln!("{}", text);
    } else {
        println!("{}", text);
    }
}

/// Print a success message with a green checkmark.
/// Additional lines are treated as hints and prefixed with a blue arrow.
/// Text between backticks is highlighted in yellow.
pub fn success(message: &str) {
    agent_mode::record_message(message);
    print_message("✓".green(), message, false, Level::Success);
}

/// Print a warning message with a yellow exclamation mark.
/// Additional lines are treated as hints and prefixed with a blue arrow.
/// Text between backticks is highlighted in yellow.
pub fn warn(message: &str) {
    agent_mode::record_message(message);
    print_message("!".yellow(), message, false, Level::Warn);
}

/// Print a warning that is *not* collected into the agent `messages` list.
///
/// For warnings agent mode already reports structurally — the conflict pause
/// becomes a `paused` status — so the JSON does not repeat itself.
pub fn warn_reported(message: &str) {
    print_message("!".yellow(), message, false, Level::Warn);
}

/// Print an error message with a red cross to stderr.
/// Additional lines are treated as hints and prefixed with a blue arrow.
/// Text between backticks is highlighted in yellow.
pub fn error(message: &str) {
    print_message("✗".red(), message, true, Level::Error);
}

// --- Interactive prompts ---
//
// Every prompt takes an `agent_hint`: the command to re-run with the answer
// supplied. In agent mode the prompt is not rendered — the choices and the
// hint are returned as a structured `needs_input`/`needs_confirmation`
// response instead (see spec 019). In TUI mode the prompt is a popup asked
// through `core::ui`, and validation re-asks with the failure shown.

/// The error for a cancelled operation: declining a `confirm`, choosing a
/// "Cancel" entry, or quitting a picker. Same marker a dismissed prompt
/// raises, so TUI mode reports all of them as a cancel, not a failure.
pub fn cancelled() -> anyhow::Error {
    ui::Cancelled.into()
}

/// TUI-mode text prompt: re-ask until the validator accepts the answer.
fn tui_text<F>(kind: PromptKind, prompt: &str, validator: F) -> Result<String>
where
    F: Fn(&str) -> std::result::Result<(), &'static str>,
{
    let mut error = None;
    loop {
        let Answer::Text(text) = ui::prompt(kind.clone(), prompt, error.take())? else {
            return Err(ui::Cancelled.into());
        };
        match validator(&text) {
            Ok(()) => return Ok(text),
            Err(e) => error = Some(e.to_string()),
        }
    }
}

/// Prompt the user for a yes/no confirmation. Returns `true` if confirmed.
///
/// Lines after the first are detail (what exactly the yes does): `›` lines
/// above the question on the CLI, the help box of the TUI menu.
pub fn confirm(prompt: &str, agent_hint: &str) -> Result<bool> {
    if agent_mode::enabled() {
        return Err(agent_mode::respond_needs_confirmation(prompt, agent_hint));
    }
    if ui::active() {
        return match ui::prompt(PromptKind::Confirm, prompt, None)? {
            Answer::Bool(yes) => Ok(yes),
            _ => Err(ui::Cancelled.into()),
        };
    }
    let (question, detail) = prompt.split_once('\n').unwrap_or((prompt, ""));
    for line in detail.lines() {
        println!("  {} {}", "›".blue(), colorize_backticks(line));
    }
    let answer = inquire::Confirm::new(question)
        .with_default(false)
        .prompt()?;
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
    if ui::active() {
        return tui_text(PromptKind::Input { placeholder: None }, prompt, validator);
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
    if ui::active() {
        let kind = PromptKind::Input {
            placeholder: Some(placeholder.to_string()),
        };
        return tui_text(kind, prompt, validator);
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
    if ui::active() {
        let kind = PromptKind::Select {
            items,
            allow_other: false,
        };
        return tui_text(kind, prompt, |_| Ok(()));
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
    if ui::active() {
        let kind = PromptKind::Select {
            items: suggestions,
            allow_other: true,
        };
        return tui_text(kind, prompt, validator);
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
    if ui::active() {
        return match ui::prompt(PromptKind::MultiSelect { items }, prompt, None)? {
            Answer::Many(selected) => Ok(selected),
            _ => Err(ui::Cancelled.into()),
        };
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
