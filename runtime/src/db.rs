//! The database layer: SQLite and PostgreSQL behind one API.
//!
//! ```lipi
//! db = database.open("shop.db")                         # SQLite file (":memory:" for a throwaway one)
//! db = database.open("postgres://user:pw@host/shop")    # PostgreSQL
//! user = db.users.create({name: "Dezy", age: 25})
//! adults = db.users.where(active: true, order: "name")
//! db.orders.update(order.id, {status: "paid"})
//! rows = db.query("SELECT * FROM users WHERE age > ?", [18])
//! ```
//!
//! - Tables are created on the first `create`, and new fields add columns.
//! - Values are always sent as parameters, never pasted into SQL.
//! - `?` and `:name` placeholders work on both databases (they are rewritten
//!   to `$1, $2...` for PostgreSQL).
//! - Table and column names must be plain identifiers.

use crate::builtins::{callable, civil_from_days, need, opt_int, text, MODULES};
use crate::interp::{Flow, Interpreter};
use crate::json;
use crate::value::*;
use bytes::{BufMut, BytesMut};
use lipi_compiler::checker::with_article;
use lipi_compiler::Span;
use postgres::types::{to_sql_checked, FromSql, IsNull, ToSql as PgToSql, Type};
use rusqlite::types::{Value as Sql, ValueRef};
use rusqlite::{params_from_iter, Connection, ToSql};
use std::cell::{RefCell, RefMut};
use std::collections::HashMap;
use std::error::Error;
use std::rc::Rc;

type BoxError = Box<dyn Error + Sync + Send>;

// ----- parameters -------------------------------------------------------------

/// A value sent to the database.
#[derive(Debug, Clone)]
enum Param {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Json(String),
}

impl Param {
    fn lite(&self) -> Sql {
        match self {
            Param::Null => Sql::Null,
            Param::Bool(b) => Sql::Integer(*b as i64),
            Param::Int(n) => Sql::Integer(*n),
            Param::Float(f) => Sql::Real(*f),
            Param::Text(s) | Param::Json(s) => Sql::Text(s.clone()),
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Param::Null => "null",
            Param::Bool(_) => "a Boolean",
            Param::Int(_) => "an Integer",
            Param::Float(_) => "a Decimal",
            Param::Text(_) => "a String",
            Param::Json(_) => "an Array/Object",
        }
    }

    fn as_text(&self) -> String {
        match self {
            Param::Null => String::new(),
            Param::Bool(b) => b.to_string(),
            Param::Int(n) => n.to_string(),
            Param::Float(f) => f.to_string(),
            Param::Text(s) | Param::Json(s) => s.clone(),
        }
    }
}

fn to_param(it: &Interpreter, v: &Value, span: Span) -> Result<Param, Flow> {
    Ok(match v {
        Value::Nil => Param::Null,
        Value::Bool(b) => Param::Bool(*b),
        Value::Int(n) => Param::Int(*n),
        Value::Num(n) => Param::Float(*n),
        Value::Str(s) => Param::Text(s.to_string()),
        Value::List(_) | Value::Object(_) => Param::Json(json::stringify(it, v, false, span)?),
        other => return Err(it.err("LIP5008", format!("{} can't be stored in a database", with_article(&other.type_name())), span, None)),
    })
}

const EPOCH_2000_SECS: i64 = 946_684_800;
const EPOCH_2000_DAYS: i64 = 10_957;

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = (m as u64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

/// "2026-09-11", "2026-09-11T10:30:00Z", "2026-09-11 10:30:00.5+05:30" → microseconds since 1970.
fn parse_timestamp(s: &str) -> Result<i64, BoxError> {
    let s = s.trim();
    let bad = || -> BoxError { format!("\"{s}\" isn't a date like 2026-09-11 or 2026-09-11T10:30:00Z").into() };
    let date = s.get(..10).ok_or_else(bad)?;
    let mut parts = date.split('-');
    let y: i64 = parts.next().and_then(|p| p.parse().ok()).ok_or_else(bad)?;
    let m: u32 = parts.next().and_then(|p| p.parse().ok()).ok_or_else(bad)?;
    let d: u32 = parts.next().and_then(|p| p.parse().ok()).ok_or_else(bad)?;
    let mut secs = days_from_civil(y, m, d) * 86_400;
    let mut micros = 0i64;
    let mut rest = &s[10..];
    if let Some(time) = rest.strip_prefix('T').or_else(|| rest.strip_prefix(' ')) {
        let end = time.find(|c: char| c == 'Z' || c == '+' || (c == '-')).unwrap_or(time.len());
        let (clock, zone) = time.split_at(end);
        let (hms, frac) = clock.split_once('.').unwrap_or((clock, ""));
        let nums: Vec<i64> = hms.split(':').map(|p| p.parse()).collect::<Result<_, _>>().map_err(|_| bad())?;
        secs += nums.first().copied().unwrap_or(0) * 3600 + nums.get(1).copied().unwrap_or(0) * 60 + nums.get(2).copied().unwrap_or(0);
        if !frac.is_empty() {
            let digits: String = frac.chars().chain(std::iter::repeat('0')).take(6).collect();
            micros = digits.parse().map_err(|_| bad())?;
        }
        rest = zone;
    }
    if let Some(offset) = rest.strip_prefix('+').map(|o| (1, o)).or_else(|| rest.strip_prefix('-').map(|o| (-1, o))) {
        let (sign, o) = offset;
        let (h, m) = o.split_once(':').unwrap_or((o, "0"));
        let off = h.parse::<i64>().map_err(|_| bad())? * 3600 + m.parse::<i64>().map_err(|_| bad())? * 60;
        secs -= sign * off;
    }
    Ok(secs * 1_000_000 + micros)
}

fn format_timestamp(unix_micros: i64, utc: bool) -> String {
    let secs = unix_micros.div_euclid(1_000_000);
    let micros = unix_micros.rem_euclid(1_000_000);
    let (y, mo, d) = civil_from_days(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    let mut s = format!("{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}", rem / 3600, (rem % 3600) / 60, rem % 60);
    if micros != 0 {
        s.push_str(&format!(".{:03}", micros / 1000));
    }
    if utc {
        s.push('Z');
    }
    s
}

/// PostgreSQL's binary NUMERIC format, from a plain decimal string.
fn encode_numeric(s: &str, out: &mut BytesMut) -> Result<(), BoxError> {
    let s = s.trim();
    let (neg, s) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s),
    };
    let (int, frac) = s.split_once('.').unwrap_or((s, ""));
    if !int.chars().chain(frac.chars()).all(|c| c.is_ascii_digit()) || (int.is_empty() && frac.is_empty()) {
        return Err(format!("\"{s}\" isn't a number").into());
    }
    let int = int.trim_start_matches('0');
    let int_s = format!("{}{int}", "0".repeat((4 - int.len() % 4) % 4));
    let frac_s = format!("{frac}{}", "0".repeat((4 - frac.len() % 4) % 4));
    let mut groups: Vec<i16> = (0..int_s.len()).step_by(4).chain((0..frac_s.len()).step_by(4).map(|i| i + int_s.len())).map(|i| {
        let all = format!("{int_s}{frac_s}");
        all[i..i + 4].parse::<i16>().unwrap_or(0)
    }).collect();
    let mut weight = (int_s.len() / 4) as i16 - 1;
    while groups.first() == Some(&0) {
        groups.remove(0);
        weight -= 1;
    }
    while groups.last() == Some(&0) {
        groups.pop();
    }
    if groups.is_empty() {
        weight = 0;
    }
    out.put_i16(groups.len() as i16);
    out.put_i16(weight);
    out.put_u16(if neg && !groups.is_empty() { 0x4000 } else { 0 });
    out.put_u16(frac.len() as u16);
    for g in groups {
        out.put_i16(g);
    }
    Ok(())
}

impl PgToSql for Param {
    fn to_sql(&self, ty: &Type, out: &mut BytesMut) -> Result<IsNull, BoxError> {
        if matches!(self, Param::Null) {
            return Ok(IsNull::Yes);
        }
        let fail = || -> BoxError { format!("can't send {} to a column of type {ty} (cast it in SQL, for example $1::text)", self.kind()).into() };
        let whole = || -> Result<i64, BoxError> {
            match self {
                Param::Int(n) => Ok(*n),
                Param::Float(f) if f.fract() == 0.0 => Ok(*f as i64),
                Param::Text(s) => s.trim().parse().map_err(|_| fail()),
                _ => Err(fail()),
            }
        };
        if *ty == Type::BOOL {
            match self {
                Param::Bool(b) => out.put_u8(*b as u8),
                _ => return Err(fail()),
            }
        } else if *ty == Type::INT2 {
            out.put_i16(i16::try_from(whole()?)?);
        } else if *ty == Type::INT4 {
            out.put_i32(i32::try_from(whole()?)?);
        } else if *ty == Type::INT8 {
            out.put_i64(whole()?);
        } else if *ty == Type::FLOAT4 || *ty == Type::FLOAT8 {
            let f = match self {
                Param::Int(n) => *n as f64,
                Param::Float(f) => *f,
                Param::Text(s) => s.trim().parse().map_err(|_| fail())?,
                _ => return Err(fail()),
            };
            if *ty == Type::FLOAT4 {
                out.put_f32(f as f32);
            } else {
                out.put_f64(f);
            }
        } else if *ty == Type::NUMERIC {
            match self {
                Param::Int(_) | Param::Float(_) | Param::Text(_) => encode_numeric(&self.as_text(), out)?,
                _ => return Err(fail()),
            }
        } else if *ty == Type::JSON || *ty == Type::JSONB {
            let text = match self {
                Param::Text(s) => serde_json::to_string(s)?,
                other => other.as_text(),
            };
            if *ty == Type::JSONB {
                out.put_u8(1);
            }
            out.put_slice(text.as_bytes());
        } else if *ty == Type::UUID {
            let Param::Text(s) = self else { return Err(fail()) };
            let hex: String = s.chars().filter(|c| *c != '-').collect();
            if hex.len() != 32 {
                return Err(format!("\"{s}\" isn't a UUID").into());
            }
            for i in (0..32).step_by(2) {
                out.put_u8(u8::from_str_radix(&hex[i..i + 2], 16)?);
            }
        } else if *ty == Type::TIMESTAMP || *ty == Type::TIMESTAMPTZ {
            let unix_micros = match self {
                Param::Int(ms) => ms * 1000, // time.now() values are milliseconds
                Param::Text(s) => parse_timestamp(s)?,
                _ => return Err(fail()),
            };
            out.put_i64(unix_micros - EPOCH_2000_SECS * 1_000_000);
        } else if *ty == Type::DATE {
            let Param::Text(s) = self else { return Err(fail()) };
            let days = parse_timestamp(s)?.div_euclid(86_400_000_000);
            out.put_i32((days - EPOCH_2000_DAYS) as i32);
        } else {
            // TEXT, VARCHAR, NAME, unknown (untyped) parameters...
            out.put_slice(self.as_text().as_bytes());
        }
        Ok(IsNull::No)
    }

    fn accepts(_: &Type) -> bool {
        true
    }

    to_sql_checked!();
}

// ----- results --------------------------------------------------------------------

/// A value read from PostgreSQL.
enum Cell {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Json(serde_json::Value),
}

fn be<const N: usize>(raw: &[u8]) -> Result<[u8; N], BoxError> {
    raw.get(..N).and_then(|s| s.try_into().ok()).ok_or_else(|| "value too short".into())
}

fn decode_numeric(raw: &[u8]) -> Result<Cell, BoxError> {
    let ndigits = i16::from_be_bytes(be(raw)?) as usize;
    let weight = i16::from_be_bytes(be(&raw[2..])?) as i64;
    let sign = u16::from_be_bytes(be(&raw[4..])?);
    let dscale = u16::from_be_bytes(be(&raw[6..])?) as usize;
    if sign == 0xC000 {
        return Ok(Cell::Float(f64::NAN));
    }
    let digits: Vec<i64> = (0..ndigits).map(|i| be::<2>(&raw[8 + 2 * i..]).map(|b| i16::from_be_bytes(b) as i64)).collect::<Result<_, _>>()?;
    let digit = |i: i64| if i >= 0 && (i as usize) < digits.len() { digits[i as usize] } else { 0 };
    let mut int = String::new();
    for i in 0..=weight.max(-1) {
        if int.is_empty() {
            int.push_str(&digit(i).to_string());
        } else {
            int.push_str(&format!("{:04}", digit(i)));
        }
    }
    if int.is_empty() {
        int.push('0');
    }
    let mut frac = String::new();
    let mut i = weight + 1;
    while frac.len() < dscale {
        frac.push_str(&format!("{:04}", digit(i)));
        i += 1;
    }
    frac.truncate(dscale);
    let text = format!("{}{int}{}", if sign == 0x4000 { "-" } else { "" }, if frac.is_empty() { String::new() } else { format!(".{frac}") });
    if dscale == 0 {
        if let Ok(n) = text.parse::<i64>() {
            return Ok(Cell::Int(n));
        }
    }
    Ok(Cell::Float(text.parse()?))
}

impl<'a> FromSql<'a> for Cell {
    fn from_sql(ty: &Type, raw: &'a [u8]) -> Result<Self, BoxError> {
        Ok(if *ty == Type::BOOL {
            Cell::Bool(raw.first().is_some_and(|b| *b != 0))
        } else if *ty == Type::INT2 {
            Cell::Int(i16::from_be_bytes(be(raw)?) as i64)
        } else if *ty == Type::INT4 {
            Cell::Int(i32::from_be_bytes(be(raw)?) as i64)
        } else if *ty == Type::INT8 {
            Cell::Int(i64::from_be_bytes(be(raw)?))
        } else if *ty == Type::OID {
            Cell::Int(u32::from_be_bytes(be(raw)?) as i64)
        } else if *ty == Type::FLOAT4 {
            Cell::Float(f32::from_be_bytes(be(raw)?) as f64)
        } else if *ty == Type::FLOAT8 {
            Cell::Float(f64::from_be_bytes(be(raw)?))
        } else if *ty == Type::NUMERIC {
            decode_numeric(raw)?
        } else if *ty == Type::JSON {
            Cell::Json(serde_json::from_slice(raw)?)
        } else if *ty == Type::JSONB {
            Cell::Json(serde_json::from_slice(raw.get(1..).unwrap_or_default())?)
        } else if *ty == Type::UUID {
            let h: String = raw.iter().map(|b| format!("{b:02x}")).collect();
            Cell::Text(format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32]))
        } else if *ty == Type::TIMESTAMP || *ty == Type::TIMESTAMPTZ {
            let micros = i64::from_be_bytes(be(raw)?) + EPOCH_2000_SECS * 1_000_000;
            Cell::Text(format_timestamp(micros, *ty == Type::TIMESTAMPTZ))
        } else if *ty == Type::DATE {
            let days = i32::from_be_bytes(be(raw)?) as i64 + EPOCH_2000_DAYS;
            let (y, m, d) = civil_from_days(days);
            Cell::Text(format!("{y:04}-{m:02}-{d:02}"))
        } else {
            Cell::Text(String::from_utf8_lossy(raw).into_owned())
        })
    }

    fn from_sql_null(_: &Type) -> Result<Self, BoxError> {
        Ok(Cell::Null)
    }

    fn accepts(_: &Type) -> bool {
        true
    }
}

impl Cell {
    fn into_value(self) -> Value {
        match self {
            Cell::Null => Value::Nil,
            Cell::Bool(b) => Value::Bool(b),
            Cell::Int(n) => Value::Int(n),
            Cell::Float(f) => Value::Num(f),
            Cell::Text(s) => Value::string(s),
            Cell::Json(j) => json::from_json(j),
        }
    }
}

// ----- connections ----------------------------------------------------------------

enum Params {
    Positional(Vec<Param>),
    Named(Vec<(String, Param)>),
}

fn no_params() -> Params {
    Params::Positional(Vec::new())
}

enum Conn {
    Lite(Connection),
    Pg(postgres::Client),
}

fn lite_error(it: &Interpreter, e: rusqlite::Error, span: Span) -> Flow {
    db_error(it, e.to_string(), span)
}

fn pg_error(it: &Interpreter, e: postgres::Error, span: Span) -> Flow {
    let message = match e.as_db_error() {
        Some(db) => db.message().to_string(),
        None => e.to_string(),
    };
    db_error(it, message, span)
}

fn db_error(it: &Interpreter, message: String, span: Span) -> Flow {
    let hint = if message.contains("no such table") || (message.contains("relation") && message.contains("does not exist")) {
        Some("Create the table first: insert a row with db.<table>.create({...}) or run a migration.")
    } else if message.contains("no such column") || (message.contains("column") && message.contains("does not exist")) {
        Some("Check the column name. New columns are added automatically by create() and update().")
    } else if message.contains("UNIQUE constraint failed") || message.contains("duplicate key") {
        Some("A row with this value already exists.")
    } else if message.contains("syntax error") {
        Some("Check the SQL. Values go in as ? placeholders: db.query(\"SELECT * FROM users WHERE id = ?\", [id])")
    } else if message.contains("error connecting") || message.contains("password authentication") {
        Some("Check the connection URL, for example postgres://user:password@localhost:5432/mydb")
    } else {
        None
    };
    it.err("LIP5011", format!("database error: {message}"), span, hint.map(String::from))
}

/// Rewrite `?` and `:name` placeholders to PostgreSQL's `$1, $2, ...`.
fn pg_translate(it: &Interpreter, sql: &str, params: &Params, span: Span) -> Result<(String, Vec<Param>), Flow> {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = String::with_capacity(sql.len());
    let mut ordered: Vec<Param> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let mut in_quote = false;
    let mut next_positional = 0;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            in_quote = !in_quote;
            out.push(c);
        } else if in_quote {
            out.push(c);
        } else if c == '?' {
            if let Params::Positional(list) = params {
                let Some(p) = list.get(next_positional) else {
                    return Err(it.err("LIP5008", format!("the query has more ? placeholders than values ({} given)", list.len()), span, None));
                };
                ordered.push(p.clone());
                next_positional += 1;
                out.push_str(&format!("${}", ordered.len()));
            } else {
                out.push(c);
            }
        } else if c == ':' && matches!(params, Params::Named(_)) && chars.get(i + 1).is_some_and(|n| n.is_ascii_alphabetic() || *n == '_') && (i == 0 || chars[i - 1] != ':') {
            let start = i + 1;
            let mut end = start;
            while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_') {
                end += 1;
            }
            let name: String = chars[start..end].iter().collect();
            let Params::Named(map) = params else { unreachable!() };
            let index = match names.iter().position(|n| *n == name) {
                Some(p) => p + 1,
                None => {
                    let Some((_, value)) = map.iter().find(|(k, _)| *k == name) else {
                        return Err(it.err("LIP5008", format!("the query uses :{name}, but no value named \"{name}\" was given"), span, None));
                    };
                    names.push(name);
                    ordered.push(value.clone());
                    ordered.len()
                }
            };
            out.push_str(&format!("${index}"));
            i = end;
            continue;
        } else {
            out.push(c);
        }
        i += 1;
    }
    if let Params::Positional(list) = params {
        if next_positional == 0 {
            return Ok((sql.to_string(), list.clone())); // already $1-style
        }
    }
    Ok((out, ordered))
}

impl Conn {
    fn is_pg(&self) -> bool {
        matches!(self, Conn::Pg(_))
    }

    fn placeholder(&self, n: usize) -> String {
        if self.is_pg() { format!("${n}") } else { "?".into() }
    }

    fn query(&mut self, it: &Interpreter, sql: &str, params: &Params, span: Span) -> Result<Vec<Value>, Flow> {
        match self {
            Conn::Lite(c) => lite_query(it, c, sql, params, span),
            Conn::Pg(c) => {
                let (sql, list) = pg_translate(it, sql, params, span)?;
                let refs: Vec<&(dyn PgToSql + Sync)> = list.iter().map(|p| p as &(dyn PgToSql + Sync)).collect();
                let rows = c.query(sql.as_str(), &refs).map_err(|e| pg_error(it, e, span))?;
                Ok(rows
                    .iter()
                    .map(|row| {
                        let mut f = Fields::new();
                        for (i, col) in row.columns().iter().enumerate() {
                            let cell: Cell = row.try_get(i).unwrap_or(Cell::Null);
                            f.insert(col.name().to_string(), cell.into_value());
                        }
                        Value::object(f)
                    })
                    .collect())
            }
        }
    }

    fn execute(&mut self, it: &Interpreter, sql: &str, params: &Params, span: Span) -> Result<u64, Flow> {
        match self {
            Conn::Lite(c) => match params {
                Params::Positional(v) => c.execute(sql, params_from_iter(v.iter().map(Param::lite))),
                Params::Named(v) => {
                    let values: Vec<(String, Sql)> = v.iter().map(|(k, p)| (format!(":{k}"), p.lite())).collect();
                    let refs: Vec<(&str, &dyn ToSql)> = values.iter().map(|(k, val)| (k.as_str(), val as &dyn ToSql)).collect();
                    c.execute(sql, refs.as_slice())
                }
            }
            .map(|n| n as u64)
            .map_err(|e| lite_error(it, e, span)),
            Conn::Pg(c) => {
                let (sql, list) = pg_translate(it, sql, params, span)?;
                let refs: Vec<&(dyn PgToSql + Sync)> = list.iter().map(|p| p as &(dyn PgToSql + Sync)).collect();
                c.execute(sql.as_str(), &refs).map_err(|e| pg_error(it, e, span))
            }
        }
    }

    fn batch(&mut self, it: &Interpreter, sql: &str, span: Span) -> Result<(), Flow> {
        match self {
            Conn::Lite(c) => c.execute_batch(sql).map_err(|e| lite_error(it, e, span)),
            Conn::Pg(c) => c.batch_execute(sql).map_err(|e| pg_error(it, e, span)),
        }
    }

    /// Run an INSERT and return the new row's id.
    fn insert(&mut self, it: &Interpreter, sql: &str, params: &Params, span: Span) -> Result<i64, Flow> {
        if self.is_pg() {
            let rows = self.query(it, &format!("{sql} RETURNING \"id\""), params, span)?;
            return Ok(match rows.first() {
                Some(Value::Object(o)) => match o.fields.borrow().get("id") {
                    Some(Value::Int(n)) => *n,
                    _ => 0,
                },
                _ => 0,
            });
        }
        self.execute(it, sql, params, span)?;
        match self {
            Conn::Lite(c) => Ok(c.last_insert_rowid()),
            Conn::Pg(_) => unreachable!(),
        }
    }

    fn last_id(&self) -> i64 {
        match self {
            Conn::Lite(c) => c.last_insert_rowid(),
            Conn::Pg(_) => 0,
        }
    }
}

fn lite_query(it: &Interpreter, conn: &Connection, sql: &str, params: &Params, span: Span) -> Result<Vec<Value>, Flow> {
    let mut stmt = conn.prepare(sql).map_err(|e| lite_error(it, e, span))?;
    let cols: Vec<(String, String)> = stmt.columns().iter().map(|c| (c.name().to_string(), c.decl_type().unwrap_or("").to_uppercase())).collect();
    let mut rows = match params {
        Params::Positional(v) => stmt.query(params_from_iter(v.iter().map(Param::lite))),
        Params::Named(v) => {
            let values: Vec<(String, Sql)> = v.iter().map(|(k, p)| (format!(":{k}"), p.lite())).collect();
            let refs: Vec<(&str, &dyn ToSql)> = values.iter().map(|(k, val)| (k.as_str(), val as &dyn ToSql)).collect();
            stmt.query(refs.as_slice())
        }
    }
    .map_err(|e| lite_error(it, e, span))?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(|e| lite_error(it, e, span))? {
        let mut f = Fields::new();
        for (i, (name, decl)) in cols.iter().enumerate() {
            let value = row.get_ref(i).map(|v| from_lite(v, decl)).unwrap_or(Value::Nil);
            f.insert(name.clone(), value);
        }
        out.push(Value::object(f));
    }
    Ok(out)
}

fn from_lite(v: ValueRef, decl: &str) -> Value {
    match v {
        ValueRef::Null => Value::Nil,
        ValueRef::Integer(n) if decl == "BOOLEAN" => Value::Bool(n != 0),
        ValueRef::Integer(n) => Value::Int(n),
        ValueRef::Real(f) => Value::Num(f),
        ValueRef::Text(t) => {
            let s = String::from_utf8_lossy(t).into_owned();
            if decl == "JSON" {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&s) {
                    return json::from_json(parsed);
                }
            }
            Value::string(s)
        }
        ValueRef::Blob(b) => Value::string(String::from_utf8_lossy(b).into_owned()),
    }
}

/// An open (or lazily opened) database.
pub struct Handle {
    /// `None` means the default database, resolved on first use.
    target: RefCell<Option<String>>,
    conn: RefCell<Option<Conn>>,
    /// Column names and declared types per table.
    columns: RefCell<HashMap<String, Vec<(String, String)>>>,
}

impl Handle {
    fn new(target: Option<String>) -> Rc<Handle> {
        Rc::new(Handle { target: RefCell::new(target), conn: RefCell::new(None), columns: RefCell::new(HashMap::new()) })
    }

    fn conn(&self, it: &Interpreter, span: Span) -> Result<RefMut<'_, Conn>, Flow> {
        if self.conn.borrow().is_none() {
            let target = self.target.borrow().clone().unwrap_or_else(|| {
                std::env::var("DATABASE_URL").unwrap_or_else(|_| it.project_root.join("lipi.db").to_string_lossy().into_owned())
            });
            let conn = if target.starts_with("postgres://") || target.starts_with("postgresql://") {
                let connector = native_tls::TlsConnector::new().map_err(|e| it.err("LIP5011", format!("couldn't set up TLS: {e}"), span, None))?;
                let tls = postgres_native_tls::MakeTlsConnector::new(connector);
                Conn::Pg(postgres::Client::connect(&target, tls).map_err(|e| pg_error(it, e, span))?)
            } else {
                let file = target.trim_start_matches("sqlite://").trim_start_matches("sqlite:").to_string();
                let c = Connection::open(&file).map_err(|e| lite_error(it, e, span))?;
                let _ = c.busy_timeout(std::time::Duration::from_secs(5));
                let _ = c.execute_batch("PRAGMA foreign_keys = ON;");
                Conn::Lite(c)
            };
            *self.target.borrow_mut() = Some(target);
            *self.conn.borrow_mut() = Some(conn);
        }
        Ok(RefMut::map(self.conn.borrow_mut(), |c| c.as_mut().expect("opened above")))
    }
}

fn valid_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn check_identifier(it: &Interpreter, name: &str, what: &str, span: Span) -> Result<(), Flow> {
    if valid_identifier(name) {
        return Ok(());
    }
    Err(it.err("LIP5008", format!("\"{name}\" isn't a valid {what} name"), span, Some("Use letters, digits and _ only, like order_items.".into())))
}

fn params_arg(it: &Interpreter, a: &Args, i: usize) -> Result<Params, Flow> {
    match a.get(i, "params") {
        None | Some(Value::Nil) => Ok(no_params()),
        Some(Value::List(l)) => Ok(Params::Positional(l.borrow().iter().map(|v| to_param(it, v, a.span)).collect::<Result<_, _>>()?)),
        Some(Value::Object(o)) => {
            Ok(Params::Named(o.fields.borrow().iter().map(|(k, v)| Ok((k.clone(), to_param(it, v, a.span)?))).collect::<Result<_, Flow>>()?))
        }
        Some(other) => Err(it.err(
            "LIP5008",
            format!("query parameters should be an Array or an Object, not {}", with_article(&other.type_name())),
            a.span,
            Some("For example: db.query(\"SELECT * FROM users WHERE id = ?\", [id])".into()),
        )),
    }
}

// ----- tables ---------------------------------------------------------------

fn column_type(v: &Value, pg: bool) -> &'static str {
    match (v, pg) {
        (Value::Int(_), false) => "INTEGER",
        (Value::Int(_), true) => "BIGINT",
        (Value::Num(_), false) => "REAL",
        (Value::Num(_), true) => "DOUBLE PRECISION",
        (Value::Str(_), _) => "TEXT",
        (Value::Bool(_), _) => "BOOLEAN",
        (Value::List(_) | Value::Object(_), false) => "JSON",
        (Value::List(_) | Value::Object(_), true) => "JSONB",
        (_, false) => "",
        (_, true) => "TEXT",
    }
}

fn table_columns(it: &Interpreter, h: &Handle, table: &str, span: Span) -> Result<Vec<(String, String)>, Flow> {
    if let Some(cols) = h.columns.borrow().get(table) {
        return Ok(cols.clone());
    }
    let rows = {
        let mut conn = h.conn(it, span)?;
        if conn.is_pg() {
            conn.query(
                it,
                "SELECT column_name AS name, data_type AS type FROM information_schema.columns WHERE table_schema = current_schema() AND table_name = $1 ORDER BY ordinal_position",
                &Params::Positional(vec![Param::Text(table.to_string())]),
                span,
            )?
        } else {
            conn.query(it, &format!("PRAGMA table_info(\"{table}\")"), &no_params(), span)?
        }
    };
    let cols: Vec<(String, String)> = rows
        .iter()
        .filter_map(|r| match r {
            Value::Object(o) => {
                let f = o.fields.borrow();
                Some((f.get("name")?.display(), f.get("type").map(|t| t.display().to_uppercase()).unwrap_or_default()))
            }
            _ => None,
        })
        .collect();
    if !cols.is_empty() {
        h.columns.borrow_mut().insert(table.to_string(), cols.clone());
    }
    Ok(cols)
}

/// Create the table, or add missing columns, so `values` can be stored.
fn ensure_columns(it: &Interpreter, h: &Handle, table: &str, values: &Fields, span: Span) -> Result<(), Flow> {
    let existing = table_columns(it, h, table, span)?;
    let mut conn = h.conn(it, span)?;
    let pg = conn.is_pg();
    if existing.is_empty() {
        let id = if pg { "\"id\" BIGSERIAL PRIMARY KEY" } else { "\"id\" INTEGER PRIMARY KEY AUTOINCREMENT" };
        let mut defs = vec![id.to_string()];
        for (k, v) in values {
            if k == "id" {
                continue;
            }
            check_identifier(it, k, "column", span)?;
            defs.push(format!("\"{k}\" {}", column_type(v, pg)));
        }
        conn.batch(it, &format!("CREATE TABLE IF NOT EXISTS \"{table}\" ({})", defs.join(", ")), span)?;
    } else {
        for (k, v) in values {
            if !existing.iter().any(|(name, _)| name == k) {
                check_identifier(it, k, "column", span)?;
                conn.batch(it, &format!("ALTER TABLE \"{table}\" ADD COLUMN \"{k}\" {}", column_type(v, pg)), span)?;
            }
        }
    }
    drop(conn);
    h.columns.borrow_mut().remove(table);
    Ok(())
}

struct Query {
    filters: Vec<(String, Value)>,
    order: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

/// Filters from named arguments (`where(active: true)`) or an Object, plus the
/// options `order`, `limit` and `offset`.
fn query_args(it: &Interpreter, a: &Args, first_filter_arg: usize) -> Result<Query, Flow> {
    let mut q = Query { filters: Vec::new(), order: None, limit: None, offset: None };
    if let Some(Value::Object(o)) = a.pos.get(first_filter_arg) {
        q.filters.extend(o.fields.borrow().iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    for (k, v) in &a.named {
        match k.as_str() {
            "order" => q.order = Some(v.display()),
            "limit" => q.limit = opt_int(it, a, usize::MAX, "limit")?,
            "offset" => q.offset = opt_int(it, a, usize::MAX, "offset")?,
            _ => q.filters.push((k.clone(), v.clone())),
        }
    }
    Ok(q)
}

fn build_select(it: &Interpreter, conn: &Conn, table: &str, q: &Query, what: &str, span: Span) -> Result<(String, Vec<Param>), Flow> {
    let mut sql = format!("SELECT {what} FROM \"{table}\"");
    let mut params = Vec::new();
    if !q.filters.is_empty() {
        let mut conds = Vec::new();
        for (k, v) in &q.filters {
            check_identifier(it, k, "column", span)?;
            if matches!(v, Value::Nil) {
                conds.push(format!("\"{k}\" IS NULL"));
            } else {
                params.push(to_param(it, v, span)?);
                conds.push(format!("\"{k}\" = {}", conn.placeholder(params.len())));
            }
        }
        sql.push_str(&format!(" WHERE {}", conds.join(" AND ")));
    }
    if let Some(order) = &q.order {
        let mut parts = Vec::new();
        for item in order.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let (col, dir) = match item.strip_prefix('-') {
                Some(c) => (c.trim(), "DESC"),
                None => match item.split_once(' ') {
                    Some((c, d)) if d.trim().eq_ignore_ascii_case("desc") => (c, "DESC"),
                    Some((c, _)) => (c, "ASC"),
                    None => (item, "ASC"),
                },
            };
            check_identifier(it, col, "column", span)?;
            parts.push(format!("\"{col}\" {dir}"));
        }
        if !parts.is_empty() {
            sql.push_str(&format!(" ORDER BY {}", parts.join(", ")));
        }
    }
    if q.limit.is_some() || q.offset.is_some() {
        let limit = match (q.limit, conn.is_pg()) {
            (Some(n), _) => Param::Int(n),
            (None, true) => Param::Null,
            (None, false) => Param::Int(-1),
        };
        params.push(limit);
        params.push(Param::Int(q.offset.unwrap_or(0)));
        sql.push_str(&format!(" LIMIT {} OFFSET {}", conn.placeholder(params.len() - 1), conn.placeholder(params.len())));
    }
    Ok((sql, params))
}

fn select(it: &Interpreter, h: &Handle, table: &str, q: &Query, span: Span) -> Result<Vec<Value>, Flow> {
    if table_columns(it, h, table, span)?.is_empty() {
        return Ok(Vec::new()); // the table doesn't exist yet: nothing stored
    }
    let mut conn = h.conn(it, span)?;
    let (sql, params) = build_select(it, &conn, table, q, "*", span)?;
    conn.query(it, &sql, &Params::Positional(params), span)
}

fn find_by_id(it: &Interpreter, h: &Handle, table: &str, id: Value, span: Span) -> Result<Value, Flow> {
    let q = Query { filters: vec![("id".into(), id)], order: None, limit: Some(1), offset: None };
    Ok(select(it, h, table, &q, span)?.into_iter().next().unwrap_or(Value::Nil))
}

fn object_arg(it: &Interpreter, a: &Args, i: usize, pname: &str) -> Result<Fields, Flow> {
    match need(it, a, i, pname)? {
        Value::Object(o) => Ok(o.fields.borrow().clone()),
        other => Err(it.err(
            "LIP5008",
            format!("{}() expects an Object, but got {}", a.name, with_article(&other.type_name())),
            a.span,
            Some(format!("For example: db.users.{}({{name: \"Dezy\"}})", a.name)),
        )),
    }
}

/// `db.<name>`: a table object, or `None` when `name` can't be a table.
pub fn table(o: &ObjectData, name: &str) -> Option<Value> {
    let handle = o.payload.clone()?.downcast::<Handle>().ok()?;
    if !valid_identifier(name) || MODULES.contains(&name) {
        return None;
    }
    Some(table_value(handle, name))
}

fn table_value(h: Rc<Handle>, table: &str) -> Value {
    let t = table.to_string();
    let mut f = Fields::new();
    f.insert("name".into(), Value::text(table));
    let (hh, tt) = (h.clone(), t.clone());
    f.insert(
        "all".into(),
        Value::native("all", move |it, a| {
            let q = query_args(it, a, usize::MAX)?;
            Ok(Value::list(select(it, &hh, &tt, &q, a.span)?))
        }),
    );
    let (hh, tt) = (h.clone(), t.clone());
    f.insert(
        "where".into(),
        Value::native("where", move |it, a| {
            let q = query_args(it, a, 0)?;
            Ok(Value::list(select(it, &hh, &tt, &q, a.span)?))
        }),
    );
    let (hh, tt) = (h.clone(), t.clone());
    f.insert(
        "find".into(),
        Value::native("find", move |it, a| {
            let mut q = query_args(it, a, 0)?;
            if let Some(id) = a.pos.first().filter(|v| !matches!(v, Value::Object(_))) {
                q.filters.push(("id".into(), id.clone()));
            }
            if q.filters.is_empty() {
                return Err(it.err("LIP5008", "find() needs an id or a filter", a.span, Some("For example: db.users.find(3) or db.users.find(email: email)".into())));
            }
            q.limit = Some(1);
            Ok(select(it, &hh, &tt, &q, a.span)?.into_iter().next().unwrap_or(Value::Nil))
        }),
    );
    let (hh, tt) = (h.clone(), t.clone());
    f.insert(
        "count".into(),
        Value::native("count", move |it, a| {
            if table_columns(it, &hh, &tt, a.span)?.is_empty() {
                return Ok(Value::Int(0));
            }
            let q = query_args(it, a, 0)?;
            let mut conn = hh.conn(it, a.span)?;
            let (sql, params) = build_select(it, &conn, &tt, &Query { order: None, limit: None, offset: None, ..q }, "COUNT(*) AS n", a.span)?;
            let rows = conn.query(it, &sql, &Params::Positional(params), a.span)?;
            Ok(match rows.first() {
                Some(Value::Object(o)) => o.fields.borrow().get("n").cloned().unwrap_or(Value::Int(0)),
                _ => Value::Int(0),
            })
        }),
    );
    let (hh, tt) = (h.clone(), t.clone());
    f.insert(
        "create".into(),
        Value::native("create", move |it, a| {
            check_identifier(it, &tt, "table", a.span)?;
            let values = object_arg(it, a, 0, "row")?;
            ensure_columns(it, &hh, &tt, &values, a.span)?;
            let id = {
                let mut conn = hh.conn(it, a.span)?;
                if values.is_empty() {
                    conn.insert(it, &format!("INSERT INTO \"{tt}\" DEFAULT VALUES"), &no_params(), a.span)?
                } else {
                    let cols: Vec<String> = values.keys().map(|k| format!("\"{k}\"")).collect();
                    let marks: Vec<String> = (1..=values.len()).map(|i| conn.placeholder(i)).collect();
                    let params = values.values().map(|v| to_param(it, v, a.span)).collect::<Result<Vec<_>, _>>()?;
                    let sql = format!("INSERT INTO \"{tt}\" ({}) VALUES ({})", cols.join(", "), marks.join(", "));
                    conn.insert(it, &sql, &Params::Positional(params), a.span)?
                }
            };
            find_by_id(it, &hh, &tt, Value::Int(id), a.span)
        }),
    );
    let (hh, tt) = (h.clone(), t.clone());
    f.insert(
        "update".into(),
        Value::native("update", move |it, a| {
            let id = need(it, a, 0, "id")?.clone();
            let changes = object_arg(it, a, 1, "changes")?;
            if changes.is_empty() {
                return find_by_id(it, &hh, &tt, id, a.span);
            }
            ensure_columns(it, &hh, &tt, &changes, a.span)?;
            {
                let mut conn = hh.conn(it, a.span)?;
                let sets: Vec<String> = changes.keys().enumerate().map(|(i, k)| format!("\"{k}\" = {}", conn.placeholder(i + 1))).collect();
                let mut params = changes.values().map(|v| to_param(it, v, a.span)).collect::<Result<Vec<_>, _>>()?;
                params.push(to_param(it, &id, a.span)?);
                let sql = format!("UPDATE \"{tt}\" SET {} WHERE \"id\" = {}", sets.join(", "), conn.placeholder(params.len()));
                conn.execute(it, &sql, &Params::Positional(params), a.span)?;
            }
            find_by_id(it, &hh, &tt, id, a.span)
        }),
    );
    let (hh, tt) = (h, t);
    f.insert(
        "delete".into(),
        Value::native("delete", move |it, a| {
            let id = need(it, a, 0, "id")?.clone();
            if table_columns(it, &hh, &tt, a.span)?.is_empty() {
                return Ok(Value::Bool(false));
            }
            let mut conn = hh.conn(it, a.span)?;
            let sql = format!("DELETE FROM \"{tt}\" WHERE \"id\" = {}", conn.placeholder(1));
            let n = conn.execute(it, &sql, &Params::Positional(vec![to_param(it, &id, a.span)?]), a.span)?;
            Ok(Value::Bool(n > 0))
        }),
    );
    Value::Object(Rc::new(ObjectData { fields: RefCell::new(f), ty: None, module: None, tag: Some("table"), payload: None }))
}

// ----- database objects ---------------------------------------------------------

fn db_entries(h: &Rc<Handle>) -> Fields {
    let mut f = Fields::new();
    let hh = h.clone();
    f.insert(
        "query".into(),
        Value::native("query", move |it, a| {
            let sql = text(it, a, 0, "sql")?;
            let params = params_arg(it, a, 1)?;
            let mut conn = hh.conn(it, a.span)?;
            Ok(Value::list(conn.query(it, &sql, &params, a.span)?))
        }),
    );
    let hh = h.clone();
    f.insert(
        "run".into(),
        Value::native("run", move |it, a| {
            let sql = text(it, a, 0, "sql")?;
            let params = params_arg(it, a, 1)?;
            let (changes, last_id) = {
                let mut conn = hh.conn(it, a.span)?;
                let n = conn.execute(it, &sql, &params, a.span)?;
                (n, conn.last_id())
            };
            hh.columns.borrow_mut().clear();
            let mut out = Fields::new();
            out.insert("changes".into(), Value::Int(changes as i64));
            out.insert("lastId".into(), Value::Int(last_id));
            Ok(Value::object(out))
        }),
    );
    let hh = h.clone();
    f.insert(
        "transaction".into(),
        Value::native("transaction", move |it, a| {
            let block = callable(it, a, 0, "function")?;
            hh.conn(it, a.span)?.batch(it, "BEGIN", a.span)?;
            match it.call_callback(&block, vec![db_value(hh.clone(), None)], a.span) {
                Ok(v) => {
                    hh.conn(it, a.span)?.batch(it, "COMMIT", a.span)?;
                    Ok(v)
                }
                Err(e) => {
                    if let Ok(mut conn) = hh.conn(it, a.span) {
                        let _ = conn.batch(it, "ROLLBACK", a.span);
                    }
                    hh.columns.borrow_mut().clear();
                    Err(e)
                }
            }
        }),
    );
    let hh = h.clone();
    f.insert(
        "migrate".into(),
        Value::native("migrate", move |it, a| {
            let name = text(it, a, 0, "name")?;
            let sql = text(it, a, 1, "sql")?;
            let mut conn = hh.conn(it, a.span)?;
            conn.batch(it, "CREATE TABLE IF NOT EXISTS lipi_migrations (name TEXT PRIMARY KEY, applied_at TEXT DEFAULT CURRENT_TIMESTAMP)", a.span)?;
            let check = format!("SELECT name FROM lipi_migrations WHERE name = {}", conn.placeholder(1));
            if !conn.query(it, &check, &Params::Positional(vec![Param::Text(name.to_string())]), a.span)?.is_empty() {
                return Ok(Value::Bool(false));
            }
            let batch = format!("BEGIN; {sql}; INSERT INTO lipi_migrations (name) VALUES ('{}'); COMMIT;", name.replace('\'', "''"));
            if let Err(e) = conn.batch(it, &batch, a.span) {
                let _ = conn.batch(it, "ROLLBACK", a.span);
                return Err(e);
            }
            drop(conn);
            hh.columns.borrow_mut().clear();
            Ok(Value::Bool(true))
        }),
    );
    let hh = h.clone();
    f.insert(
        "tables".into(),
        Value::native("tables", move |it, a| {
            let mut conn = hh.conn(it, a.span)?;
            let sql = if conn.is_pg() {
                "SELECT table_name AS name FROM information_schema.tables WHERE table_schema = current_schema() AND table_name <> 'lipi_migrations' ORDER BY table_name"
            } else {
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name != 'lipi_migrations' ORDER BY name"
            };
            let rows = conn.query(it, sql, &no_params(), a.span)?;
            Ok(Value::list(
                rows.into_iter()
                    .filter_map(|r| match r {
                        Value::Object(o) => o.fields.borrow().get("name").cloned(),
                        _ => None,
                    })
                    .collect(),
            ))
        }),
    );
    let hh = h.clone();
    f.insert(
        "close".into(),
        Value::native("close", move |_, _| {
            *hh.conn.borrow_mut() = None;
            hh.columns.borrow_mut().clear();
            Ok(Value::Nil)
        }),
    );
    f
}

fn db_value(h: Rc<Handle>, module: Option<String>) -> Value {
    let fields = db_entries(&h);
    Value::Object(Rc::new(ObjectData { fields: RefCell::new(fields), ty: None, module, tag: Some("db"), payload: Some(h) }))
}

/// The global `database` module: `database.open(target)`, and the default
/// database (`DATABASE_URL`, else `lipi.db` in the project) through `database.<table>`.
pub fn module() -> Value {
    let value = db_value(Handle::new(None), Some("database".into()));
    if let Value::Object(o) = &value {
        o.fields.borrow_mut().insert(
            "open".into(),
            Value::native("open", |it, a| {
                let target = text(it, a, 0, "path")?;
                Ok(db_value(Handle::new(Some(target.to_string())), None))
            }),
        );
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_round_trip() {
        for s in ["0", "1", "-12.5", "1234.5678", "0.0001", "100000000", "19.99"] {
            let mut buf = BytesMut::new();
            encode_numeric(s, &mut buf).unwrap();
            let decoded = match decode_numeric(&buf).unwrap() {
                Cell::Int(n) => n.to_string(),
                Cell::Float(f) => f.to_string(),
                _ => panic!(),
            };
            assert_eq!(decoded.parse::<f64>().unwrap(), s.parse::<f64>().unwrap(), "{s}");
        }
    }

    #[test]
    fn timestamps() {
        assert_eq!(parse_timestamp("2000-01-01T00:00:00Z").unwrap(), EPOCH_2000_SECS * 1_000_000);
        assert_eq!(format_timestamp(parse_timestamp("2026-09-11T10:30:05.250Z").unwrap(), true), "2026-09-11T10:30:05.250Z");
        assert_eq!(parse_timestamp("2026-09-11T16:00:00+05:30").unwrap(), parse_timestamp("2026-09-11T10:30:00Z").unwrap());
    }
}
