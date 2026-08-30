//! Agent mode: machine-readable responses for AI agents (see spec 019).
//!
//! When enabled (global `--agent` flag or the `LOOM_AGENT` environment
//! variable), every invocation ends with exactly one single-line JSON status
//! on stderr, and interactive prompts return structured answers instead of
//! rendering. Activation is explicit only — never inferred from a missing
//! terminal, which would change behavior for pipelines and tests.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;

static ENABLED: AtomicBool = AtomicBool::new(false);
/// Success/warning lines collected for the final `ok` response.
static MESSAGES: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Response stored by a prompt site or a conflict pause, emitted by `finish`.
static PENDING: Mutex<Option<AgentResponse>> = Mutex::new(None);

/// Enable or disable agent mode. Called once from `main()` before dispatch.
pub fn set(enabled: bool) {
    ENABLED.store(enabled, Ordering::SeqCst);
}

/// Whether agent mode is active for this process.
pub fn enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}

/// Marker error carried when a prompt was answered structurally instead of
/// interactively. `main()` maps it to exit code 10 and suppresses the usual
/// human error line (the stored response is the message).
#[derive(Debug)]
pub struct NeedsInput;

impl std::fmt::Display for NeedsInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "input required — see the JSON status line")
    }
}

impl std::error::Error for NeedsInput {}

/// The kind of input a prompt would have collected.
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InputKind {
    Select,
    Text,
    Multiselect,
}

/// The JSON status emitted as the last line of stderr in agent mode.
#[derive(Serialize, Debug)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AgentResponse {
    Ok {
        #[serde(skip_serializing_if = "Vec::is_empty")]
        messages: Vec<String>,
    },
    NeedsInput {
        kind: InputKind,
        prompt: String,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        options: Vec<String>,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        allow_other: bool,
        hint: String,
    },
    NeedsConfirmation {
        prompt: String,
        hint: String,
    },
    Paused {
        message: String,
        hint: String,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        messages: Vec<String>,
    },
    Error {
        message: String,
    },
}

impl AgentResponse {
    /// Serialize to the single-line JSON form.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("AgentResponse serialization cannot fail")
    }

    /// The exit code this response maps to.
    pub fn exit_code(&self) -> i32 {
        match self {
            AgentResponse::Ok { .. } | AgentResponse::Paused { .. } => 0,
            AgentResponse::Error { .. } => 1,
            AgentResponse::NeedsInput { .. } | AgentResponse::NeedsConfirmation { .. } => 10,
        }
    }
}

/// Collect a success/warning line for the final `ok` response.
///
/// No-op when agent mode is off.
pub fn record_message(message: &str) {
    if enabled() {
        MESSAGES.lock().unwrap().push(message.to_string());
    }
}

/// Store a `needs_input` response and return the marker error to propagate.
pub fn respond_needs_input(
    kind: InputKind,
    prompt: &str,
    options: Vec<String>,
    allow_other: bool,
    hint: &str,
) -> anyhow::Error {
    *PENDING.lock().unwrap() = Some(AgentResponse::NeedsInput {
        kind,
        prompt: prompt.to_string(),
        options,
        allow_other,
        hint: hint.to_string(),
    });
    anyhow::Error::new(NeedsInput)
}

/// Store a `needs_confirmation` response and return the marker error to propagate.
pub fn respond_needs_confirmation(prompt: &str, hint: &str) -> anyhow::Error {
    *PENDING.lock().unwrap() = Some(AgentResponse::NeedsConfirmation {
        prompt: prompt.to_string(),
        hint: hint.to_string(),
    });
    anyhow::Error::new(NeedsInput)
}

/// Note that the operation paused on conflicts; the final status becomes
/// `paused` instead of `ok`. No-op when agent mode is off.
pub fn note_paused(message: &str, hint: &str) {
    if enabled() {
        *PENDING.lock().unwrap() = Some(AgentResponse::Paused {
            message: message.to_string(),
            hint: hint.to_string(),
            // The lines recorded so far are attached by `finish`.
            messages: Vec::new(),
        });
    }
}

/// Emit the final JSON status line to stderr and return the exit code.
///
/// Called exactly once, at the very end of `main()`, when agent mode is on.
pub fn finish(result: &anyhow::Result<()>) -> i32 {
    let pending = PENDING.lock().unwrap().take();
    let collected = std::mem::take(&mut *MESSAGES.lock().unwrap());
    let response = match result {
        // Only a conflict pause overrides success; a leftover `needs_*`
        // response here means its marker error was swallowed and the command
        // completed anyway, so `ok` is the truth.
        Ok(()) => match pending {
            Some(AgentResponse::Paused { message, hint, .. }) => AgentResponse::Paused {
                message,
                hint,
                messages: collected,
            },
            _ => AgentResponse::Ok {
                messages: collected,
            },
        },
        Err(e) if e.downcast_ref::<NeedsInput>().is_some() => {
            pending.unwrap_or_else(|| AgentResponse::Error {
                message: e.to_string(),
            })
        }
        Err(e) => AgentResponse::Error {
            message: e.to_string(),
        },
    };
    eprintln!("{}", response.to_json());
    response.exit_code()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_with_messages() {
        let r = AgentResponse::Ok {
            messages: vec!["Created commit `1a2b3c4`".to_string()],
        };
        assert_eq!(
            r.to_json(),
            r#"{"status":"ok","messages":["Created commit `1a2b3c4`"]}"#
        );
        assert_eq!(r.exit_code(), 0);
    }

    #[test]
    fn ok_without_messages_omits_field() {
        let r = AgentResponse::Ok { messages: vec![] };
        assert_eq!(r.to_json(), r#"{"status":"ok"}"#);
    }

    #[test]
    fn needs_input_select() {
        let r = AgentResponse::NeedsInput {
            kind: InputKind::Select,
            prompt: "Select target branch".to_string(),
            options: vec!["feature-a".to_string(), "feature-b".to_string()],
            allow_other: true,
            hint: "re-run with: loom commit -b <branch>".to_string(),
        };
        assert_eq!(
            r.to_json(),
            r#"{"status":"needs_input","kind":"select","prompt":"Select target branch","options":["feature-a","feature-b"],"allow_other":true,"hint":"re-run with: loom commit -b <branch>"}"#
        );
        assert_eq!(r.exit_code(), 10);
    }

    #[test]
    fn needs_input_text_omits_options_and_allow_other() {
        let r = AgentResponse::NeedsInput {
            kind: InputKind::Text,
            prompt: "Commit message".to_string(),
            options: vec![],
            allow_other: false,
            hint: "pass -m <message>".to_string(),
        };
        assert_eq!(
            r.to_json(),
            r#"{"status":"needs_input","kind":"text","prompt":"Commit message","hint":"pass -m <message>"}"#
        );
    }

    #[test]
    fn needs_confirmation() {
        let r = AgentResponse::NeedsConfirmation {
            prompt: "Discard changes?".to_string(),
            hint: "re-run with: loom drop <target> -y".to_string(),
        };
        assert_eq!(
            r.to_json(),
            r#"{"status":"needs_confirmation","prompt":"Discard changes?","hint":"re-run with: loom drop <target> -y"}"#
        );
        assert_eq!(r.exit_code(), 10);
    }

    #[test]
    fn paused_without_messages_omits_field() {
        let r = AgentResponse::Paused {
            message: "Conflicts detected".to_string(),
            hint: "run loom continue".to_string(),
            messages: vec![],
        };
        assert_eq!(
            r.to_json(),
            r#"{"status":"paused","message":"Conflicts detected","hint":"run loom continue"}"#
        );
        assert_eq!(r.exit_code(), 0);
    }

    #[test]
    fn paused_with_messages() {
        let r = AgentResponse::Paused {
            message: "Conflicts detected".to_string(),
            hint: "run loom continue".to_string(),
            messages: vec!["Rebased onto `origin/main`".to_string()],
        };
        assert_eq!(
            r.to_json(),
            r#"{"status":"paused","message":"Conflicts detected","hint":"run loom continue","messages":["Rebased onto `origin/main`"]}"#
        );
    }

    #[test]
    fn error() {
        let r = AgentResponse::Error {
            message: "Branch 'x' not found".to_string(),
        };
        assert_eq!(
            r.to_json(),
            r#"{"status":"error","message":"Branch 'x' not found"}"#
        );
        assert_eq!(r.exit_code(), 1);
    }

    #[test]
    fn multiselect_kind_serializes_lowercase() {
        let r = AgentResponse::NeedsInput {
            kind: InputKind::Multiselect,
            prompt: "Select files".to_string(),
            options: vec!["a.rs".to_string()],
            allow_other: false,
            hint: "pass files".to_string(),
        };
        assert!(r.to_json().contains(r#""kind":"multiselect""#));
    }
}
