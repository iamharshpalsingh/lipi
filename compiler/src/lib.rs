//! The LiPi compiler front end.
//!
//! Source text flows through: lexer → parser → AST → name resolution and
//! type checking (`checker`). The runtime crate executes the checked AST.

pub mod ast;
pub mod checker;
pub mod codegen;
pub mod diagnostics;
pub mod format;
pub mod lexer;
pub mod lint;
pub mod parser;
pub mod resolve;
pub mod scope;
pub mod suggest;

pub use diagnostics::{Diagnostic, Severity, Span};

/// Lex and parse a whole source file into a program.
pub fn parse_source(source: &str) -> Result<ast::Program, Diagnostic> {
    let result = lexer::Lexer::new(source).tokenize().and_then(|tokens| parser::Parser::new(source, tokens).parse_program());
    result.map_err(|d| d.code_or("LIP0001"))
}
