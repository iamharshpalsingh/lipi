//! Which variables a function body (or file) creates. Shared by the
//! interpreter's slot resolver and the JavaScript code generator, so both apply
//! LiPi's scope rule the same way: assignment updates the nearest existing
//! variable, otherwise it creates one in the current function. Blocks such as
//! `if` and `for` don't have their own scope.

use crate::ast::*;
use crate::checker::module_binding_name;

#[derive(Default)]
pub struct ScopeNames {
    /// Always local: variables of `for`, `catch` and `use`, functions, types,
    /// components, typed and constant variables, and `state`.
    pub defined: Vec<(String, Option<TypeExpr>)>,
    /// Plain assignments: local unless an enclosing scope has the name.
    pub assigned: Vec<String>,
    /// Functions, types and components at the top of the body (defined before it runs).
    pub hoisted: Vec<String>,
    /// `state` variables.
    pub states: Vec<String>,
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
        StmtKind::For { first, second, body, pattern, .. } => {
            n.defined.push((first.text.clone(), None));
            if let Some(s) = second {
                n.defined.push((s.text.clone(), None));
            }
            if let Some(p) = pattern {
                n.defined.extend(p.names().into_iter().map(|x| (x.text.clone(), None)));
            }
            collect_block(body, n, false);
        }
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
