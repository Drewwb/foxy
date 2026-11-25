//! `foxy import`: copies a Visual FoxPro / dBASE table (.dbf) into a SQLite
//! database, the bridge for bringing legacy FoxPro data into Foxy.
//!
//! Mapping decisions (see docs/grammar.md):
//! - The table and its columns are named after the .dbf file and its fields,
//!   lowercased (FoxPro names are case-insensitive and usually uppercase),
//!   and turned into names Foxy can type: other characters become `_`, a
//!   leading digit gets a `_` prefix, and a keyword gets a `_` suffix
//!   (`marker-col` -> `marker_col`, `from` -> `from_`).
//! - Deleted-but-not-packed records are skipped, as FoxPro does after PACK.
//! - Dates become `YYYY-MM-DD` text and datetimes `YYYY-MM-DD HH:MM:SS`,
//!   since Foxy has no date type yet. Logicals become 1/0.
//! - Numeric fields use SQLite's NUMERIC type, so whole numbers come back
//!   to Foxy as ints and fractional ones as floats.
//! - A blank character field is an empty string; a blank number, date, or
//!   logical is null.
//! - Text is decoded with the code page recorded in the file (FoxPro writes
//!   one, usually 1252). A file that doesn't record one can be given an
//!   encoding explicitly, e.g. `--encoding cp850`.

use std::path::Path;

use dbase::{FieldType, FieldValue};

use crate::lexer::{Lexer, Token};
use rusqlite::Connection;
use rusqlite::types::Value as SqlValue;

/// What an import did, for the command to report.
#[derive(Debug, PartialEq)]
pub struct ImportSummary {
    pub table: String,
    pub columns: Vec<String>,
    pub rows: usize,
}

/// Imports `dbf_path` into a new table in `db_path`, creating the database
/// file if needed. Fails without changing anything if the table exists.
/// `encoding` overrides the code page recorded in the file (e.g. "cp850").
pub fn import_dbf(
    dbf_path: &Path,
    db_path: &Path,
    encoding: Option<&str>,
) -> Result<ImportSummary, String> {
    let table = dbf_path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(foxy_name)
        .ok_or_else(|| format!("cannot take a table name from '{}'", dbf_path.display()))?;

    let mut builder = dbase::ReaderBuilder::new();
    if let Some(name) = encoding {
        builder = builder.with_encoding(code_page(name)?);
    }
    let mut reader = builder
        .open(dbf_path)
        .map_err(|e| format!("cannot read '{}': {e}", dbf_path.display()))?;
    let fields: Vec<(String, String, FieldType)> = reader
        .fields()
        .iter()
        .map(|f| (f.name().to_string(), foxy_name(f.name()), f.field_type()))
        .collect();
    for (i, (original, name, _)) in fields.iter().enumerate() {
        if let Some((other, _, _)) = fields[..i].iter().find(|(_, n, _)| n == name) {
            return Err(format!(
                "fields '{other}' and '{original}' would both become column '{name}'"
            ));
        }
    }

    let mut conn = Connection::open(db_path)
        .map_err(|e| format!("cannot open database '{}': {e}", db_path.display()))?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;

    let exists: bool = tx
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1 COLLATE NOCASE)",
            [&table],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if exists {
        return Err(format!(
            "table '{table}' already exists in '{}'; nothing was imported",
            db_path.display()
        ));
    }

    let column_defs = fields
        .iter()
        .map(|(_, name, ty)| format!("{} {}", quote_ident(name), sqlite_type(*ty)))
        .collect::<Vec<_>>()
        .join(", ");
    tx.execute(
        &format!("CREATE TABLE {} ({column_defs})", quote_ident(&table)),
        [],
    )
    .map_err(|e| format!("cannot create table '{table}': {e}"))?;

    let placeholders = vec!["?"; fields.len()].join(", ");
    let insert = format!(
        "INSERT INTO {} VALUES ({placeholders})",
        quote_ident(&table)
    );
    let mut rows = 0;
    {
        let mut stmt = tx.prepare(&insert).map_err(|e| e.to_string())?;
        for (i, record) in reader.iter_records().enumerate() {
            let record = record.map_err(|e| format!("cannot read record {}: {e}", i + 1))?;
            let values: Vec<SqlValue> = fields
                .iter()
                .map(|(original, _, _)| record.get(original).map_or(SqlValue::Null, to_sql))
                .collect();
            stmt.execute(rusqlite::params_from_iter(values))
                .map_err(|e| format!("cannot store record {}: {e}", i + 1))?;
            rows += 1;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;

    Ok(ImportSummary {
        table,
        columns: fields.into_iter().map(|(_, name, _)| name).collect(),
        rows,
    })
}

/// Looks up a code page by name: "cp850", "850", "utf-8", and so on.
fn code_page(name: &str) -> Result<dbase::encoding::DynEncoding, String> {
    use dbase::CodePageMark as Cp;
    let key = name.trim().to_ascii_lowercase();
    let mark = match key.strip_prefix("cp").unwrap_or(&key) {
        "437" => Some(Cp::CP437),
        "850" => Some(Cp::CP850),
        "852" => Some(Cp::CP852),
        "861" => Some(Cp::CP861),
        "865" => Some(Cp::CP865),
        "866" => Some(Cp::CP866),
        "874" => Some(Cp::CP874),
        "1250" => Some(Cp::CP1250),
        "1251" => Some(Cp::CP1251),
        "1252" => Some(Cp::CP1252),
        "1253" => Some(Cp::CP1253),
        "1254" => Some(Cp::CP1254),
        "1255" => Some(Cp::CP1255),
        "1256" => Some(Cp::CP1256),
        "utf8" | "utf-8" => Some(Cp::Utf8),
        // Anything else is rejected rather than silently decoded some default way.
        _ => None,
    };
    mark.and_then(Cp::to_encoding).ok_or_else(|| {
        format!(
            "unknown encoding '{name}'; try cp437, cp850, cp852, cp861, cp865, cp866, cp874, cp1250-cp1256, or utf-8"
        )
    })
}

/// Turns a FoxPro table or field name into an identifier Foxy can type.
/// Uses Foxy's own lexer as the judge, so keywords are always caught.
fn foxy_name(raw: &str) -> String {
    let mut name: String = raw
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
        name.insert(0, '_');
    }
    let is_identifier = |n: &str| matches!(Lexer::new(n).tokenize().as_deref(), Ok([Token::Ident(t), Token::Eof]) if t == n);
    if !is_identifier(&name) {
        name.push('_');
    }
    name
}

fn sqlite_type(ty: FieldType) -> &'static str {
    match ty {
        FieldType::Character | FieldType::Memo | FieldType::Date | FieldType::DateTime => "TEXT",
        FieldType::Integer | FieldType::Logical => "INTEGER",
        FieldType::Float | FieldType::Double | FieldType::Currency => "REAL",
        FieldType::Numeric => "NUMERIC",
    }
}

fn to_sql(value: &FieldValue) -> SqlValue {
    match value {
        FieldValue::Character(s) => SqlValue::Text(s.clone().unwrap_or_default()),
        FieldValue::Memo(s) => SqlValue::Text(s.clone()),
        FieldValue::Numeric(n) => n.map_or(SqlValue::Null, SqlValue::Real),
        FieldValue::Float(n) => n.map_or(SqlValue::Null, |f| SqlValue::Real(f64::from(f))),
        FieldValue::Double(n) | FieldValue::Currency(n) => SqlValue::Real(*n),
        FieldValue::Integer(n) => SqlValue::Integer(i64::from(*n)),
        FieldValue::Logical(b) => b.map_or(SqlValue::Null, |b| SqlValue::Integer(i64::from(b))),
        FieldValue::Date(d) => d
            .as_ref()
            .map_or(SqlValue::Null, |d| SqlValue::Text(date_text(d))),
        FieldValue::DateTime(dt) => {
            let t = dt.time();
            SqlValue::Text(format!(
                "{} {:02}:{:02}:{:02}",
                date_text(&dt.date()),
                t.hours(),
                t.minutes(),
                t.seconds()
            ))
        }
    }
}

fn date_text(d: &dbase::Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day())
}

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbase::{FieldName, Record, TableWriterBuilder};
    use std::path::PathBuf;

    /// Writes a small FoxPro-style CUSTOMERS.DBF (uppercase names, like VFP)
    /// into a fresh temp dir and returns (dbf path, db path).
    fn fixture(name: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("foxy_import_{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dbf = dir.join("CUSTOMERS.DBF");

        let mut writer = TableWriterBuilder::new()
            .add_character_field(FieldName::try_from("NAME").unwrap(), 20)
            .add_numeric_field(FieldName::try_from("AGE").unwrap(), 3, 0)
            .add_numeric_field(FieldName::try_from("BALANCE").unwrap(), 10, 2)
            .add_logical_field(FieldName::try_from("ACTIVE").unwrap())
            .add_date_field(FieldName::try_from("JOINED").unwrap())
            .build_with_file_dest(&dbf)
            .unwrap();
        let rows = [
            (
                "Ann",
                Some(34.0),
                Some(120.5),
                Some(true),
                Some((14, 3, 2001)),
            ),
            ("Bo", Some(17.0), None, Some(false), None),
        ];
        for (n, age, bal, active, joined) in rows {
            let mut r = Record::default();
            r.insert("NAME".into(), FieldValue::Character(Some(n.into())));
            r.insert("AGE".into(), FieldValue::Numeric(age));
            r.insert("BALANCE".into(), FieldValue::Numeric(bal));
            r.insert("ACTIVE".into(), FieldValue::Logical(active));
            r.insert(
                "JOINED".into(),
                FieldValue::Date(joined.map(|(d, m, y)| dbase::Date::new(d, m, y).unwrap())),
            );
            writer.write_record(&r).unwrap();
        }
        writer.finalize().unwrap();
        (dbf, dir.join("shop.db"))
    }

    #[test]
    fn imports_a_dbf_into_a_new_lowercase_table() {
        let (dbf, db) = fixture("basic");
        let summary = import_dbf(&dbf, &db, None).unwrap();
        assert_eq!(
            summary,
            ImportSummary {
                table: "customers".into(),
                columns: vec!["name", "age", "balance", "active", "joined"]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                rows: 2,
            }
        );

        let conn = Connection::open(&db).unwrap();
        let row: (String, i64, f64, i64, String) = conn
            .query_row(
                "SELECT name, age, balance, active, joined FROM customers WHERE name = 'Ann'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(row, ("Ann".into(), 34, 120.5, 1, "2001-03-14".into()));

        let (balance, joined): (Option<f64>, Option<String>) = conn
            .query_row(
                "SELECT balance, joined FROM customers WHERE name = 'Bo'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((balance, joined), (None, None));
    }

    #[test]
    fn whole_numbers_come_back_to_foxy_as_ints() {
        let (dbf, db) = fixture("ints");
        import_dbf(&dbf, &db, None).unwrap();
        let conn = Connection::open(&db).unwrap();
        let kind: String = conn
            .query_row("SELECT typeof(age) FROM customers LIMIT 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(kind, "integer");
    }

    #[test]
    fn importing_into_an_existing_table_changes_nothing() {
        let (dbf, db) = fixture("twice");
        import_dbf(&dbf, &db, None).unwrap();
        let err = import_dbf(&dbf, &db, None).unwrap_err();
        assert!(err.contains("already exists"), "{err}");
        let conn = Connection::open(&db).unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM customers", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn names_become_identifiers_foxy_can_type() {
        assert_eq!(foxy_name("MARKER-COL"), "marker_col");
        assert_eq!(foxy_name("KUNDEN-schema only"), "kunden_schema_only");
        assert_eq!(foxy_name("2024SALES"), "_2024sales");
        assert_eq!(foxy_name("FROM"), "from_");
        assert_eq!(foxy_name("Select"), "select_");
        assert_eq!(foxy_name("CUST_ID"), "cust_id");
    }

    #[test]
    fn an_explicit_encoding_is_used_and_a_bad_one_is_rejected() {
        let (dbf, db) = fixture("encoding");
        assert_eq!(import_dbf(&dbf, &db, Some("cp1252")).unwrap().rows, 2);
        let err = import_dbf(&dbf, &db, Some("klingon")).unwrap_err();
        assert!(err.contains("unknown encoding 'klingon'"), "{err}");
    }

    #[test]
    fn a_missing_dbf_is_a_clear_error() {
        let dir = std::env::temp_dir();
        let err = import_dbf(&dir.join("no_such_table.dbf"), &dir.join("x.db"), None).unwrap_err();
        assert!(err.contains("cannot read"), "{err}");
    }
}
