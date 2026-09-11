//! Converting between Lipi values and JSON text.

use crate::interp::{Flow, Interpreter};
use crate::value::{Fields, Value};
use lipi_compiler::Span;

pub fn parse(it: &Interpreter, text: &str, span: Span) -> Result<Value, Flow> {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(v) => Ok(from_json(v)),
        Err(e) => Err(it.error(
            format!("this isn't valid JSON ({e})"),
            span,
            Some("JSON looks like {\"name\": \"Dezy\", \"age\": 25}. Keys and text need double quotes.".into()),
        )),
    }
}

pub fn from_json(v: serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Nil,
        serde_json::Value::Bool(b) => Value::Bool(b),
        serde_json::Value::Number(n) => Value::Num(n.as_f64().unwrap_or(f64::NAN)),
        serde_json::Value::String(s) => Value::string(s),
        serde_json::Value::Array(items) => Value::list(items.into_iter().map(from_json).collect()),
        serde_json::Value::Object(map) => Value::object(map.into_iter().map(|(k, v)| (k, from_json(v))).collect::<Fields>()),
    }
}

pub fn to_json(v: &Value, depth: usize) -> Result<serde_json::Value, String> {
    if depth > 200 {
        return Err("it is nested too deeply (or contains itself)".into());
    }
    Ok(match v {
        Value::Nil => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Num(n) => {
            if n.fract() == 0.0 && n.abs() < 9.0e15 {
                serde_json::Value::from(*n as i64)
            } else {
                serde_json::Number::from_f64(*n)
                    .map(serde_json::Value::Number)
                    .ok_or_else(|| format!("{n} can't be written as JSON"))?
            }
        }
        Value::Str(s) => serde_json::Value::String(s.to_string()),
        Value::List(items) => {
            serde_json::Value::Array(items.borrow().iter().map(|x| to_json(x, depth + 1)).collect::<Result<_, _>>()?)
        }
        Value::Object(o) => {
            let mut map = serde_json::Map::new();
            for (k, v) in o.fields.borrow().iter() {
                if matches!(v, Value::Func(_) | Value::Native(_) | Value::Method(_)) {
                    continue; // methods like response.json() aren't data
                }
                map.insert(k.clone(), to_json(v, depth + 1)?);
            }
            serde_json::Value::Object(map)
        }
        other => return Err(format!("{} can't be turned into JSON", lipi_compiler::checker::with_article(&other.type_name()))),
    })
}

pub fn stringify(it: &Interpreter, v: &Value, pretty: bool, span: Span) -> Result<String, Flow> {
    let json = to_json(v, 0).map_err(|e| it.error(format!("can't convert to JSON: {e}"), span, None))?;
    Ok(if pretty { serde_json::to_string_pretty(&json) } else { serde_json::to_string(&json) }.unwrap_or_default())
}
