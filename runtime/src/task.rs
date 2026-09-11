//! Tasks: the values behind `async` and `await`.
//!
//! Slow operations (HTTP requests, `sleep`) run on background threads and
//! hand back a task immediately, so several of them can be in flight at once:
//!
//! ```lipi
//! a = http.get(url1)          # both requests start now
//! b = http.get(url2)
//! results = await all([a, b]) # wait for both
//! ```
//!
//! Calling an `async` function runs it to completion and returns a finished
//! task; errors inside it surface where the task is awaited.

use crate::interp::{Flow, Interpreter, Thrown};
use crate::value::{Fields, Value};
use lipi_compiler::Span;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, TryRecvError};
use std::time::Duration;

/// Data that can cross threads (runtime values can't).
pub enum SendValue {
    Nil,
    Bool(bool),
    Num(f64),
    Str(String),
    List(Vec<SendValue>),
    Obj(Vec<(String, SendValue)>),
}

impl SendValue {
    pub fn into_value(self) -> Value {
        match self {
            SendValue::Nil => Value::Nil,
            SendValue::Bool(b) => Value::Bool(b),
            SendValue::Num(n) => Value::Num(n),
            SendValue::Str(s) => Value::string(s),
            SendValue::List(items) => Value::list(items.into_iter().map(SendValue::into_value).collect()),
            SendValue::Obj(fields) => Value::object(fields.into_iter().map(|(k, v)| (k, v.into_value())).collect::<Fields>()),
        }
    }
}

/// Turns the thread's result into a runtime value on the main thread.
pub type Mapper = fn(&mut Interpreter, SendValue) -> Value;

pub enum TaskState {
    Pending { rx: Receiver<Result<SendValue, String>>, map: Mapper },
    Done(Result<Value, Box<Thrown>>),
    Cancelled,
}

pub fn identity(_: &mut Interpreter, v: SendValue) -> Value {
    v.into_value()
}

/// Start `work` on a background thread and return a task for its result.
pub fn spawn(map: Mapper, work: impl FnOnce() -> Result<SendValue, String> + Send + 'static) -> Value {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    Value::Task(Rc::new(RefCell::new(TaskState::Pending { rx, map })))
}

/// A task that has already finished.
pub fn done(result: Result<Value, Box<Thrown>>) -> Value {
    Value::Task(Rc::new(RefCell::new(TaskState::Done(result))))
}

/// Wait for a task (optionally with a time limit) and return its value or rethrow its error.
pub fn await_task(it: &mut Interpreter, task: &Rc<RefCell<TaskState>>, span: Span, timeout: Option<Duration>) -> Result<Value, Flow> {
    let state = std::mem::replace(&mut *task.borrow_mut(), TaskState::Cancelled);
    let result = match state {
        TaskState::Done(r) => r,
        TaskState::Cancelled => {
            return Err(it.error(
                "this task was cancelled",
                span,
                Some("A cancelled task has no result, so it can't be awaited.".into()),
            ))
        }
        TaskState::Pending { rx, map } => {
            let received = match timeout {
                None => rx.recv().map_err(|_| "the task stopped unexpectedly".to_string()),
                Some(limit) => match rx.recv_timeout(limit) {
                    Ok(r) => Ok(r),
                    Err(RecvTimeoutError::Timeout) => {
                        return Err(it.error(
                            format!("timed out after {} ms", limit.as_millis()),
                            span,
                            Some("The task took too long and was cancelled. Allow more time if it needs longer.".into()),
                        ))
                    }
                    Err(RecvTimeoutError::Disconnected) => Err("the task stopped unexpectedly".to_string()),
                },
            };
            match received.and_then(|r| r) {
                Ok(v) => Ok(map(it, v)),
                Err(message) => Err(it.thrown(message, span, None)),
            }
        }
    };
    *task.borrow_mut() = TaskState::Done(result.clone());
    result.map_err(Flow::Throw)
}

/// Has the task finished? Never blocks.
pub fn poll(it: &mut Interpreter, task: &Rc<RefCell<TaskState>>, span: Span) -> bool {
    let state = std::mem::replace(&mut *task.borrow_mut(), TaskState::Cancelled);
    let (new_state, done) = match state {
        TaskState::Pending { rx, map } => match rx.try_recv() {
            Ok(Ok(v)) => (TaskState::Done(Ok(map(it, v))), true),
            Ok(Err(message)) => (TaskState::Done(Err(it.thrown(message, span, None))), true),
            Err(TryRecvError::Empty) => (TaskState::Pending { rx, map }, false),
            Err(TryRecvError::Disconnected) => (TaskState::Done(Err(it.thrown("the task stopped unexpectedly", span, None))), true),
        },
        other => (other, true),
    };
    *task.borrow_mut() = new_state;
    done
}
