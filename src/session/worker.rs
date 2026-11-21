//! The worker threads of a session: the action in flight, what it sends back
//! (`core::ui`), and the log its lines are filed in.

use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::Result;

use crate::core::graph;
use crate::core::trace_log;
use crate::core::ui::{self, Answer, Cancelled, Level, PromptKind, Request};
use crate::session::action::{self, Action, LogEntry, Summary, execute_action};

/// Work on a thread of its own, its `msg` output and prompts sent to the
/// session.
pub(crate) struct Job<T> {
    handle: JoinHandle<Result<T>>,
}

impl<T> Job<T> {
    pub(crate) fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }

    /// Its result, waited for; `what` names the work in the error of a thread
    /// that panicked.
    pub(crate) fn join(self, what: &str) -> Result<T> {
        self.handle.join().unwrap_or_else(|_| {
            Err(anyhow::anyhow!(
                "{what} crashed — run `loom trace` for what it did"
            ))
        })
    }
}

/// The action running on its thread.
pub(crate) struct Running {
    job: Job<()>,
    pub(crate) command: String,
    /// Text of the command's active spinner, if any.
    pub(crate) spinner: Option<String>,
}

/// A prompt from the running command; replying `None` cancels it.
pub(crate) struct Ask {
    /// The command line asking, empty when no action runs.
    pub(crate) command: String,
    pub(crate) kind: PromptKind,
    pub(crate) prompt: String,
    /// Why the previous answer was rejected.
    pub(crate) error: Option<String>,
    pub(crate) reply: Sender<Option<Answer>>,
}

/// What a request leaves the front-end to do.
pub(crate) enum Event {
    /// Nothing: a message, a spinner or a hand-back, taken in. What is shown
    /// may have changed.
    Filed,
    Ask(Ask),
    /// The command wants the terminal for a subprocess; ack once it is free.
    Suspend(Sender<()>),
}

/// How an action ended, its closing line in the log.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Ended {
    Done,
    /// A prompt was dismissed.
    Cancelled,
    Failed(String),
}

/// The work a session has in flight. One thing at a time: its front-end
/// starts no action and no read while another runs, or two loom commands
/// would work on one repository and report into one log entry.
pub(crate) struct Worker {
    requests: Receiver<Request>,
    /// What each job's thread gets a clone of.
    request_tx: Sender<Request>,
    running: Option<Running>,
    /// One entry per command started, the latest last.
    pub(crate) log: Vec<LogEntry>,
}

impl Worker {
    pub(crate) fn new() -> Self {
        let (request_tx, requests) = channel();
        Worker {
            requests,
            request_tx,
            running: None,
            log: Vec::new(),
        }
    }

    fn spawn<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> Result<T> + Send + 'static,
    ) -> Job<T> {
        let tx = self.request_tx.clone();
        let handle = std::thread::spawn(move || {
            // Without the sink `msg` prints, onto whatever the front-end is
            // drawing at the same moment.
            ui::install(tx);
            let result = work();
            ui::uninstall();
            result
        });
        Job { handle }
    }

    fn open(&mut self, command: &str) {
        self.log.push(LogEntry {
            command: command.to_string(),
            lines: Vec::new(),
        });
    }

    /// Run `action` as `command`, the line the log and the trace name it by.
    pub(crate) fn start(
        &mut self,
        action: Action,
        command: String,
        git_dir: &Path,
        theme: &graph::Theme,
    ) {
        let git_dir = git_dir.to_path_buf();
        let theme = theme.clone();
        let name = command.clone();
        self.run(command, move || {
            execute_action(action, &name, &git_dir, &theme)
        });
    }

    /// Run `work` as the action `command`: `start` for work that is no
    /// `Action`, which tests use to run one touching no repository.
    pub(crate) fn run(
        &mut self,
        command: String,
        work: impl FnOnce() -> Result<()> + Send + 'static,
    ) {
        debug_assert!(self.running.is_none(), "one action at a time");
        self.open(&command);
        let job = self.spawn(work);
        self.running = Some(Running {
            job,
            command,
            spinner: None,
        });
    }

    /// Start `read` on a thread, as what `command` does before it can run.
    /// The entry is opened here, before the command itself starts: the read
    /// is part of it, and its messages have to land there.
    pub(crate) fn read<T: Send + 'static>(
        &mut self,
        command: String,
        git_dir: &Path,
        read: impl FnOnce() -> Result<T> + Send + 'static,
    ) -> Job<T> {
        self.open(&command);
        let git_dir = git_dir.to_path_buf();
        self.spawn(move || {
            trace_log::init(&git_dir, &action::trace_name(&command), false);
            let result = read();
            trace_log::finalize();
            result
        })
    }

    /// Run `work` on the calling thread, its messages filed as a job's are.
    /// Nothing under it may prompt: the thread that would answer is this one.
    pub(crate) fn inline<T>(&self, work: impl FnOnce() -> T) -> T {
        ui::install(self.request_tx.clone());
        let result = work();
        ui::uninstall();
        result
    }

    pub(crate) fn running(&self) -> Option<&Running> {
        self.running.as_ref()
    }

    /// The next request waiting, if any, taken in as far as the worker can.
    pub(crate) fn poll(&mut self) -> Option<Event> {
        let request = self.requests.try_recv().ok()?;
        Some(self.file(request))
    }

    fn file(&mut self, request: Request) -> Event {
        match request {
            Request::Prompt {
                kind,
                prompt,
                error,
                reply,
            } => Event::Ask(Ask {
                command: self
                    .running
                    .as_ref()
                    .map_or_else(String::new, |r| r.command.clone()),
                kind,
                prompt,
                error,
                reply,
            }),
            Request::Message { level, text } => {
                if let Some(entry) = self.log.last_mut() {
                    entry.lines.push((level, text));
                }
                Event::Filed
            }
            Request::Spinner(text) => {
                if let Some(running) = &mut self.running {
                    running.spinner = text;
                }
                Event::Filed
            }
            Request::Suspend(ack) => Event::Suspend(ack),
            Request::Resume => Event::Filed,
        }
    }

    /// Block while the command holds the terminal: its `Resume` ends the
    /// wait, and so does a thread that ends without one. A prompt asked
    /// meanwhile goes to `ask`.
    pub(crate) fn wait_for_resume(&mut self, mut ask: impl FnMut(Ask)) {
        loop {
            match self.requests.recv_timeout(Duration::from_millis(100)) {
                Ok(Request::Resume) | Err(RecvTimeoutError::Disconnected) => return,
                Ok(request) => {
                    if let Event::Ask(asked) = self.file(request) {
                        ask(asked);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    if self.running.as_ref().is_none_or(|r| r.job.is_finished()) {
                        return;
                    }
                }
            }
        }
    }

    /// How the action ended, once its thread is done.
    pub(crate) fn reap(&mut self) -> Option<Ended> {
        if !self.running.as_ref()?.job.is_finished() {
            return None;
        }
        let running = self.running.take()?;
        Some(self.close(running.job.join("the action")))
    }

    /// File how an action that returned `result` ended: the second half of
    /// `reap`, which tests drive an ending through.
    pub(crate) fn close(&mut self, result: Result<()>) -> Ended {
        match result {
            Ok(()) => Ended::Done,
            Err(e) if e.downcast_ref::<Cancelled>().is_some() => {
                self.log_line(Level::Warn, "Cancelled");
                Ended::Cancelled
            }
            Err(e) => {
                let text = e.to_string();
                self.log_line(Level::Error, &text);
                Ended::Failed(text)
            }
        }
    }

    /// The one line the latest command is summed up in.
    pub(crate) fn summary(&self) -> Summary<'_> {
        self.log.last().map_or(Summary::Done, LogEntry::summary)
    }

    /// Add a line to the latest command's entry.
    pub(crate) fn log_line(&mut self, level: Level, text: &str) {
        if let Some(entry) = self.log.last_mut() {
            entry.lines.push((level, text.to_string()));
        }
    }

    /// What a job's thread sends its requests through.
    #[cfg(test)]
    pub(crate) fn sender(&self) -> Sender<Request> {
        self.request_tx.clone()
    }
}

#[cfg(test)]
#[path = "worker_test.rs"]
mod tests;
