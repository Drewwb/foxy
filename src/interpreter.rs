use std::collections::HashMap;

use rusqlite::types::{Value as SqlValue, ValueRef};
use rusqlite::{Connection, OpenFlags, config::DbConfig};

use crate::ast::{BinOp, Expr, Stmt, UnaryOp};
use crate::value::Value;

type Scope = HashMap<String, Value>;
type ScopeStack = Vec<Scope>;

#[derive(Debug, Clone, PartialEq)]
struct FunctionDef {
    params: Vec<String>,
    body: Vec<Stmt>,
}

/// Unwinding signal threaded through statement/expression evaluation.
/// `Fatal` always halts the whole program; `Return`/`Propagate` are caught
/// at the nearest function-call boundary (or the top of the program, for
/// top-level code).
enum Signal {
    Return(Value),
    Propagate(Value),
    Fatal(String),
}

type SigResult<T> = Result<T, Signal>;

/// A whole table read from the database: column names, rows in column order,
/// and (when asked for) each row's SQLite rowid.
struct TableData {
    columns: Vec<String>,
    rows: Vec<Vec<Value>>,
    rowids: Vec<i64>,
}

pub struct Interpreter {
    functions: HashMap<String, FunctionDef>,
    /// The database opened by the most recent `open` statement, if any.
    db: Option<Connection>,
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

impl Interpreter {
    pub fn new() -> Self {
        Interpreter {
            functions: HashMap::new(),
            db: None,
        }
    }

    /// Runs a whole program in a fresh top-level scope: hoists `fn`
    /// declarations first (so call order doesn't matter), then executes the
    /// remaining top-level statements in order, as if they were the body of
    /// an implicit `main`. This is what `foxy run` uses.
    pub fn run_program(&mut self, program: &[Stmt]) -> Result<(), String> {
        let mut scopes: Vec<HashMap<String, Value>> = vec![HashMap::new()];
        self.run_in_scopes(program, &mut scopes)
    }

    /// Same as [`Self::run_program`], but against a scope stack the caller
    /// owns and can reuse across calls -- what the REPL uses to keep
    /// variables and functions alive between lines.
    pub fn run_in_scopes(
        &mut self,
        program: &[Stmt],
        scopes: &mut Vec<HashMap<String, Value>>,
    ) -> Result<(), String> {
        for stmt in program {
            if let Stmt::FnDecl {
                name, params, body, ..
            } = stmt
            {
                self.functions.insert(
                    name.clone(),
                    FunctionDef {
                        params: params.iter().map(|p| p.name.clone()).collect(),
                        body: body.clone(),
                    },
                );
            }
        }

        for stmt in program {
            if matches!(stmt, Stmt::FnDecl { .. }) {
                continue;
            }
            match self.exec_stmt(stmt, scopes) {
                Ok(()) => {}
                Err(Signal::Return(_)) => break,
                Err(Signal::Propagate(v)) => {
                    return Err(format!("unhandled error propagated to top level: {v}"));
                }
                Err(Signal::Fatal(msg)) => return Err(msg),
            }
        }
        Ok(())
    }

    /// Evaluates a single expression against a persistent scope stack and
    /// returns its value, for the REPL's "bare expression prints its value"
    /// behavior. `return`/`?` at this level (outside any function) are
    /// reported as errors rather than silently doing something surprising.
    pub fn eval_repl_expr(
        &mut self,
        expr: &Expr,
        scopes: &mut Vec<HashMap<String, Value>>,
    ) -> Result<Value, String> {
        match self.eval_expr(expr, scopes) {
            Ok(v) => Ok(v),
            Err(Signal::Fatal(msg)) => Err(msg),
            Err(Signal::Propagate(v)) => {
                Err(format!("unhandled error propagated to top level: {v}"))
            }
            Err(Signal::Return(_)) => Err("'return' is not valid outside a function".to_string()),
        }
    }

    fn exec_block(&mut self, stmts: &[Stmt], scopes: &mut ScopeStack) -> SigResult<()> {
        scopes.push(Scope::new());
        let result = self.exec_stmts(stmts, scopes);
        scopes.pop();
        result
    }

    fn exec_stmts(&mut self, stmts: &[Stmt], scopes: &mut ScopeStack) -> SigResult<()> {
        for stmt in stmts {
            self.exec_stmt(stmt, scopes)?;
        }
        Ok(())
    }

    fn exec_stmt(&mut self, stmt: &Stmt, scopes: &mut ScopeStack) -> SigResult<()> {
        match stmt {
            Stmt::VarDecl { name, value } => {
                let v = self.eval_expr(value, scopes)?;
                scopes
                    .last_mut()
                    .expect("scope stack is never empty")
                    .insert(name.clone(), v);
                Ok(())
            }
            Stmt::Assign { name, value } => {
                let v = self.eval_expr(value, scopes)?;
                for scope in scopes.iter_mut().rev() {
                    if let Some(slot) = scope.get_mut(name) {
                        *slot = v;
                        return Ok(());
                    }
                }
                Err(Signal::Fatal(format!(
                    "cannot assign to undefined variable '{name}'"
                )))
            }
            Stmt::FnDecl { .. } => Ok(()), // already hoisted
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond = self.eval_bool(condition, scopes)?;
                if cond {
                    self.exec_block(then_branch, scopes)
                } else if let Some(else_branch) = else_branch {
                    self.exec_block(else_branch, scopes)
                } else {
                    Ok(())
                }
            }
            Stmt::While { condition, body } => {
                while self.eval_bool(condition, scopes)? {
                    self.exec_block(body, scopes)?;
                }
                Ok(())
            }
            Stmt::For {
                var_name,
                iterable,
                body,
            } => {
                let items = match self.eval_expr(iterable, scopes)? {
                    Value::List(items) => items,
                    other => {
                        return Err(Signal::Fatal(format!(
                            "'for ... in' expects a list, got {other}"
                        )));
                    }
                };
                for item in items {
                    scopes.push(Scope::from([(var_name.clone(), item)]));
                    let result = self.exec_stmts(body, scopes);
                    scopes.pop();
                    result?;
                }
                Ok(())
            }
            Stmt::Return(expr) => {
                let v = match expr {
                    Some(e) => self.eval_expr(e, scopes)?,
                    None => Value::Null,
                };
                Err(Signal::Return(v))
            }
            Stmt::Import(_) => Ok(()), // module resolution is out of scope until Den (Milestone 4)
            Stmt::Insert { table, record } => self.exec_insert(table, record, scopes),
            Stmt::Update {
                table,
                assignments,
                where_clause,
            } => self.exec_update(table, assignments, where_clause, scopes),
            Stmt::Delete {
                table,
                where_clause,
            } => self.exec_delete(table, where_clause, scopes),
            Stmt::Open(path) => {
                // No CREATE flag: opening a path that doesn't exist is an
                // error, so a typo'd filename can't silently make an empty db.
                let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
                let conn = Connection::open_with_flags(path, flags)
                    .map_err(|e| Signal::Fatal(format!("cannot open database '{path}': {e}")))?;
                // SQLite's legacy "double-quoted string" mode would turn a
                // misspelled quoted column name into a text literal instead of
                // an error. Turn it off so typos fail loudly.
                conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DQS_DML, false)
                    .map_err(|e| Signal::Fatal(format!("cannot configure database: {e}")))?;
                self.db = Some(conn);
                Ok(())
            }
            Stmt::ExprStmt(expr) => {
                self.eval_expr(expr, scopes)?;
                Ok(())
            }
        }
    }

    fn eval_bool(&mut self, expr: &Expr, scopes: &mut ScopeStack) -> SigResult<bool> {
        match self.eval_expr(expr, scopes)? {
            Value::Bool(b) => Ok(b),
            other => Err(Signal::Fatal(format!(
                "condition must be a bool, got {other}"
            ))),
        }
    }

    fn eval_expr(&mut self, expr: &Expr, scopes: &mut ScopeStack) -> SigResult<Value> {
        match expr {
            Expr::Int(n) => Ok(Value::Int(*n)),
            Expr::Float(n) => Ok(Value::Float(*n)),
            Expr::Str(s) => Ok(Value::Str(s.clone())),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Null => Ok(Value::Null),
            Expr::Ident(name) => {
                for scope in scopes.iter().rev() {
                    if let Some(v) = scope.get(name) {
                        return Ok(v.clone());
                    }
                }
                Err(Signal::Fatal(format!("undefined variable '{name}'")))
            }
            Expr::List(elements) => {
                let mut values = Vec::with_capacity(elements.len());
                for e in elements {
                    values.push(self.eval_expr(e, scopes)?);
                }
                Ok(Value::List(values))
            }
            Expr::Record(fields) => {
                let mut values = Vec::with_capacity(fields.len());
                for (name, e) in fields {
                    values.push((name.clone(), self.eval_expr(e, scopes)?));
                }
                Ok(Value::Record(values))
            }
            Expr::Unary { op, expr } => {
                let v = self.eval_expr(expr, scopes)?;
                match (op, v) {
                    (UnaryOp::Neg, Value::Int(n)) => Ok(Value::Int(-n)),
                    (UnaryOp::Neg, Value::Float(n)) => Ok(Value::Float(-n)),
                    (UnaryOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
                    (op, other) => Err(Signal::Fatal(format!(
                        "unary {op:?} is not defined for {other}"
                    ))),
                }
            }
            Expr::Binary { op, left, right } => {
                let l = self.eval_expr(left, scopes)?;
                let r = self.eval_expr(right, scopes)?;
                eval_binary(*op, l, r).map_err(Signal::Fatal)
            }
            Expr::Call { callee, args } => self.eval_call(callee, args, scopes),
            Expr::FieldAccess { target, field } => match self.eval_expr(target, scopes)? {
                Value::Record(fields) => fields
                    .into_iter()
                    .find(|(name, _)| name == field)
                    .map(|(_, v)| v)
                    .ok_or_else(|| Signal::Fatal(format!("record has no field '{field}'"))),
                other => Err(Signal::Fatal(format!(
                    "cannot access field '{field}' on {other}"
                ))),
            },
            Expr::Try(inner) => match self.eval_expr(inner, scopes)? {
                Value::Result(Ok(v)) => Ok(*v),
                Value::Result(Err(v)) => Err(Signal::Propagate(*v)),
                other => Err(Signal::Fatal(format!(
                    "'?' can only be used on a Result value, got {other}"
                ))),
            },
            Expr::Ok(inner) => Ok(Value::Result(Ok(Box::new(self.eval_expr(inner, scopes)?)))),
            Expr::Err(inner) => Ok(Value::Result(Err(Box::new(self.eval_expr(inner, scopes)?)))),
            Expr::Select {
                columns,
                table,
                where_clause,
            } => self.eval_select(columns, table, where_clause.as_deref(), scopes),
        }
    }

    /// The open database, or a clear error if `open` hasn't run yet.
    fn db(&self) -> SigResult<&Connection> {
        self.db.as_ref().ok_or_else(|| {
            Signal::Fatal("no database is open; use `open \"file.db\"` first".to_string())
        })
    }

    /// Reads every row of `table` from the open database, as Foxy values in
    /// column order. With `with_rowids`, also returns each row's SQLite rowid,
    /// which `update` and `delete` use to target exactly the matching rows.
    fn fetch_table(&self, table: &str, with_rowids: bool) -> SigResult<TableData> {
        let db = self.db()?;
        let sql = if with_rowids {
            format!("SELECT rowid, * FROM {}", quote_ident(table))
        } else {
            format!("SELECT * FROM {}", quote_ident(table))
        };
        let sql_err = |e: rusqlite::Error| table_error(table, e);

        let mut stmt = db.prepare(&sql).map_err(sql_err)?;
        let skip = usize::from(with_rowids);
        let columns: Vec<String> = stmt
            .column_names()
            .into_iter()
            .skip(skip)
            .map(String::from)
            .collect();
        let mut result = stmt.query([]).map_err(sql_err)?;
        let mut rows = Vec::new();
        let mut rowids = Vec::new();
        while let Some(row) = result.next().map_err(sql_err)? {
            if with_rowids {
                rowids.push(row.get::<_, i64>(0).map_err(sql_err)?);
            }
            let mut values = Vec::with_capacity(columns.len());
            for (i, name) in columns.iter().enumerate() {
                values.push(match row.get_ref(i + skip).map_err(sql_err)? {
                    ValueRef::Null => Value::Null,
                    ValueRef::Integer(n) => Value::Int(n),
                    ValueRef::Real(f) => Value::Float(f),
                    ValueRef::Text(bytes) => {
                        Value::Str(String::from_utf8_lossy(bytes).into_owned())
                    }
                    ValueRef::Blob(_) => {
                        return Err(Signal::Fatal(format!(
                            "column '{name}' holds binary data, which Foxy can't represent yet"
                        )));
                    }
                });
            }
            rows.push(values);
        }
        Ok(TableData {
            columns,
            rows,
            rowids,
        })
    }

    /// `insert into table record`: the record's fields become the columns.
    fn exec_insert(
        &mut self,
        table: &str,
        record: &Expr,
        scopes: &mut ScopeStack,
    ) -> SigResult<()> {
        let fields = match self.eval_expr(record, scopes)? {
            Value::Record(fields) => fields,
            other => {
                return Err(Signal::Fatal(format!(
                    "insert expects a record like {{ name: \"Ann\" }}, got {other}"
                )));
            }
        };
        let table_columns = self.fetch_table(table, false)?.columns;
        let mut names = Vec::with_capacity(fields.len());
        let mut params = Vec::with_capacity(fields.len());
        for (field, value) in &fields {
            names.push(quote_ident(&resolve_column(table, &table_columns, field)?));
            params.push(to_sql(field, value)?);
        }

        let sql = if names.is_empty() {
            format!("INSERT INTO {} DEFAULT VALUES", quote_ident(table))
        } else {
            let placeholders = vec!["?"; names.len()].join(", ");
            format!(
                "INSERT INTO {} ({}) VALUES ({placeholders})",
                quote_ident(table),
                names.join(", ")
            )
        };
        self.db()?
            .execute(&sql, rusqlite::params_from_iter(params))
            .map_err(|e| Signal::Fatal(format!("insert failed: {e}")))?;
        Ok(())
    }

    /// `update table set ... where ...`. Every row's `where` (and, for
    /// matching rows, every new value) is evaluated first; then all changes are
    /// written in one transaction, so an error leaves the table untouched.
    fn exec_update(
        &mut self,
        table: &str,
        assignments: &[(String, Expr)],
        where_clause: &Expr,
        scopes: &mut ScopeStack,
    ) -> SigResult<()> {
        let data = self.fetch_table(table, true)?;
        let mut targets = Vec::with_capacity(assignments.len());
        for (column, _) in assignments {
            targets.push(quote_ident(&resolve_column(table, &data.columns, column)?));
        }

        let mut changes = Vec::new();
        for (row, rowid) in data.rows.into_iter().zip(data.rowids) {
            scopes.push(data.columns.iter().cloned().zip(row).collect());
            let outcome = self.eval_update_row(assignments, where_clause, scopes);
            scopes.pop();
            if let Some(new_values) = outcome? {
                changes.push((rowid, new_values));
            }
        }

        let set_list = targets
            .iter()
            .map(|t| format!("{t} = ?"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "UPDATE {} SET {set_list} WHERE rowid = ?",
            quote_ident(table)
        );
        self.write_rows(&sql, changes, "update")
    }

    /// For one row (already in scope): `None` if `where` is false, otherwise
    /// the new value for each assignment, converted for SQLite.
    fn eval_update_row(
        &mut self,
        assignments: &[(String, Expr)],
        where_clause: &Expr,
        scopes: &mut ScopeStack,
    ) -> SigResult<Option<Vec<SqlValue>>> {
        if !self.eval_bool(where_clause, scopes)? {
            return Ok(None);
        }
        let mut values = Vec::with_capacity(assignments.len());
        for (column, expr) in assignments {
            values.push(to_sql(column, &self.eval_expr(expr, scopes)?)?);
        }
        Ok(Some(values))
    }

    /// `delete from table where ...`, evaluated and applied like `update`.
    fn exec_delete(
        &mut self,
        table: &str,
        where_clause: &Expr,
        scopes: &mut ScopeStack,
    ) -> SigResult<()> {
        let data = self.fetch_table(table, true)?;
        let mut doomed = Vec::new();
        for (row, rowid) in data.rows.into_iter().zip(data.rowids) {
            scopes.push(data.columns.iter().cloned().zip(row).collect());
            let keep = self.eval_bool(where_clause, scopes);
            scopes.pop();
            if keep? {
                doomed.push((rowid, Vec::new()));
            }
        }
        let sql = format!("DELETE FROM {} WHERE rowid = ?", quote_ident(table));
        self.write_rows(&sql, doomed, "delete")
    }

    /// Runs `sql` once per (rowid, values) pair inside one transaction. The
    /// values bind to the leading `?`s and the rowid to the last one.
    fn write_rows(&self, sql: &str, rows: Vec<(i64, Vec<SqlValue>)>, what: &str) -> SigResult<()> {
        let fail = |e: rusqlite::Error| Signal::Fatal(format!("{what} failed: {e}"));
        let tx = self.db()?.unchecked_transaction().map_err(fail)?;
        {
            let mut stmt = tx.prepare(sql).map_err(fail)?;
            for (rowid, mut values) in rows {
                values.push(SqlValue::Integer(rowid));
                stmt.execute(rusqlite::params_from_iter(values))
                    .map_err(fail)?;
            }
        }
        tx.commit().map_err(fail)
    }

    /// Evaluates a `select`: reads the source, keeps the rows where the
    /// `where` expression is true, and returns the chosen columns of each
    /// kept row as a record, fields in the order they were selected.
    ///
    /// The source after `from` is a variable holding a list of records if one
    /// with that name is in scope (a "cursor", so query results can be queried
    /// again), otherwise a table in the open database.
    ///
    /// Inside `where`, every column of the current row is in scope by name,
    /// including columns that aren't selected. Columns shadow variables of
    /// the same name, like in SQL.
    fn eval_select(
        &mut self,
        columns: &[String],
        table: &str,
        where_clause: Option<&Expr>,
        scopes: &mut ScopeStack,
    ) -> SigResult<Value> {
        let cursor = scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(table))
            .cloned();
        let (column_names, rows) = match cursor {
            Some(value) => {
                let (column_names, rows) = rows_from_records(table, value)?;
                if rows.is_empty() {
                    // An empty cursor has no known columns to check against.
                    return Ok(Value::List(Vec::new()));
                }
                (column_names, rows)
            }
            None => {
                let data = self.fetch_table(table, false)?;
                (data.columns, data.rows)
            }
        };

        // Resolve each selected column to its position up front, so a typo
        // fails even when the table is empty. SQLite names are case-insensitive.
        let mut picks = Vec::with_capacity(columns.len());
        for col in columns {
            let Some(i) = column_names
                .iter()
                .position(|c| c.eq_ignore_ascii_case(col))
            else {
                return Err(Signal::Fatal(format!(
                    "unknown column '{col}' in '{table}' (columns: {})",
                    column_names.join(", ")
                )));
            };
            picks.push((col.clone(), i));
        }

        let mut out = Vec::new();
        for row in rows {
            if let Some(condition) = where_clause {
                scopes.push(
                    column_names
                        .iter()
                        .cloned()
                        .zip(row.iter().cloned())
                        .collect(),
                );
                let keep = self.eval_bool(condition, scopes);
                scopes.pop();
                if !keep? {
                    continue;
                }
            }
            let fields = picks
                .iter()
                .map(|(name, i)| (name.clone(), row[*i].clone()))
                .collect();
            out.push(Value::Record(fields));
        }
        Ok(Value::List(out))
    }

    fn eval_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        scopes: &mut ScopeStack,
    ) -> SigResult<Value> {
        let Expr::Ident(name) = callee else {
            return Err(Signal::Fatal(
                "only calling a plain function name is supported right now".to_string(),
            ));
        };

        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            values.push(self.eval_expr(arg, scopes)?);
        }

        if let Some(result) = call_builtin(name, &values) {
            return result;
        }

        let Some(func) = self.functions.get(name).cloned() else {
            return Err(Signal::Fatal(format!("undefined function '{name}'")));
        };

        if func.params.len() != values.len() {
            return Err(Signal::Fatal(format!(
                "function '{name}' expects {} argument(s), got {}",
                func.params.len(),
                values.len()
            )));
        }

        let mut fn_scopes: ScopeStack = vec![func.params.into_iter().zip(values).collect()];
        match self.exec_stmts(&func.body, &mut fn_scopes) {
            Ok(()) => Ok(Value::Null),
            Err(Signal::Return(v)) => Ok(v),
            Err(Signal::Propagate(v)) => Ok(Value::Result(Err(Box::new(v)))),
            Err(Signal::Fatal(msg)) => Err(Signal::Fatal(msg)),
        }
    }
}

/// Turns a variable used as a `select` source into columns and rows. It must
/// hold a list of records. Columns are every field name seen, in first-seen
/// order; a record missing a field contributes `null` for it.
fn rows_from_records(name: &str, value: Value) -> SigResult<(Vec<String>, Vec<Vec<Value>>)> {
    let not_queryable = || {
        Signal::Fatal(format!(
            "cannot select from '{name}': it is not a list of records"
        ))
    };
    let Value::List(items) = value else {
        return Err(not_queryable());
    };

    let mut records = Vec::with_capacity(items.len());
    let mut column_names: Vec<String> = Vec::new();
    for item in items {
        let Value::Record(fields) = item else {
            return Err(not_queryable());
        };
        for (field, _) in &fields {
            if !column_names.contains(field) {
                column_names.push(field.clone());
            }
        }
        records.push(fields);
    }

    let rows = records
        .into_iter()
        .map(|fields| {
            column_names
                .iter()
                .map(|col| {
                    fields
                        .iter()
                        .find(|(f, _)| f == col)
                        .map(|(_, v)| v.clone())
                        .unwrap_or(Value::Null)
                })
                .collect()
        })
        .collect();
    Ok((column_names, rows))
}

/// Turns a SQLite error from reading `table` into a Foxy error message.
fn table_error(table: &str, e: rusqlite::Error) -> Signal {
    let msg = e.to_string();
    if msg.starts_with("no such table") {
        Signal::Fatal(format!("unknown table '{table}'"))
    } else if msg.contains("no such column: rowid") {
        Signal::Fatal(format!(
            "table '{table}' has no rowid, which update/delete need (WITHOUT ROWID tables aren't supported)"
        ))
    } else {
        Signal::Fatal(format!("query failed: {msg}"))
    }
}

/// Finds `name` among a table's columns (case-insensitively, like SQLite)
/// and returns the column's real spelling.
fn resolve_column(table: &str, columns: &[String], name: &str) -> SigResult<String> {
    columns
        .iter()
        .find(|c| c.eq_ignore_ascii_case(name))
        .cloned()
        .ok_or_else(|| {
            Signal::Fatal(format!(
                "unknown column '{name}' in '{table}' (columns: {})",
                columns.join(", ")
            ))
        })
}

/// Converts a Foxy value into one SQLite can store. Bools are stored as 1/0,
/// SQLite's convention; lists, records, and Results can't be stored.
fn to_sql(column: &str, value: &Value) -> SigResult<SqlValue> {
    Ok(match value {
        Value::Null => SqlValue::Null,
        Value::Int(n) => SqlValue::Integer(*n),
        Value::Float(f) => SqlValue::Real(*f),
        Value::Str(s) => SqlValue::Text(s.clone()),
        Value::Bool(b) => SqlValue::Integer(i64::from(*b)),
        other => {
            return Err(Signal::Fatal(format!(
                "cannot store {other} in column '{column}': only numbers, strings, bools, and null fit in a table"
            )));
        }
    })
}

/// Wraps a table or column name in double quotes for SQL, so names are never
/// read as SQL keywords. The lexer only produces plain identifiers, but quotes
/// inside a name are escaped anyway.
fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Built-in functions available without any `import`. Returns `None` if
/// `name` isn't one, so the caller falls through to user-defined functions.
fn call_builtin(name: &str, args: &[Value]) -> Option<SigResult<Value>> {
    match name {
        "print" => {
            let rendered = args
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            println!("{rendered}");
            Some(Ok(Value::Null))
        }
        "read_file" => Some(builtin_read_file(args)),
        "write_file" => Some(builtin_write_file(args)),
        _ => None,
    }
}

fn builtin_read_file(args: &[Value]) -> SigResult<Value> {
    let [Value::Str(path)] = args else {
        return Err(Signal::Fatal(
            "read_file(path) expects a single string argument".to_string(),
        ));
    };
    let result = match std::fs::read_to_string(path) {
        Ok(contents) => Ok(Box::new(Value::Str(contents))),
        Err(e) => Err(Box::new(Value::Str(e.to_string()))),
    };
    Ok(Value::Result(result))
}

fn builtin_write_file(args: &[Value]) -> SigResult<Value> {
    let [Value::Str(path), Value::Str(contents)] = args else {
        return Err(Signal::Fatal(
            "write_file(path, contents) expects two string arguments".to_string(),
        ));
    };
    let result = match std::fs::write(path, contents) {
        Ok(()) => Ok(Box::new(Value::Null)),
        Err(e) => Err(Box::new(Value::Str(e.to_string()))),
    };
    Ok(Value::Result(result))
}

fn eval_binary(op: BinOp, left: Value, right: Value) -> Result<Value, String> {
    use Value::{Bool, Float, Int, Str};

    if op == BinOp::Add
        && let (Str(a), Str(b)) = (&left, &right)
    {
        return Ok(Str(format!("{a}{b}")));
    }

    match op {
        BinOp::Eq => return Ok(Bool(values_equal(&left, &right))),
        BinOp::NotEq => return Ok(Bool(!values_equal(&left, &right))),
        _ => {}
    }

    // Int op Int stays Int; any Float operand promotes the whole expression
    // to Float. Anything else (strings, bools, ...) is a type error here.
    match (left, right) {
        (Int(a), Int(b)) => int_binary(op, a, b),
        (Int(a), Float(b)) => float_binary(op, a as f64, b),
        (Float(a), Int(b)) => float_binary(op, a, b as f64),
        (Float(a), Float(b)) => float_binary(op, a, b),
        (l, r) => Err(format!("{op:?} is not defined for {l} and {r}")),
    }
}

fn int_binary(op: BinOp, a: i64, b: i64) -> Result<Value, String> {
    use Value::{Bool, Int};
    match op {
        BinOp::Add => Ok(Int(a + b)),
        BinOp::Sub => Ok(Int(a - b)),
        BinOp::Mul => Ok(Int(a * b)),
        BinOp::Div => {
            if b == 0 {
                Err("division by zero".to_string())
            } else {
                Ok(Int(a / b))
            }
        }
        BinOp::Mod => {
            if b == 0 {
                Err("division by zero".to_string())
            } else {
                Ok(Int(a % b))
            }
        }
        BinOp::Lt => Ok(Bool(a < b)),
        BinOp::LtEq => Ok(Bool(a <= b)),
        BinOp::Gt => Ok(Bool(a > b)),
        BinOp::GtEq => Ok(Bool(a >= b)),
        BinOp::Eq | BinOp::NotEq => unreachable!("handled by values_equal"),
    }
}

fn float_binary(op: BinOp, a: f64, b: f64) -> Result<Value, String> {
    use Value::{Bool, Float};
    match op {
        BinOp::Add => Ok(Float(a + b)),
        BinOp::Sub => Ok(Float(a - b)),
        BinOp::Mul => Ok(Float(a * b)),
        BinOp::Div => {
            if b == 0.0 {
                Err("division by zero".to_string())
            } else {
                Ok(Float(a / b))
            }
        }
        BinOp::Mod => {
            if b == 0.0 {
                Err("division by zero".to_string())
            } else {
                Ok(Float(a % b))
            }
        }
        BinOp::Lt => Ok(Bool(a < b)),
        BinOp::LtEq => Ok(Bool(a <= b)),
        BinOp::Gt => Ok(Bool(a > b)),
        BinOp::GtEq => Ok(Bool(a >= b)),
        BinOp::Eq | BinOp::NotEq => unreachable!("handled by values_equal"),
    }
}

fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Int(x), Value::Float(y)) | (Value::Float(y), Value::Int(x)) => *x as f64 == *y,
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn run(src: &str) -> Result<(), String> {
        let tokens = Lexer::new(src).tokenize().expect("lex should succeed");
        let program = Parser::new(tokens)
            .parse_program()
            .expect("parse should succeed");
        Interpreter::new().run_program(&program)
    }

    /// Runs a program whose final top-level statement is `var result = <expr>`,
    /// and returns the value bound to `result`.
    fn eval_result(src: &str) -> Value {
        let full = format!("{src}\nvar __probe_result = result");
        let tokens = Lexer::new(&full).tokenize().expect("lex should succeed");
        let program = Parser::new(tokens)
            .parse_program()
            .expect("parse should succeed");

        let mut interp = Interpreter::new();
        for stmt in &program {
            if let Stmt::FnDecl {
                name, params, body, ..
            } = stmt
            {
                interp.functions.insert(
                    name.clone(),
                    FunctionDef {
                        params: params.iter().map(|p| p.name.clone()).collect(),
                        body: body.clone(),
                    },
                );
            }
        }
        let mut scopes: ScopeStack = vec![Scope::new()];
        for stmt in &program {
            if matches!(stmt, Stmt::FnDecl { .. }) {
                continue;
            }
            match interp.exec_stmt(stmt, &mut scopes) {
                Ok(()) => {}
                Err(Signal::Fatal(msg)) => panic!("runtime error: {msg}"),
                Err(Signal::Propagate(v)) => panic!("unhandled propagation: {v}"),
                Err(Signal::Return(_)) => break,
            }
        }
        scopes
            .last()
            .unwrap()
            .get("__probe_result")
            .cloned()
            .expect("__probe_result should be set")
    }

    #[test]
    fn evaluates_arithmetic_with_int_float_promotion() {
        assert_eq!(eval_result("var result = 1 + 2"), Value::Int(3));
        assert_eq!(eval_result("var result = 1 + 2.5"), Value::Float(3.5));
        assert_eq!(eval_result("var result = 2.5 + 1"), Value::Float(3.5));
    }

    #[test]
    fn integer_division_and_modulo_stay_integer() {
        assert_eq!(eval_result("var result = 7 / 2"), Value::Int(3));
        assert_eq!(eval_result("var result = 7 % 2"), Value::Int(1));
    }

    #[test]
    fn division_by_zero_is_a_runtime_error() {
        let err = run("var x = 1 / 0").unwrap_err();
        assert!(err.contains("division by zero"));
    }

    #[test]
    fn string_concatenation_with_plus() {
        assert_eq!(
            eval_result(r#"var result = "Hello, " + "world""#),
            Value::Str("Hello, world".into())
        );
    }

    #[test]
    fn comparisons_and_equality() {
        assert_eq!(eval_result("var result = 1 == 1.0"), Value::Bool(true));
        assert_eq!(eval_result("var result = 3 > 2"), Value::Bool(true));
        assert_eq!(eval_result(r#"var result = 1 == "1""#), Value::Bool(false));
    }

    #[test]
    fn if_else_branches() {
        // `var` inside a branch is block-scoped and doesn't leak out -- `result`
        // has to be declared outside and reassigned inside to observe it here.
        assert_eq!(
            eval_result("var result = 0\nif true {\n    result = 1\n} else {\n    result = 2\n}"),
            Value::Int(1)
        );
    }

    #[test]
    fn while_loop_accumulates() {
        let src = "var total = 0\nvar i = 1\nwhile i <= 5 {\n    total = total + i\n    i = i + 1\n}\nvar result = total";
        assert_eq!(eval_result(src), Value::Int(15));
    }

    #[test]
    fn for_in_loop_over_list() {
        let src =
            "var total = 0\nfor n in [1, 2, 3, 4] {\n    total = total + n\n}\nvar result = total";
        assert_eq!(eval_result(src), Value::Int(10));
    }

    #[test]
    fn function_call_and_return() {
        let src = "fn double(n: int) -> int {\n    return n * 2\n}\nvar result = double(21)";
        assert_eq!(eval_result(src), Value::Int(42));
    }

    #[test]
    fn function_without_explicit_return_yields_null() {
        let src = "fn noop() {\n    var x = 1\n}\nvar result = noop()";
        assert_eq!(eval_result(src), Value::Null);
    }

    #[test]
    fn try_operator_unwraps_ok_inside_function() {
        let src = "fn safe_div(a: int, b: int) -> Result<int, string> {\n    if b == 0 {\n        return Err(\"div by zero\")\n    }\n    return Ok(a / b)\n}\nfn double_safe(a: int, b: int) -> int {\n    var v = safe_div(a, b)?\n    return v * 2\n}\nvar result = double_safe(10, 2)";
        assert_eq!(eval_result(src), Value::Int(10));
    }

    #[test]
    fn try_operator_propagates_err_as_function_result() {
        let src = "fn safe_div(a: int, b: int) -> Result<int, string> {\n    if b == 0 {\n        return Err(\"div by zero\")\n    }\n    return Ok(a / b)\n}\nfn double_safe(a: int, b: int) -> Result<int, string> {\n    var v = safe_div(a, b)?\n    return Ok(v * 2)\n}\nvar result = double_safe(10, 0)";
        assert_eq!(
            eval_result(src),
            Value::Result(Err(Box::new(Value::Str("div by zero".into()))))
        );
    }

    #[test]
    fn try_operator_at_top_level_halts_with_the_wrapped_error() {
        let src = "fn always_fails() -> Result<int, string> {\n    return Err(\"nope\")\n}\nvar x = always_fails()?";
        let err = run(src).unwrap_err();
        assert!(err.contains("nope"));
    }

    #[test]
    fn undefined_variable_is_a_runtime_error() {
        let err = run("var x = y").unwrap_err();
        assert!(err.contains("undefined variable"));
    }

    #[test]
    fn assign_to_undeclared_variable_is_a_runtime_error() {
        let err = run("x = 1").unwrap_err();
        assert!(err.contains("undefined variable"));
    }

    #[test]
    fn record_field_access() {
        assert_eq!(
            eval_result(
                r#"var p = { name: "Alice", age: 30 }
var result = p.age"#
            ),
            Value::Int(30)
        );
    }

    #[test]
    fn print_builtin_does_not_error() {
        assert!(run(r#"print("hello", 1, true)"#).is_ok());
    }

    #[test]
    fn runs_fizzbuzz_example() {
        let src = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/fizzbuzz.foxy"
        ))
        .expect("fizzbuzz example should exist");
        assert!(run(&src).is_ok());
    }

    #[test]
    fn write_file_then_read_file_round_trips_via_result() {
        let path = std::env::temp_dir().join("foxy_interp_test_round_trip.txt");
        let path_str = path.to_str().unwrap().replace('\\', "\\\\");
        let src = format!(
            r#"var w = write_file("{path_str}", "hello from foxy")?
var result = read_file("{path_str}")?"#
        );
        assert_eq!(eval_result(&src), Value::Str("hello from foxy".into()));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn read_file_missing_path_is_an_err_variant() {
        match eval_result(r#"var result = read_file("/no/such/path/foxy_missing.txt")"#) {
            Value::Result(Err(_)) => {}
            other => panic!("expected an Err Result, got {other:?}"),
        }
    }

    #[test]
    fn runs_file_io_example() {
        let src = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/file_io.foxy"
        ))
        .expect("file_io example should exist");
        assert!(run(&src).is_ok());
    }

    /// Creates a fresh SQLite file in the temp dir with a small `customers`
    /// table, and returns its path escaped for use inside a Foxy string.
    fn temp_db(name: &str) -> (std::path::PathBuf, String) {
        let path = std::env::temp_dir().join(format!("foxy_test_{name}.db"));
        let _ = std::fs::remove_file(&path);
        let conn = Connection::open(&path).expect("create test db");
        conn.execute_batch(
            "CREATE TABLE customers (name TEXT, age INTEGER, balance REAL, note TEXT);
             INSERT INTO customers VALUES ('Ann', 34, 12.5, NULL);
             INSERT INTO customers VALUES ('Bo', 17, 0.0, 'minor');",
        )
        .expect("seed test db");
        let escaped = path.to_str().unwrap().replace('\\', "\\\\");
        (path, escaped)
    }

    #[test]
    fn select_returns_rows_as_records_in_column_order() {
        let (path, db) = temp_db("select_rows");
        let src = format!(
            r#"open "{db}"
var result = select age, name from customers"#
        );
        assert_eq!(
            eval_result(&src),
            Value::List(vec![
                Value::Record(vec![
                    ("age".into(), Value::Int(34)),
                    ("name".into(), Value::Str("Ann".into())),
                ]),
                Value::Record(vec![
                    ("age".into(), Value::Int(17)),
                    ("name".into(), Value::Str("Bo".into())),
                ]),
            ])
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn select_maps_sqlite_real_and_null_to_foxy_values() {
        let (path, db) = temp_db("select_types");
        let src = format!(
            r#"open "{db}"
var rows = select balance, note from customers
var result = rows"#
        );
        match eval_result(&src) {
            Value::List(rows) => assert_eq!(
                rows[0],
                Value::Record(vec![
                    ("balance".into(), Value::Float(12.5)),
                    ("note".into(), Value::Null),
                ])
            ),
            other => panic!("expected a list, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn select_rows_work_with_for_and_field_access() {
        let (path, db) = temp_db("select_for");
        let src = format!(
            r#"open "{db}"
var total = 0
for row in select age from customers {{
    total = total + row.age
}}
var result = total"#
        );
        assert_eq!(eval_result(&src), Value::Int(51));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn select_without_open_is_an_error() {
        let err = run("var rows = select name from customers").unwrap_err();
        assert!(err.contains("no database is open"), "{err}");
    }

    #[test]
    fn open_missing_file_is_an_error_and_creates_nothing() {
        let path = std::env::temp_dir().join("foxy_test_does_not_exist.db");
        let _ = std::fs::remove_file(&path);
        let db = path.to_str().unwrap().replace('\\', "\\\\");
        let err = run(&format!(r#"open "{db}""#)).unwrap_err();
        assert!(err.contains("cannot open database"), "{err}");
        assert!(!path.exists());
    }

    #[test]
    fn select_unknown_column_reports_the_sqlite_error() {
        let (path, db) = temp_db("select_bad_column");
        let err = run(&format!(
            r#"open "{db}"
var rows = select nmae from customers"#
        ))
        .unwrap_err();
        assert!(err.contains("unknown column 'nmae'"), "{err}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn select_where_filters_rows() {
        let (path, db) = temp_db("where_filter");
        let src = format!(
            r#"open "{db}"
var result = select name from customers where age >= 18"#
        );
        assert_eq!(
            eval_result(&src),
            Value::List(vec![Value::Record(vec![(
                "name".into(),
                Value::Str("Ann".into())
            )])])
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn select_where_can_use_variables_and_functions() {
        let (path, db) = temp_db("where_fn");
        let src = format!(
            r#"fn is_minor(a: int, limit: int) -> bool {{
    return a < limit
}}
open "{db}"
var limit = 18
var result = select name from customers where is_minor(age, limit)"#
        );
        assert_eq!(
            eval_result(&src),
            Value::List(vec![Value::Record(vec![(
                "name".into(),
                Value::Str("Bo".into())
            )])])
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn select_where_with_no_matches_is_an_empty_list() {
        let (path, db) = temp_db("where_empty");
        let src = format!(
            r#"open "{db}"
var result = select name from customers where age > 100"#
        );
        assert_eq!(eval_result(&src), Value::List(vec![]));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn select_where_must_be_a_bool() {
        let (path, db) = temp_db("where_not_bool");
        let err = run(&format!(
            r#"open "{db}"
var rows = select name from customers where age"#
        ))
        .unwrap_err();
        assert!(err.contains("condition must be a bool"), "{err}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn select_unknown_table_is_a_clear_error() {
        let (path, db) = temp_db("bad_table");
        let err = run(&format!(
            r#"open "{db}"
var rows = select name from custmers"#
        ))
        .unwrap_err();
        assert!(err.contains("unknown table 'custmers'"), "{err}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn runs_hello_data_example() {
        let src = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/hello_data.foxy"
        ))
        .expect("hello_data example should exist");
        assert!(run(&src).is_ok());
    }

    #[test]
    fn select_from_a_previous_result_queries_it_again() {
        let (path, db) = temp_db("cursor_chain");
        let src = format!(
            r#"open "{db}"
var everyone = select name, age from customers
var result = select name from everyone where age < 18"#
        );
        assert_eq!(
            eval_result(&src),
            Value::List(vec![Value::Record(vec![(
                "name".into(),
                Value::Str("Bo".into())
            )])])
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn select_from_a_hand_built_list_needs_no_database() {
        let src = r#"var people = [{ name: "Ann", age: 34 }, { name: "Bo" }]
var result = select age, name from people"#;
        assert_eq!(
            eval_result(src),
            Value::List(vec![
                Value::Record(vec![
                    ("age".into(), Value::Int(34)),
                    ("name".into(), Value::Str("Ann".into())),
                ]),
                Value::Record(vec![
                    ("age".into(), Value::Null),
                    ("name".into(), Value::Str("Bo".into())),
                ]),
            ])
        );
    }

    #[test]
    fn a_variable_wins_over_a_table_with_the_same_name() {
        let (path, db) = temp_db("cursor_shadow");
        let src = format!(
            r#"open "{db}"
var customers = [{{ name: "Zed" }}]
var result = select name from customers"#
        );
        assert_eq!(
            eval_result(&src),
            Value::List(vec![Value::Record(vec![(
                "name".into(),
                Value::Str("Zed".into())
            )])])
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn select_from_an_empty_result_is_empty() {
        let src = r#"var nobody = []
var result = select name from nobody where age > 1"#;
        assert_eq!(eval_result(src), Value::List(vec![]));
    }

    #[test]
    fn select_from_a_non_record_variable_is_an_error() {
        let err = run("var nums = [1, 2]\nvar rows = select x from nums").unwrap_err();
        assert!(err.contains("not a list of records"), "{err}");
    }

    #[test]
    fn select_unknown_column_in_a_cursor_is_an_error() {
        let err =
            run("var people = [{ name: \"Ann\" }]\nvar rows = select age from people").unwrap_err();
        assert!(err.contains("unknown column 'age' in 'people'"), "{err}");
    }

    /// Reads `SELECT name, age FROM customers ORDER BY rowid` straight from
    /// SQLite, to check what writes actually stored.
    fn names_and_ages(path: &std::path::Path) -> Vec<(String, Option<i64>)> {
        let conn = Connection::open(path).unwrap();
        let mut stmt = conn
            .prepare("SELECT name, age FROM customers ORDER BY rowid")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    #[test]
    fn insert_adds_a_row_from_a_record() {
        let (path, db) = temp_db("insert");
        run(&format!(
            r#"open "{db}"
insert into customers {{ name: "Fay", age: 40 }}"#
        ))
        .unwrap();
        assert_eq!(
            names_and_ages(&path).last().unwrap(),
            &("Fay".into(), Some(40))
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn insert_accepts_a_record_variable_and_stores_bools_as_numbers() {
        let (path, db) = temp_db("insert_var");
        run(&format!(
            r#"open "{db}"
var c = {{ name: "Gus", age: true }}
insert into customers c"#
        ))
        .unwrap();
        assert_eq!(
            names_and_ages(&path).last().unwrap(),
            &("Gus".into(), Some(1))
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn insert_unknown_column_or_non_record_is_an_error() {
        let (path, db) = temp_db("insert_bad");
        let err = run(&format!(
            r#"open "{db}"
insert into customers {{ nmae: "Fay" }}"#
        ))
        .unwrap_err();
        assert!(err.contains("unknown column 'nmae'"), "{err}");
        let err = run(&format!(
            r#"open "{db}"
insert into customers 5"#
        ))
        .unwrap_err();
        assert!(err.contains("insert expects a record"), "{err}");
        assert_eq!(names_and_ages(&path).len(), 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn update_changes_only_matching_rows_using_their_own_values() {
        let (path, db) = temp_db("update");
        run(&format!(
            r#"open "{db}"
update customers set age = age + 1, name = name + "!" where age < 18"#
        ))
        .unwrap();
        assert_eq!(
            names_and_ages(&path),
            vec![("Ann".into(), Some(34)), ("Bo!".into(), Some(18))]
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn update_where_true_changes_every_row() {
        let (path, db) = temp_db("update_all");
        run(&format!(
            r#"open "{db}"
update customers set age = 0 where true"#
        ))
        .unwrap();
        assert_eq!(
            names_and_ages(&path),
            vec![("Ann".into(), Some(0)), ("Bo".into(), Some(0))]
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn delete_removes_only_matching_rows() {
        let (path, db) = temp_db("delete");
        run(&format!(
            r#"open "{db}"
delete from customers where age < 18"#
        ))
        .unwrap();
        assert_eq!(names_and_ages(&path), vec![("Ann".into(), Some(34))]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_failed_update_leaves_the_table_unchanged() {
        let (path, db) = temp_db("update_atomic");
        Connection::open(&path)
            .unwrap()
            .execute_batch(
                "CREATE TABLE strict_people (name TEXT NOT NULL, age INTEGER);
                 INSERT INTO strict_people VALUES ('Ann', 1), ('Bo', 2);",
            )
            .unwrap();
        // Row 1 gets a valid name; row 2 gets null, which violates NOT NULL.
        let err = run(&format!(
            r#"fn new_name(a: int) -> string {{
    if a == 1 {{
        return "Changed"
    }}
    return null
}}
open "{db}"
update strict_people set name = new_name(age) where true"#
        ))
        .unwrap_err();
        assert!(err.contains("update failed"), "{err}");
        let conn = Connection::open(&path).unwrap();
        let first: String = conn
            .query_row("SELECT name FROM strict_people WHERE age = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(first, "Ann", "row 1 must be rolled back");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn update_cannot_store_a_list() {
        let (path, db) = temp_db("update_list");
        let err = run(&format!(
            r#"open "{db}"
update customers set name = [1] where true"#
        ))
        .unwrap_err();
        assert!(err.contains("cannot store"), "{err}");
        let _ = std::fs::remove_file(&path);
    }
}
