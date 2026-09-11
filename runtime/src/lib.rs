//! The Lipi runtime: a tree-walking interpreter plus the standard library.
//!
//! This is the first execution engine for Lipi. The AST it runs is the same
//! one a future JavaScript / WASM / native backend will compile.

pub mod builtins;
mod http;
pub mod interp;
mod json;
mod methods;
pub mod task;
pub mod value;

pub use interp::{Interpreter, RunError, TestResult};
pub use value::{Env, Value};
