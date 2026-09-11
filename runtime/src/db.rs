//! The database layer (SQLite; PostgreSQL is next).
//!
//! ```lipi
//! db = database.open("shop.db")          # or just use `database.users...` (lipi.db)
//! user = db.users.create({name: "Dezy", age: 25})
//! adults = db.users.where(active: true, order: "name")
//! db.orders.update(order.id, {status: "paid"})
//! rows = db.query("SELECT * FROM users WHERE age > ?", [18])
//! ```
//!
//! - Tables are created on the first `create`, and new fields add columns.
//! - Values are always sent as parameters, never pasted into SQL.
//! - Table and column names must be plain identifiers.

use crate::builtins::{callable, need, opt_int, text, MODULES};
use crate::interp::{Flow, Interpreter};
use crate::json;
use crate::value::*;
use lipi_compiler::checker::with_article;
use lipi_compiler::Span;
use rusqlite::types::{Value as Sql, ValueRef};
use rusqlite::{params_from_iter, Connection, ToSql};
use std::cell::{RefCell, RefMut};
use std::collections::HashMap;
use std::rc::Rc;

/// An open (or lazily opened) database.
pub struct Handle {
    /// `None` means the default database, resolved on first use.
    path: RefCell<Option<String>>,
    conn: RefCell<Option<Connection>>,
    /// Column names and declared types per table.
    columns: RefCell<HashMap<String, Vec<(String, String)>>>,
}

impl Handle {
    fn new(path: Option<String>) -> Rc<Handle> {
        Rc::new(Handle { path: RefCell::new(path), conn: RefCell::new(None), columns: RefCell::new(HashMap::new()) })
    }

    fn conn(&self, it: &Interpreter, span: Span) -> Result<RefMut<'_, Connection>, Flow> {
        if self.conn.borrow().is_none() {
            let target = self.path.borrow().clone().unwrap_or_else(|| {
                std::env::var("DATABASE_URL").unwrap_or_else(|_| it.project_root.join("lipi.db").to_string_lossy().into_owned())
            });
            if target.starts_with("postgres://") || target.starts_with("postgresql://") {
                return Err(it.err(
                    "LIP5011",
                    "PostgreSQL support is coming in the next LiPi update",
                    span,
                    Some("Use SQLite for now, for example: database.open(\"app.db\")".into()),
                ));
            }
            let file = target.trim_start_matches("sqlite://").trim_start_matches("sqlite:").to_string();
            let conn = Connection::open(&file).map_err(|e| db_error(it, e, span))?;
            let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
            let _ = conn.execute_batch("PRAGMA foreign_keys = ON;");
            *self.path.borrow_mut() = Some(file);
            *self.conn.borrow_mut() = Some(conn);
        }
        Ok(RefMut::map(self.conn.borrow_mut(), |c| c.as_mut().expect("opened above")))
    }
}

fn db_error(it: &Interpreter, e: rusqlite::Error, span: Span) -> Flow {
    let message = e.to_string();
    let hint = if message.contains("no such table") {
        Some("Create the table first: insert a row with db.<table>.create({...}) or run a migration.")
    } else if message.contains("no such column") {
        Some("Check the column name. New columns are added automatically by create() and update().")
    } else if message.contains("UNIQUE constraint failed") {
        Some("A row with this value already exists.")
    } else if message.contains("syntax error") {
        Some("Check the SQL. Values go in as ? placeholders: db.query(\"SELECT * FROM users WHERE id = ?\", [id])")
    } else {
        None
    };
    it.err("LIP5011", format!("database error: {message}"), span, hint.map(String::from))
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

// ----- value conversion ---------------------------------------------------------

fn to_sql(it: &Interpreter, v: &Value, span: Span) -> Result<Sql, Flow> {
    Ok(match v {
        Value::Nil => Sql::Null,
        Value::Bool(b) => Sql::Integer(*b as i64),
        Value::Int(n) => Sql::Integer(*n),
        Value::Num(n) => Sql::Real(*n),
        Value::Str(s) => Sql::Text(s.to_string()),
        Value::List(_) | Value::Object(_) => Sql::Text(json::stringify(it, v, false, span)?),
        other => return Err(it.err("LIP5008", format!("{} can't be stored in a database", with_article(&other.type_name())), span, None)),
    })
}

fn column_type(v: &Value) -> &'static str {
    match v {
        Value::Int(_) => "INTEGER",
        Value::Num(_) => "REAL",
        Value::Str(_) => "TEXT",
        Value::Bool(_) => "BOOLEAN",
        Value::List(_) | Value::Object(_) => "JSON",
        _ => "",
    }
}

fn from_sql(v: ValueRef, decl: &str) -> Value {
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

enum Params {
    Positional(Vec<Sql>),
    Named(Vec<(String, Sql)>),
}

fn params_arg(it: &Interpreter, a: &Args, i: usize) -> Result<Params, Flow> {
    match a.get(i, "params") {
        None | Some(Value::Nil) => Ok(Params::Positional(Vec::new())),
        Some(Value::List(l)) => Ok(Params::Positional(l.borrow().iter().map(|v| to_sql(it, v, a.span)).collect::<Result<_, _>>()?)),
        Some(Value::Object(o)) => Ok(Params::Named(
            o.fields.borrow().iter().map(|(k, v)| Ok((format!(":{k}"), to_sql(it, v, a.span)?))).collect::<Result<_, Flow>>()?,
        )),
        Some(other) => Err(it.err(
            "LIP5008",
            format!("query parameters should be an Array or an Object, not {}", with_article(&other.type_name())),
            a.span,
            Some("For example: db.query(\"SELECT * FROM users WHERE id = ?\", [id])".into()),
        )),
    }
}

fn query_rows(it: &Interpreter, conn: &Connection, sql: &str, params: &Params, span: Span) -> Result<Vec<Value>, Flow> {
    let mut stmt = conn.prepare(sql).map_err(|e| db_error(it, e, span))?;
    let cols: Vec<(String, String)> = stmt.columns().iter().map(|c| (c.name().to_string(), c.decl_type().unwrap_or("").to_uppercase())).collect();
    let mut rows = match params {
        Params::Positional(v) => stmt.query(params_from_iter(v.iter())),
        Params::Named(v) => {
            let refs: Vec<(&str, &dyn ToSql)> = v.iter().map(|(k, val)| (k.as_str(), val as &dyn ToSql)).collect();
            stmt.query(refs.as_slice())
        }
    }
    .map_err(|e| db_error(it, e, span))?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(|e| db_error(it, e, span))? {
        let mut f = Fields::new();
        for (i, (name, decl)) in cols.iter().enumerate() {
            let value = row.get_ref(i).map(|v| from_sql(v, decl)).unwrap_or(Value::Nil);
            f.insert(name.clone(), value);
        }
        out.push(Value::object(f));
    }
    Ok(out)
}

fn execute(it: &Interpreter, conn: &Connection, sql: &str, params: &Params, span: Span) -> Result<usize, Flow> {
    match params {
        Params::Positional(v) => conn.execute(sql, params_from_iter(v.iter())),
        Params::Named(v) => {
            let refs: Vec<(&str, &dyn ToSql)> = v.iter().map(|(k, val)| (k.as_str(), val as &dyn ToSql)).collect();
            conn.execute(sql, refs.as_slice())
        }
    }
    .map_err(|e| db_error(it, e, span))
}

// ----- tables ---------------------------------------------------------------

fn table_columns(it: &Interpreter, h: &Handle, table: &str, span: Span) -> Result<Vec<(String, String)>, Flow> {
    if let Some(cols) = h.columns.borrow().get(table) {
        return Ok(cols.clone());
    }
    let cols: Vec<(String, String)> = {
        let conn = h.conn(it, span)?;
        let rows = query_rows(it, &conn, &format!("PRAGMA table_info(\"{table}\")"), &Params::Positional(Vec::new()), span)?;
        rows.iter()
            .filter_map(|r| match r {
                Value::Object(o) => {
                    let f = o.fields.borrow();
                    Some((f.get("name")?.display(), f.get("type").map(|t| t.display().to_uppercase()).unwrap_or_default()))
                }
                _ => None,
            })
            .collect()
    };
    if !cols.is_empty() {
        h.columns.borrow_mut().insert(table.to_string(), cols.clone());
    }
    Ok(cols)
}

/// Create the table, or add missing columns, so `values` can be stored.
fn ensure_columns(it: &Interpreter, h: &Handle, table: &str, values: &Fields, span: Span) -> Result<(), Flow> {
    let existing = table_columns(it, h, table, span)?;
    let conn = h.conn(it, span)?;
    let none = Params::Positional(Vec::new());
    if existing.is_empty() {
        let mut defs = vec!["\"id\" INTEGER PRIMARY KEY AUTOINCREMENT".to_string()];
        for (k, v) in values {
            if k == "id" {
                continue;
            }
            check_identifier(it, k, "column", span)?;
            defs.push(format!("\"{k}\" {}", column_type(v)));
        }
        execute(it, &conn, &format!("CREATE TABLE IF NOT EXISTS \"{table}\" ({})", defs.join(", ")), &none, span)?;
    } else {
        for (k, v) in values {
            if !existing.iter().any(|(name, _)| name == k) {
                check_identifier(it, k, "column", span)?;
                execute(it, &conn, &format!("ALTER TABLE \"{table}\" ADD COLUMN \"{k}\" {}", column_type(v)), &none, span)?;
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

fn build_select(it: &Interpreter, table: &str, q: &Query, what: &str, span: Span) -> Result<(String, Vec<Sql>), Flow> {
    let mut sql = format!("SELECT {what} FROM \"{table}\"");
    let mut params = Vec::new();
    if !q.filters.is_empty() {
        let mut conds = Vec::new();
        for (k, v) in &q.filters {
            check_identifier(it, k, "column", span)?;
            if matches!(v, Value::Nil) {
                conds.push(format!("\"{k}\" IS NULL"));
            } else {
                conds.push(format!("\"{k}\" = ?"));
                params.push(to_sql(it, v, span)?);
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
        sql.push_str(" LIMIT ? OFFSET ?");
        params.push(Sql::Integer(q.limit.unwrap_or(-1)));
        params.push(Sql::Integer(q.offset.unwrap_or(0)));
    }
    Ok((sql, params))
}

fn select(it: &Interpreter, h: &Handle, table: &str, q: &Query, span: Span) -> Result<Vec<Value>, Flow> {
    if table_columns(it, h, table, span)?.is_empty() {
        return Ok(Vec::new()); // the table doesn't exist yet: nothing stored
    }
    let (sql, params) = build_select(it, table, q, "*", span)?;
    let conn = h.conn(it, span)?;
    query_rows(it, &conn, &sql, &Params::Positional(params), span)
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
            let (sql, params) = build_select(it, &tt, &Query { order: None, limit: None, offset: None, ..q }, "COUNT(*) AS n", a.span)?;
            let conn = hh.conn(it, a.span)?;
            let rows = query_rows(it, &conn, &sql, &Params::Positional(params), a.span)?;
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
                let conn = hh.conn(it, a.span)?;
                if values.is_empty() {
                    execute(it, &conn, &format!("INSERT INTO \"{tt}\" DEFAULT VALUES"), &Params::Positional(Vec::new()), a.span)?;
                } else {
                    let cols: Vec<String> = values.keys().map(|k| format!("\"{k}\"")).collect();
                    let marks = vec!["?"; values.len()].join(", ");
                    let params = values.values().map(|v| to_sql(it, v, a.span)).collect::<Result<Vec<_>, _>>()?;
                    execute(it, &conn, &format!("INSERT INTO \"{tt}\" ({}) VALUES ({marks})", cols.join(", ")), &Params::Positional(params), a.span)?;
                }
                conn.last_insert_rowid()
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
                let conn = hh.conn(it, a.span)?;
                let sets: Vec<String> = changes.keys().map(|k| format!("\"{k}\" = ?")).collect();
                let mut params = changes.values().map(|v| to_sql(it, v, a.span)).collect::<Result<Vec<_>, _>>()?;
                params.push(to_sql(it, &id, a.span)?);
                execute(it, &conn, &format!("UPDATE \"{tt}\" SET {} WHERE \"id\" = ?", sets.join(", ")), &Params::Positional(params), a.span)?;
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
            let conn = hh.conn(it, a.span)?;
            let n = execute(it, &conn, &format!("DELETE FROM \"{tt}\" WHERE \"id\" = ?"), &Params::Positional(vec![to_sql(it, &id, a.span)?]), a.span)?;
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
            let conn = hh.conn(it, a.span)?;
            Ok(Value::list(query_rows(it, &conn, &sql, &params, a.span)?))
        }),
    );
    let hh = h.clone();
    f.insert(
        "run".into(),
        Value::native("run", move |it, a| {
            let sql = text(it, a, 0, "sql")?;
            let params = params_arg(it, a, 1)?;
            let (changes, last_id) = {
                let conn = hh.conn(it, a.span)?;
                let n = execute(it, &conn, &sql, &params, a.span)?;
                (n, conn.last_insert_rowid())
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
            let none = Params::Positional(Vec::new());
            execute(it, &*hh.conn(it, a.span)?, "BEGIN", &none, a.span)?;
            match it.call_callback(&block, vec![db_value(hh.clone(), None)], a.span) {
                Ok(v) => {
                    execute(it, &*hh.conn(it, a.span)?, "COMMIT", &none, a.span)?;
                    Ok(v)
                }
                Err(e) => {
                    if let Ok(conn) = hh.conn(it, a.span) {
                        let _ = conn.execute_batch("ROLLBACK");
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
            let conn = hh.conn(it, a.span)?;
            let none = Params::Positional(Vec::new());
            execute(it, &conn, "CREATE TABLE IF NOT EXISTS lipi_migrations (name TEXT PRIMARY KEY, applied_at TEXT)", &none, a.span)?;
            let done = query_rows(it, &conn, "SELECT name FROM lipi_migrations WHERE name = ?", &Params::Positional(vec![Sql::Text(name.to_string())]), a.span)?;
            if !done.is_empty() {
                return Ok(Value::Bool(false));
            }
            let batch = format!(
                "BEGIN; {sql}; INSERT INTO lipi_migrations (name, applied_at) VALUES ('{}', datetime('now')); COMMIT;",
                name.replace('\'', "''")
            );
            if let Err(e) = conn.execute_batch(&batch) {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(db_error(it, e, a.span));
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
            let conn = hh.conn(it, a.span)?;
            let rows = query_rows(
                it,
                &conn,
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name != 'lipi_migrations' ORDER BY name",
                &Params::Positional(Vec::new()),
                a.span,
            )?;
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

/// The global `database` module: `database.open(path)`, and the default
/// database (`DATABASE_URL`, else `lipi.db` in the project) through `database.<table>`.
pub fn module() -> Value {
    let value = db_value(Handle::new(None), Some("database".into()));
    if let Value::Object(o) = &value {
        o.fields.borrow_mut().insert(
            "open".into(),
            Value::native("open", |it, a| {
                let path = text(it, a, 0, "path")?;
                Ok(db_value(Handle::new(Some(path.to_string())), None))
            }),
        );
    }
    value
}
