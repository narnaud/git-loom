use std::time::{Duration, Instant};

use super::*;

/// The next event, waited for: the job's thread sends at its own pace.
fn next_event(worker: &mut Worker) -> Event {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(event) = worker.poll() {
            return event;
        }
        assert!(Instant::now() < deadline, "the job never sent anything");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn reap(worker: &mut Worker) -> Ended {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(ended) = worker.reap() {
            return ended;
        }
        assert!(Instant::now() < deadline, "the action never ended");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn lines(worker: &Worker) -> Vec<(Level, &str)> {
    let entry = worker.log.last().expect("an entry");
    entry
        .lines
        .iter()
        .map(|(level, text)| (*level, text.as_str()))
        .collect()
}

#[test]
fn an_action_reports_into_the_entry_its_command_opened() {
    let mut worker = Worker::new("tui", ui::test_words);
    worker.run("loom drop a1".to_string(), || {
        ui::spinner(Some("Rebasing"));
        ui::message(Level::Success, "Dropped `a1`");
        match ui::prompt(PromptKind::Confirm, "Drop the branch too?", None)? {
            Answer::Bool(true) => Ok(()),
            _ => anyhow::bail!("not confirmed"),
        }
    });

    assert!(matches!(next_event(&mut worker), Event::Filed));
    assert_eq!(
        worker.running().unwrap().spinner.as_deref(),
        Some("Rebasing")
    );
    assert!(matches!(next_event(&mut worker), Event::Filed));
    let Event::Ask(ask) = next_event(&mut worker) else {
        panic!("expected the prompt");
    };
    assert_eq!(ask.command, "loom drop a1");
    assert_eq!(ask.prompt, "Drop the branch too?");
    assert_eq!(worker.reap(), None, "it waits for the answer");

    ask.reply.send(Some(Answer::Bool(true))).unwrap();
    assert_eq!(reap(&mut worker), Ended::Done);
    assert!(worker.running().is_none());
    assert_eq!(worker.log.len(), 1);
    assert_eq!(worker.log[0].command, "loom drop a1");
    assert_eq!(lines(&worker), [(Level::Success, "Dropped `a1`")]);
    assert_eq!(
        worker.summary(),
        Summary::Succeeded {
            last: "Dropped `a1`",
            more: 0
        }
    );
}

#[test]
fn a_dismissed_prompt_ends_the_action_as_cancelled() {
    let mut worker = Worker::new("tui", ui::test_words);
    worker.run("loom drop a1".to_string(), || {
        ui::prompt(PromptKind::Confirm, "Drop?", None).map(|_| ())
    });
    let Event::Ask(ask) = next_event(&mut worker) else {
        panic!("expected the prompt");
    };
    ask.reply.send(None).unwrap();

    assert_eq!(reap(&mut worker), Ended::Cancelled);
    assert_eq!(lines(&worker), [(Level::Warn, "Cancelled")]);
}

#[test]
fn a_failure_is_filed_and_handed_back() {
    let mut worker = Worker::new("tui", ui::test_words);
    worker.run("loom drop a1".to_string(), || {
        anyhow::bail!("Nothing to drop")
    });

    assert_eq!(
        reap(&mut worker),
        Ended::Failed("Nothing to drop".to_string())
    );
    assert_eq!(lines(&worker), [(Level::Error, "Nothing to drop")]);
}

#[test]
fn a_thread_that_panicked_reads_as_a_failure() {
    let mut worker = Worker::new("tui", ui::test_words);
    // Unwinds without the panic hook, which would print into the test output.
    worker.run("loom drop a1".to_string(), || {
        std::panic::resume_unwind(Box::new(()))
    });

    let text = "the action crashed — run `loom trace` for what it did";
    assert_eq!(reap(&mut worker), Ended::Failed(text.to_string()));
    assert_eq!(lines(&worker), [(Level::Error, text)]);
}

#[test]
fn a_read_opens_the_entry_of_the_command_it_prepares() {
    let git_dir = tempfile::tempdir().unwrap();
    let mut worker = Worker::new("tui", ui::test_words);
    let job = worker.read("loom add -p zz".to_string(), git_dir.path(), || {
        ui::message(Level::Warn, "Skipped a binary file");
        Ok(3)
    });

    assert!(worker.running().is_none(), "a read is no action");
    assert_eq!(job.join("reading the hunks").unwrap(), 3);
    assert!(matches!(worker.poll(), Some(Event::Filed)));
    assert_eq!(worker.log[0].command, "loom add -p zz");
    assert_eq!(lines(&worker), [(Level::Warn, "Skipped a binary file")]);
}

#[test]
fn work_runs_under_the_name_and_the_words_of_its_front_end() {
    fn words(hint: ui::Hint) -> &'static str {
        match hint {
            ui::Hint::ForcePush => "from its menu",
            ui::Hint::Reload => "Reload",
        }
    }
    let git_dir = tempfile::tempdir().unwrap();
    let mut worker = Worker::new("gui", words);

    let job = worker.read("loom add -p zz".to_string(), git_dir.path(), || {
        // A trace with no command in it is not written.
        trace_log::log_command("git", "status", 1, true, "");
        Ok(ui::hint(ui::Hint::ForcePush))
    });
    assert_eq!(job.join("reading").unwrap(), Some("from its menu"));
    assert_eq!(worker.inline(|| ui::hint(ui::Hint::Reload)), Some("Reload"));

    let trace = trace_log::latest_log_path(git_dir.path()).expect("a trace");
    let trace = std::fs::read_to_string(trace).unwrap();
    assert!(trace.contains("loom gui: loom add -p zz"), "{trace}");
}

#[test]
fn inline_work_reports_as_a_job_does() {
    let mut worker = Worker::new("tui", ui::test_words);
    worker.log.push(LogEntry {
        command: "loom add -p zz".to_string(),
        lines: Vec::new(),
    });

    let staged = worker.inline(|| {
        ui::message(Level::Success, "Staged 2 hunks");
        2
    });

    assert_eq!(staged, 2);
    assert!(!ui::active(), "the sink is gone with the work");
    assert!(matches!(worker.poll(), Some(Event::Filed)));
    assert_eq!(lines(&worker), [(Level::Success, "Staged 2 hunks")]);
}

#[test]
fn the_wait_for_the_terminal_hands_prompts_over() {
    let mut worker = Worker::new("tui", ui::test_words);
    let (reply, _answer) = channel();
    let sender = worker.sender();
    sender
        .send(Request::Prompt {
            kind: PromptKind::Confirm,
            prompt: "Continue?".to_string(),
            error: None,
            reply,
        })
        .unwrap();
    sender.send(Request::Resume).unwrap();

    let mut asked = Vec::new();
    worker.wait_for_resume(|ask| asked.push(ask.prompt));

    assert_eq!(asked, ["Continue?"]);
    assert!(worker.poll().is_none());
}
