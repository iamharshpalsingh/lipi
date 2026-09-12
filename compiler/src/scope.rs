//! Which variables a function body (or file) creates. Shared by the
//! interpreter's slot resolver and the JavaScript code generator, so both apply
//! LiPi's scope rule the same way: assignment updates the nearest existing
//! variable, otherwise it creates one in the current function. `if`, `while`
//! and `try` blocks don't have their own scope.
//!
//! `for` is the one exception: its variables belong to the loop, and each
//! round of the loop gets its own copy of them, so a function made inside the
//! loop remembers the item it was made with. They are therefore not collected
//! here — the resolver and the code generator each open a small scope for
//! them around the loop body.

use crate::ast::*;
use crate::checker::module_binding_name;

#[derive(Default)]
pub struct ScopeNames {
    /// Always local: variables of `catch` and `use`, functions, types,
    /// components, typed and constant variables, and `state`.
    pub defined: Vec<(String, Option<TypeExpr>)>,
    /// Plain assignments: local unless an enclosing scope has the name.
    pub assigned: Vec<String>,
    /// Functions, types and components at the top of the body (defined before it runs).
    pub hoisted: Vec<String>,
    /// `state` variables.
    pub states: Vec<String>,
}

/// The variables one `for` loop binds: the item (and the key, when the loop
/// has two), plus every name in a destructuring pattern. Each round of the
/// loop gets its own copy of these, in the order returned here.
pub fn loop_names(first: &Name, second: Option<&Name>, pattern: Option<&Pattern>) -> Vec<String> {
    let mut names = vec![first.text.clone()];
    if let Some(s) = second {
        names.push(s.text.clone());
    }
    if let Some(p) = pattern {
        for n in p.names() {
            if !names.contains(&n.text) {
                names.push(n.text.clone());
            }
        }
    }
    names
}

/// The names `body` creates, not looking inside nested functions.
pub fn collect(body: &[Stmt]) -> ScopeNames {
    let mut n = ScopeNames::default();
    collect_block(body, &mut n, true);
    n
}

pub fn collect_block(body: &[Stmt], n: &mut ScopeNames, top: bool) {
    for stmt in body {
        collect_stmt(stmt, n, top);
    }
}

fn collect_stmt(stmt: &Stmt, n: &mut ScopeNames, top: bool) {
    match &stmt.kind {
        StmtKind::Assign { target: Target::Name(name), ty, constant, .. } => {
            if *constant || ty.is_some() {
                n.defined.push((name.text.clone(), ty.clone()));
            } else {
                n.assigned.push(name.text.clone());
            }
        }
        StmtKind::Assign { target: Target::Pattern(p), .. } => n.assigned.extend(p.names().into_iter().map(|x| x.text.clone())),
        StmtKind::If { branches, otherwise } => {
            for (_, b) in branches {
                collect_block(b, n, false);
            }
            if let Some(b) = otherwise {
                collect_block(b, n, false);
            }
        }
        StmtKind::While { body, .. } | StmtKind::Repeat { body, .. } => collect_block(body, n, false),
        // The loop's own variables live in the loop, not out here (see the
        // module comment); only what the body assigns reaches this scope.
        StmtKind::For { body, .. } => collect_block(body, n, false),
        StmtKind::Func(f) | StmtKind::Component(f) => {
            n.defined.push((f.name.text.clone(), None));
            if top {
                n.hoisted.push(f.name.text.clone());
            }
        }
        StmtKind::TypeDef(t) => {
            n.defined.push((t.name.text.clone(), None));
            if top {
                n.hoisted.push(t.name.text.clone());
            }
        }
        StmtKind::State { name, ty, .. } => {
            n.defined.push((name.text.clone(), ty.clone()));
            n.states.push(name.text.clone());
        }
        StmtKind::Try { body, catch, finally } => {
            collect_block(body, n, false);
            if let Some((name, handler)) = catch {
                if let Some(name) = name {
                    n.defined.push((name.text.clone(), None));
                }
                collect_block(handler, n, false);
            }
            if let Some(f) = finally {
                collect_block(f, n, false);
            }
        }
        StmtKind::Match { arms, otherwise, .. } => {
            for arm in arms {
                collect_block(&arm.body, n, false);
            }
            if let Some(b) = otherwise {
                collect_block(b, n, false);
            }
        }
        StmtKind::Use { source, alias, names } => match names {
            Some(names) => n.defined.extend(names.iter().map(|x| (x.text.clone(), None))),
            None => n.defined.push((alias.as_ref().map(|a| a.text.clone()).unwrap_or_else(|| module_binding_name(source)), None)),
        },
        StmtKind::Export { inner: Some(inner), .. } => collect_stmt(inner, n, top),
        _ => {}
    }
}
