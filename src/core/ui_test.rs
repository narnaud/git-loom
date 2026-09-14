use std::sync::mpsc::{Receiver, channel};
use std::thread;

use super::*;
use crate::core::msg;

/// Run `f` on a thread with the sink installed; the test thread plays the TUI.
fn with_sink<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> (Receiver<Request>, thread::JoinHandle<T>) {
    let (tx, rx) = channel();
    let handle = thread::spawn(move || {
        install(tx);
        let out = f();
        uninstall();
        out
    });
    (rx, handle)
}

fn next_prompt(
    rx: &Receiver<Request>,
) -> (PromptKind, String, Option<String>, Sender<Option<Answer>>) {
    loop {
        match rx.recv().expect("worker hung up") {
            Request::Prompt {
                kind,
                prompt,
                error,
                reply,
            } => return (kind, prompt, error, reply),
            _ => continue,
        }
    }
}

#[test]
fn sink_is_thread_local() {
    let (tx, _rx) = channel();
    install(tx);
    assert!(active());
    assert!(!thread::spawn(active).join().unwrap());
    uninstall();
    assert!(!active());
}

#[test]
fn messages_and_spinner_go_to_the_sink() {
    let (rx, handle) = with_sink(|| {
        msg::success("done `x`");
        msg::warn("careful");
        let spinner = msg::spinner();
        spinner.start("working");
        spinner.stop("worked");
    });
    handle.join().unwrap();
    let got: Vec<String> = rx
        .try_iter()
        .map(|r| match r {
            Request::Message { level, text } => format!("{level:?}:{text}"),
            Request::Spinner(Some(t)) => format!("spin:{t}"),
            Request::Spinner(None) => "spin:stop".to_string(),
            _ => "other".to_string(),
        })
        .collect();
    assert_eq!(
        got,
        [
            "Success:done `x`",
            "Warn:careful",
            "spin:working",
            "spin:stop",
            "Success:worked"
        ]
    );
}

#[test]
fn input_reasks_until_the_validator_accepts() {
    let (rx, handle) = with_sink(|| {
        msg::input("Branch name", "", |s| {
            if s.is_empty() { Err("empty") } else { Ok(()) }
        })
    });
    let (kind, prompt, error, reply) = next_prompt(&rx);
    assert_eq!(kind, PromptKind::Input { placeholder: None });
    assert_eq!(prompt, "Branch name");
    assert_eq!(error, None);
    reply.send(Some(Answer::Text(String::new()))).unwrap();

    let (_, _, error, reply) = next_prompt(&rx);
    assert_eq!(error.as_deref(), Some("empty"));
    reply.send(Some(Answer::Text("feat".into()))).unwrap();

    assert_eq!(handle.join().unwrap().unwrap(), "feat");
}

#[test]
fn dismissed_prompt_is_cancelled() {
    let (rx, handle) = with_sink(|| msg::confirm("Sure?", ""));
    let (kind, _, _, reply) = next_prompt(&rx);
    assert_eq!(kind, PromptKind::Confirm);
    reply.send(None).unwrap();
    let err = handle.join().unwrap().unwrap_err();
    assert!(err.downcast_ref::<Cancelled>().is_some());
}

#[test]
fn select_or_input_carries_suggestions_and_multi_select_returns_many() {
    let (rx, handle) = with_sink(|| {
        let a = msg::select_or_input("Target", vec!["x".into(), "y".into()], "", |_| Ok(()))?;
        let b = msg::multi_select("Files", vec!["f1".into(), "f2".into()], "")?;
        let c = msg::input_with_placeholder("Rename", "old", "", |_| Ok(()))?;
        Ok::<_, anyhow::Error>((a, b, c))
    });
    let (kind, _, _, reply) = next_prompt(&rx);
    assert_eq!(
        kind,
        PromptKind::Select {
            items: vec!["x".into(), "y".into()],
            allow_other: true
        }
    );
    reply.send(Some(Answer::Text("z".into()))).unwrap();

    let (kind, _, _, reply) = next_prompt(&rx);
    assert!(matches!(kind, PromptKind::MultiSelect { .. }));
    reply.send(Some(Answer::Many(vec!["f2".into()]))).unwrap();

    let (kind, _, _, reply) = next_prompt(&rx);
    assert_eq!(
        kind,
        PromptKind::Input {
            placeholder: Some("old".into())
        }
    );
    reply.send(Some(Answer::Text("new".into()))).unwrap();

    let (a, b, c) = handle.join().unwrap().unwrap();
    assert_eq!(a, "z");
    assert_eq!(b, vec!["f2".to_string()]);
    assert_eq!(c, "new");
}

#[test]
fn suspend_waits_for_the_ack_and_resumes_on_drop() {
    let (rx, handle) = with_sink(|| {
        let guard = suspend().unwrap().expect("sink active");
        drop(guard);
    });
    match rx.recv().unwrap() {
        Request::Suspend(ack) => ack.send(()).unwrap(),
        _ => panic!("expected a suspend request"),
    }
    assert!(matches!(rx.recv().unwrap(), Request::Resume));
    handle.join().unwrap();
}

#[test]
fn suspend_is_a_no_op_without_a_sink() {
    assert!(suspend().unwrap().is_none());
}

/// An unacknowledged handoff must not let the subprocess start: it would draw
/// and read keys on a screen the TUI still owns.
#[test]
fn a_dropped_suspend_ack_is_an_error() {
    let (rx, handle) = with_sink(|| suspend().map(|s| s.is_some()));
    match rx.recv().unwrap() {
        Request::Suspend(ack) => drop(ack),
        _ => panic!("expected a suspend request"),
    }
    assert!(handle.join().unwrap().is_err());
}
