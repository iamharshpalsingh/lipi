//! Built-in methods and properties of Strings, Arrays, numbers, Objects and tasks.
//!
//! Methods that change an Array in place: push, pop, insert, removeAt, remove.
//! Everything else (sort, reverse, map, filter, slice...) returns a new Array.

use crate::builtins::{callable, int, need, opt_int, opt_text, text, to_number, whole};
use crate::interp::{Flow, Interpreter};
use crate::task;
use crate::value::*;
use lipi_compiler::checker::{with_article, LIST_MEMBERS, NUMBER_MEMBERS, OBJECT_MEMBERS, STRING_MEMBERS, TASK_MEMBERS};
use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;

pub fn members_for(v: &Value) -> Option<(&'static str, &'static [&'static str])> {
    Some(match v {
        Value::Str(_) => ("String", STRING_MEMBERS),
        Value::List(_) => ("Array", LIST_MEMBERS),
        Value::Int(_) => ("Integer", NUMBER_MEMBERS),
        Value::Num(_) => ("Decimal", NUMBER_MEMBERS),
        Value::Task(_) => ("Task", TASK_MEMBERS),
        Value::Object(_) => ("Object", OBJECT_MEMBERS),
        _ => return None,
    })
}

/// Properties are read without parentheses: `items.length`, `items.first`.
pub fn property(v: &Value, name: &str) -> Option<Value> {
    match (v, name) {
        (Value::Str(s), "length") => Some(Value::Int(s.chars().count() as i64)),
        (Value::List(l), "length") => Some(Value::Int(l.borrow().len() as i64)),
        (Value::List(l), "first") => Some(l.borrow().first().cloned().unwrap_or(Value::Nil)),
        (Value::List(l), "last") => Some(l.borrow().last().cloned().unwrap_or(Value::Nil)),
        _ => None,
    }
}

pub fn call(it: &mut Interpreter, recv: &Value, name: &str, a: &mut Args) -> Option<Result<Value, Flow>> {
    if property(recv, name).is_some() || (matches!(recv, Value::Object(_)) && name == "length") {
        return Some(Err(it.err("LIP5008", format!("\"{name}\" is a property, not a method"), a.span, Some(format!("Write it without parentheses: .{name}")))));
    }
    let result = match recv {
        Value::Str(s) => string_method(it, s, name, a),
        Value::List(l) => list_method(it, l, name, a),
        Value::Int(_) | Value::Num(_) => number_method(it, recv, name, a),
        Value::Object(o) => object_method(it, o, name, a),
        Value::Task(t) => task_method(it, t, name, a),
        _ => return None,
    };
    result.transpose()
}

/// Turn (possibly negative) start/end positions into a clamped range.
fn bounds(len: usize, start: Option<i64>, end: Option<i64>) -> (usize, usize) {
    let norm = |x: i64| -> usize {
        let x = if x < 0 { len as i64 + x } else { x };
        x.clamp(0, len as i64) as usize
    };
    let s = norm(start.unwrap_or(0));
    let e = norm(end.unwrap_or(len as i64));
    (s, e.max(s))
}

fn string_method(it: &mut Interpreter, s: &Rc<str>, name: &str, a: &mut Args) -> Result<Option<Value>, Flow> {
    let chars = || s.chars().collect::<Vec<char>>();
    Ok(Some(match name {
        "upper" => Value::string(s.to_uppercase()),
        "lower" => Value::string(s.to_lowercase()),
        "trim" => Value::text(s.trim()),
        "trimStart" => Value::text(s.trim_start()),
        "trimEnd" => Value::text(s.trim_end()),
        "split" => {
            let parts: Vec<Value> = match opt_text(it, a, 0, "separator")? {
                None => s.split_whitespace().map(Value::text).collect(),
                Some(sep) if sep.is_empty() => s.chars().map(|c| Value::string(c.to_string())).collect(),
                Some(sep) => s.split(&*sep).map(Value::text).collect(),
            };
            Value::list(parts)
        }
        "contains" => Value::Bool(s.contains(&*text(it, a, 0, "text")?)),
        "startsWith" => Value::Bool(s.starts_with(&*text(it, a, 0, "text")?)),
        "endsWith" => Value::Bool(s.ends_with(&*text(it, a, 0, "text")?)),
        "replace" => Value::string(s.replace(&*text(it, a, 0, "old")?, &text(it, a, 1, "new")?)),
        "indexOf" => {
            let needle = text(it, a, 0, "text")?;
            match s.find(&*needle) {
                Some(b) => Value::Int(s[..b].chars().count() as i64),
                None => Value::Nil,
            }
        }
        "slice" => {
            let c = chars();
            let (st, en) = bounds(c.len(), opt_int(it, a, 0, "start")?, opt_int(it, a, 1, "end")?);
            Value::string(c[st..en].iter().collect())
        }
        "repeat" => {
            let n = int(it, a, 0, "count")?;
            if n < 0 {
                return Err(it.err("LIP5008", "repeat() needs a count that isn't negative", a.span, None));
            }
            Value::string(s.repeat(n as usize))
        }
        "chars" => Value::list(s.chars().map(|c| Value::string(c.to_string())).collect()),
        "lines" => Value::list(s.lines().map(Value::text).collect()),
        "isEmpty" => Value::Bool(s.is_empty()),
        "padStart" | "padEnd" => {
            let width = int(it, a, 0, "width")?.max(0) as usize;
            let fill = opt_text(it, a, 1, "fill")?.unwrap_or_else(|| Rc::from(" "));
            let len = s.chars().count();
            if len >= width || fill.is_empty() {
                Value::Str(s.clone())
            } else {
                let padding: String = fill.chars().cycle().take(width - len).collect();
                Value::string(if name == "padStart" { format!("{padding}{s}") } else { format!("{s}{padding}") })
            }
        }
        "reverse" => Value::string(s.chars().rev().collect()),
        "toNumber" => to_number(&Value::Str(s.clone())),
        _ => return Ok(None),
    }))
}

fn position(it: &Interpreter, a: &Args, i: i64, len: usize, allow_end: bool) -> Result<usize, Flow> {
    let idx = if i < 0 { len as i64 + i } else { i };
    let max = if allow_end { len as i64 } else { len as i64 - 1 };
    if idx < 0 || idx > max {
        return Err(it.err(
            "LIP5001",
            format!("index {i} is outside array length {len}"),
            a.span,
            Some("Positions start at 0. Negative positions count from the end: -1 is the last item.".into()),
        ));
    }
    Ok(idx as usize)
}

fn compare_values(x: &Value, y: &Value) -> Option<Ordering> {
    match (x, y) {
        (Value::Str(a), Value::Str(b)) => Some(a.cmp(b)),
        (a, b) => a.as_f64()?.partial_cmp(&b.as_f64()?),
    }
}

fn check_sortable(it: &Interpreter, a: &Args, keys: &[Value]) -> Result<(), Flow> {
    let all_num = keys.iter().all(Value::is_number);
    let all_str = keys.iter().all(|k| matches!(k, Value::Str(_)));
    if all_num || all_str {
        return Ok(());
    }
    let mut kinds: Vec<String> = keys.iter().map(|k| k.type_name()).collect();
    kinds.sort();
    kinds.dedup();
    Err(it.err(
        "LIP5008",
        format!("cannot sort values that are {}", kinds.join(" and ")),
        a.span,
        Some("sort() works on all-number or all-String Arrays. Use sortBy(item => ...) to choose what to sort by.".into()),
    ))
}

fn list_method(it: &mut Interpreter, l: &Rc<RefCell<Vec<Value>>>, name: &str, a: &mut Args) -> Result<Option<Value>, Flow> {
    let snapshot = || l.borrow().clone();
    let span = a.span;
    let what = format!("the function given to {name}()");
    Ok(Some(match name {
        "push" => {
            if a.pos.is_empty() {
                return Err(it.err("LIP5008", "push() needs an item to add", span, None));
            }
            l.borrow_mut().extend(a.pos.drain(..));
            Value::Nil
        }
        "pop" => {
            let item = l.borrow_mut().pop();
            match item {
                Some(v) => v,
                None => return Err(it.err("LIP5001", "cannot pop from an empty Array", span, Some("Check .isEmpty() first.".into()))),
            }
        }
        "insert" => {
            let len = l.borrow().len();
            let i = position(it, a, int(it, a, 0, "position")?, len, true)?;
            let item = need(it, a, 1, "item")?.clone();
            l.borrow_mut().insert(i, item);
            Value::Nil
        }
        "removeAt" => {
            let len = l.borrow().len();
            let i = position(it, a, int(it, a, 0, "position")?, len, false)?;
            l.borrow_mut().remove(i)
        }
        "remove" => {
            let item = need(it, a, 0, "item")?.clone();
            let found = l.borrow().iter().position(|x| x.equals(&item));
            match found {
                Some(i) => {
                    l.borrow_mut().remove(i);
                    Value::Bool(true)
                }
                None => Value::Bool(false),
            }
        }
        "contains" => {
            let item = need(it, a, 0, "item")?;
            Value::Bool(l.borrow().iter().any(|x| x.equals(item)))
        }
        "indexOf" => {
            let item = need(it, a, 0, "item")?;
            match l.borrow().iter().position(|x| x.equals(item)) {
                Some(i) => Value::Int(i as i64),
                None => Value::Nil,
            }
        }
        "join" => {
            let sep = opt_text(it, a, 0, "separator")?.unwrap_or_else(|| Rc::from(""));
            Value::string(l.borrow().iter().map(Value::display).collect::<Vec<_>>().join(&sep))
        }
        "map" => {
            let f = callable(it, a, 0, "function")?;
            let mut out = Vec::new();
            for (i, item) in snapshot().into_iter().enumerate() {
                out.push(it.call_callback(&f, vec![item, Value::Int(i as i64)], span)?);
            }
            Value::list(out)
        }
        "filter" => {
            let f = callable(it, a, 0, "function")?;
            let mut out = Vec::new();
            for (i, item) in snapshot().into_iter().enumerate() {
                let keep = it.call_callback(&f, vec![item.clone(), Value::Int(i as i64)], span)?;
                if it.expect_bool(&keep, span, &what)? {
                    out.push(item);
                }
            }
            Value::list(out)
        }
        "each" => {
            let f = callable(it, a, 0, "function")?;
            for (i, item) in snapshot().into_iter().enumerate() {
                it.call_callback(&f, vec![item, Value::Int(i as i64)], span)?;
            }
            Value::Nil
        }
        "reduce" => {
            let f = callable(it, a, 0, "function")?;
            let mut items = snapshot().into_iter();
            let mut acc = match a.get(1, "start") {
                Some(v) => v.clone(),
                None => match items.next() {
                    Some(v) => v,
                    None => return Ok(Some(Value::Nil)),
                },
            };
            for item in items {
                acc = it.call_callback(&f, vec![acc, item], span)?;
            }
            acc
        }
        "find" => {
            let f = callable(it, a, 0, "function")?;
            for item in snapshot() {
                let hit = it.call_callback(&f, vec![item.clone()], span)?;
                if it.expect_bool(&hit, span, &what)? {
                    return Ok(Some(item));
                }
            }
            Value::Nil
        }
        "any" | "all" => {
            let f = callable(it, a, 0, "function")?;
            let want_any = name == "any";
            for item in snapshot() {
                let r = it.call_callback(&f, vec![item], span)?;
                if it.expect_bool(&r, span, &what)? == want_any {
                    return Ok(Some(Value::Bool(want_any)));
                }
            }
            Value::Bool(!want_any)
        }
        "count" => {
            if a.pos.is_empty() {
                Value::Int(l.borrow().len() as i64)
            } else {
                let f = callable(it, a, 0, "function")?;
                let mut n = 0;
                for item in snapshot() {
                    let r = it.call_callback(&f, vec![item], span)?;
                    if it.expect_bool(&r, span, &what)? {
                        n += 1;
                    }
                }
                Value::Int(n)
            }
        }
        "sort" => {
            let mut items = snapshot();
            check_sortable(it, a, &items)?;
            items.sort_by(|x, y| compare_values(x, y).unwrap_or(Ordering::Equal));
            Value::list(items)
        }
        "sortBy" => {
            let f = callable(it, a, 0, "function")?;
            let mut pairs = Vec::new();
            for item in snapshot() {
                let key = it.call_callback(&f, vec![item.clone()], span)?;
                pairs.push((key, item));
            }
            let keys: Vec<Value> = pairs.iter().map(|p| p.0.clone()).collect();
            check_sortable(it, a, &keys)?;
            pairs.sort_by(|x, y| compare_values(&x.0, &y.0).unwrap_or(Ordering::Equal));
            Value::list(pairs.into_iter().map(|p| p.1).collect())
        }
        "reverse" => Value::list(snapshot().into_iter().rev().collect()),
        "slice" => {
            let items = snapshot();
            let (st, en) = bounds(items.len(), opt_int(it, a, 0, "start")?, opt_int(it, a, 1, "end")?);
            Value::list(items[st..en].to_vec())
        }
        "sum" => {
            let items = snapshot();
            let mut int_total: Option<i64> = Some(0);
            let mut total = 0.0;
            for (i, item) in items.iter().enumerate() {
                match item {
                    Value::Int(n) => {
                        int_total = int_total.and_then(|t| t.checked_add(*n));
                        total += *n as f64;
                    }
                    Value::Num(n) => {
                        int_total = None;
                        total += n;
                    }
                    other => {
                        return Err(it.err(
                            "LIP5008",
                            format!("sum() needs an Array of numbers, but item {i} is {}", with_article(&other.type_name())),
                            span,
                            None,
                        ))
                    }
                }
            }
            let all_int = items.iter().all(|v| matches!(v, Value::Int(_)));
            match int_total {
                Some(t) if all_int => Value::Int(t),
                _ => Value::Num(total),
            }
        }
        "min" | "max" => {
            let items = snapshot();
            check_sortable(it, a, &items)?;
            let pick = items.into_iter().reduce(|x, y| {
                let ord = compare_values(&y, &x).unwrap_or(Ordering::Equal);
                if (name == "min" && ord == Ordering::Less) || (name == "max" && ord == Ordering::Greater) { y } else { x }
            });
            pick.unwrap_or(Value::Nil)
        }
        "isEmpty" => Value::Bool(l.borrow().is_empty()),
        "copy" => Value::list(snapshot()),
        "unique" => {
            let mut out: Vec<Value> = Vec::new();
            for item in snapshot() {
                if !out.iter().any(|x| x.equals(&item)) {
                    out.push(item);
                }
            }
            Value::list(out)
        }
        "flat" => {
            let mut out = Vec::new();
            for item in snapshot() {
                match item {
                    Value::List(inner) => out.extend(inner.borrow().iter().cloned()),
                    other => out.push(other),
                }
            }
            Value::list(out)
        }
        _ => return Ok(None),
    }))
}

fn number_method(it: &mut Interpreter, n: &Value, name: &str, a: &mut Args) -> Result<Option<Value>, Flow> {
    let x = n.as_f64().unwrap_or(0.0);
    Ok(Some(match (name, n) {
        ("round" | "floor" | "ceil", Value::Int(_)) if a.pos.is_empty() => n.clone(),
        ("round", _) => match opt_int(it, a, 0, "digits")? {
            None => whole(x.round()),
            Some(d) => {
                let factor = 10f64.powi(d as i32);
                Value::Num((x * factor).round() / factor)
            }
        },
        ("floor", _) => whole(x.floor()),
        ("ceil", _) => whole(x.ceil()),
        ("abs", Value::Int(i)) => Value::Int(i.checked_abs().ok_or_else(|| it.err("LIP5009", "this Integer calculation overflowed", a.span, None))?),
        ("abs", _) => Value::Num(x.abs()),
        ("toString", _) => Value::string(n.display()),
        _ => return Ok(None),
    }))
}

fn object_method(it: &mut Interpreter, o: &Rc<ObjectData>, name: &str, a: &mut Args) -> Result<Option<Value>, Flow> {
    Ok(Some(match name {
        "keys" => Value::list(o.fields.borrow().keys().map(|k| Value::text(k)).collect()),
        "values" => Value::list(o.fields.borrow().values().cloned().collect()),
        "entries" => Value::list(o.fields.borrow().iter().map(|(k, v)| Value::list(vec![Value::text(k), v.clone()])).collect()),
        "has" => Value::Bool(o.fields.borrow().contains_key(&*text(it, a, 0, "key")?)),
        "get" => {
            let key = text(it, a, 0, "key")?;
            let found = o.fields.borrow().get(&*key).cloned();
            found.unwrap_or_else(|| a.get(1, "default").cloned().unwrap_or(Value::Nil))
        }
        "remove" => {
            let key = text(it, a, 0, "key")?;
            if o.ty.is_some() {
                return Err(it.err("LIP5008", "cannot remove a field from a typed Object", a.span, Some("Set it to null instead.".into())));
            }
            let removed = o.fields.borrow_mut().shift_remove(&*key);
            removed.unwrap_or(Value::Nil)
        }
        "copy" => Value::Object(Rc::new(ObjectData { fields: RefCell::new(o.fields.borrow().clone()), ty: o.ty.clone(), module: None, tag: None, payload: None })),
        "isEmpty" => Value::Bool(o.fields.borrow().is_empty()),
        _ => return Ok(None),
    }))
}

fn task_method(it: &mut Interpreter, t: &Rc<RefCell<task::TaskState>>, name: &str, a: &mut Args) -> Result<Option<Value>, Flow> {
    Ok(Some(match name {
        "cancel" => {
            let running = matches!(&*t.borrow(), task::TaskState::Pending { .. });
            if running {
                *t.borrow_mut() = task::TaskState::Cancelled;
            }
            Value::Bool(running)
        }
        "isDone" => Value::Bool(task::poll(it, t, a.span)),
        _ => return Ok(None),
    }))
}
