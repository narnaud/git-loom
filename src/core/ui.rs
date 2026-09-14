//! TUI mode: the channel between a loom command running on a worker thread
//! and the `loom tui` event loop that owns the terminal (spec 020).
//!
//! The sink is thread-local: the worker installs it, so only the command it
//! runs is redirected — `msg` prompts and messages travel through the channel
//! instead of touching stdout, and `git::run_git_interactive` hands the
//! terminal back for the duration of an editor.

use std::cell::RefCell;
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

thread_local! {
    static SINK: RefCell<Option<Sender<Request>>> = const { RefCell::new(None) };
}

/// Severity of a message line, mirroring `msg::success/warn/error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Success,
    Warn,
    Error,
}

/// What a prompt asks for, mirroring the `msg` prompt functions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptKind {
    Confirm,
    Input {
        placeholder: Option<String>,
    },
    /// `allow_other` is `msg::select_or_input`: a typed value is accepted too.
    Select {
        items: Vec<String>,
        allow_other: bool,
    },
    MultiSelect {
        items: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Bool(bool),
    Text(String),
    Many(Vec<String>),
}

pub enum Request {
    /// A prompt to show; `error` is why the previous answer was rejected.
    /// Replying `None` cancels the prompt.
    Prompt {
        kind: PromptKind,
        prompt: String,
        error: Option<String>,
        reply: Sender<Option<Answer>>,
    },
    Message {
        level: Level,
        text: String,
    },
    /// `Some(text)` starts a spinner, `None` stops it.
    Spinner(Option<String>),
    /// Hand the terminal to a subprocess; the TUI acks once it is restored.
    Suspend(Sender<()>),
    Resume,
}

/// Marker error: the user dismissed a prompt.
#[derive(Debug)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Cancelled")
    }
}

impl std::error::Error for Cancelled {}

/// Redirect this thread's `msg` output and prompts to the TUI.
pub fn install(sender: Sender<Request>) {
    SINK.with(|s| *s.borrow_mut() = Some(sender));
}

pub fn uninstall() {
    SINK.with(|s| *s.borrow_mut() = None);
}

pub fn active() -> bool {
    SINK.with(|s| s.borrow().is_some())
}

/// `false` when no sink is installed or the TUI is gone.
fn send(request: Request) -> bool {
    SINK.with(|s| match s.borrow().as_ref() {
        Some(sender) => sender.send(request).is_ok(),
        None => false,
    })
}

pub fn message(level: Level, text: &str) {
    send(Request::Message {
        level,
        text: text.to_string(),
    });
}

pub fn spinner(text: Option<&str>) {
    send(Request::Spinner(text.map(str::to_string)));
}

/// Ask the TUI and block for the answer. A dismissed prompt, or a TUI that
/// went away, is [`Cancelled`].
pub fn prompt(kind: PromptKind, prompt: &str, error: Option<String>) -> anyhow::Result<Answer> {
    let (reply, answer) = channel();
    let sent = send(Request::Prompt {
        kind,
        prompt: prompt.to_string(),
        error,
        reply,
    });
    match answer.recv() {
        Ok(Some(answer)) if sent => Ok(answer),
        _ => Err(Cancelled.into()),
    }
}

/// The terminal is handed back to the TUI when this drops.
pub struct Suspended;

impl Drop for Suspended {
    fn drop(&mut self) {
        send(Request::Resume);
    }
}

/// Take the terminal from the TUI for a subprocess (an editor). Blocks until
/// the TUI has restored it; `Ok(None)` outside TUI mode, or when the TUI is
/// already gone and the terminal is ours again.
///
/// A handoff that is never acknowledged is an error: the subprocess would
/// draw and read keys on a screen the TUI still owns.
pub fn suspend() -> anyhow::Result<Option<Suspended>> {
    if !active() {
        return Ok(None);
    }
    let (ack, restored) = channel();
    if !send(Request::Suspend(ack)) {
        return Ok(None);
    }
    restored
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| anyhow::anyhow!("the TUI did not hand over the terminal"))?;
    Ok(Some(Suspended))
}

#[cfg(test)]
#[path = "ui_test.rs"]
mod tests;
