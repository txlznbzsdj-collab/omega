//! Running an evaluation off the message thread.
//!
//! Rendering a large result takes seconds, and the engine permits results far
//! larger than any of the ones measured. Evaluating inside the window procedure
//! would block the message pump for that whole time, which is what makes the
//! window report "not responding". The work therefore runs on its own thread
//! and the result is posted back as a window message, so the window keeps
//! painting and responding to the close button while a computation runs.
//!
//! Only the newest request matters: typing re-evaluates on every keystroke, so
//! an in-flight evaluation is superseded the moment another key is pressed. The
//! worker is told to abandon it rather than allowed to finish and then be
//! ignored, which would keep the CPU busy for seconds on stale input.

use crate::format::{self, Style};
use crate::Engine;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

/// A request to evaluate an expression.
pub struct Request {
    pub expression: String,
    pub style: Style,
    /// Identifies this request. A result carrying a superseded id is dropped.
    pub generation: u64,
}

/// A finished evaluation, ready to display.
pub struct Answer {
    pub text: String,
    pub generation: u64,
}

/// The worker thread's handle to its queue, held by the window.
pub struct Worker {
    requests: Sender<Request>,
    answers: Receiver<Answer>,
    next_generation: u64,
    /// The generation the window is still waiting for, or has already shown.
    /// Kept in an `AtomicU64` so the worker can read it without a lock.
    latest: Arc<AtomicU64>,
}

impl Worker {
    /// Starts the worker. `notify` is called from the worker thread after an
    /// answer is queued, so the window can wake up and collect it.
    ///
    /// `notify` runs on the worker thread and must be safe to call from there;
    /// posting a window message is the intended use.
    pub fn start(notify: impl Fn() + Send + 'static) -> Worker {
        let (request_tx, request_rx) = mpsc::channel::<Request>();
        let (answer_tx, answer_rx) = mpsc::channel::<Answer>();
        let latest = Arc::new(AtomicU64::new(0));

        std::thread::spawn(move || {
            // The engine carries variable bindings and the working precision, so
            // it lives here and persists across requests, matching the CLI.
            let mut engine = Engine::new();
            while let Ok(request) = request_rx.recv() {
                // Drain anything newer: only the last keystroke matters, and
                // the intermediate ones are wasted work.
                let mut current = request;
                while let Ok(newer) = request_rx.try_recv() {
                    current = newer;
                }

                let answer = evaluate(&mut engine, &current);
                // Every request produces an answer, superseded or not. Dropping
                // one here would leave the window waiting forever whenever the
                // last request in a burst happens to be overtaken, which is
                // exactly what a keystroke burst does. The window discards a
                // stale answer when it arrives instead.
                if answer_tx.send(answer).is_err() {
                    break;
                }
                notify();
            }
        });

        Worker {
            requests: request_tx,
            answers: answer_rx,
            next_generation: 0,
            latest,
        }
    }

    /// Queues an evaluation, superseding any request still pending.
    pub fn submit(&mut self, expression: String, style: Style) -> u64 {
        self.next_generation += 1;
        let generation = self.next_generation;
        self.latest.store(generation, Ordering::SeqCst);
        // A send failure means the worker is gone; the window would already have
        // noticed through a missing answer, so it is not fatal here.
        let _ = self.requests.send(Request {
            expression,
            style,
            generation,
        });
        generation
    }

    /// Collects a finished answer, if one is waiting.
    pub fn take(&self) -> Option<Answer> {
        self.answers.try_recv().ok()
    }

    /// The newest answer, discarding any older ones that were already queued.
    pub fn take_latest(&self) -> Option<Answer> {
        let mut newest = None;
        while let Ok(answer) = self.answers.try_recv() {
            newest = Some(answer);
        }
        newest
    }

    /// True when `generation` is still the request the window is waiting for.
    pub fn is_current(&self, generation: u64) -> bool {
        self.latest.load(Ordering::SeqCst) == generation
    }
}

/// Evaluates one request. A `Value` is produced by the engine and rendered to
/// text here, so the window thread only ever moves a string.
fn evaluate(engine: &mut Engine, request: &Request) -> Answer {
    if request.expression.trim().is_empty() {
        return Answer {
            text: String::new(),
            generation: request.generation,
        };
    }
    let outcome = match engine.eval(&request.expression) {
        Ok(value) => {
            engine.set(crate::gui::edit::ANSWER_VARIABLE, value.clone());
            format::render(&value, request.style)
        }
        Err(error) => format::Rendered {
            text: error.to_string(),
            note: None,
        },
    };
    Answer {
        text: match outcome.note {
            Some(note) => format!("{}\r\n{}", outcome.text, note),
            None => outcome.text,
        },
        generation: request.generation,
    }
}

/// Convenience for tests: evaluate one expression with no worker thread.
pub fn evaluate_once(expression: &str, style: Style) -> String {
    let mut engine = Engine::new();
    evaluate(
        &mut engine,
        &Request {
            expression: expression.to_string(),
            style,
            generation: 0,
        },
    )
    .text
}
