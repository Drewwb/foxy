//! Renders query results as a text grid, for the REPL.
//!
//! Only the REPL uses this: `print()` in scripts keeps the one-line form, so
//! script output stays predictable (a design decision, see docs/grammar.md).

use crate::value::Value;

/// Cells longer than this are cut short with "...", so one long value
/// can't stretch the whole grid off the screen.
const MAX_CELL_WIDTH: usize = 40;

/// How the REPL shows a value: a grid for a non-empty list of records,
/// the normal one-line form for everything else.
pub fn render_for_repl(value: &Value) -> String {
    match as_records(value) {
        Some(records) => render_grid(&records),
        None => value.to_string(),
    }
}

/// The records in `value`, if it is a non-empty list made only of records.
fn as_records(value: &Value) -> Option<Vec<&[(String, Value)]>> {
    let Value::List(items) = value else {
        return None;
    };
    if items.is_empty() {
        return None;
    }
    items
        .iter()
        .map(|item| match item {
            Value::Record(fields) => Some(fields.as_slice()),
            _ => None,
        })
        .collect()
}

fn render_grid(records: &[&[(String, Value)]]) -> String {
    // Columns are every field name seen, in first-seen order. A record
    // missing a field gets an empty cell (different from a stored `null`).
    let mut columns: Vec<&str> = Vec::new();
    for fields in records {
        for (name, _) in fields.iter() {
            if !columns.contains(&name.as_str()) {
                columns.push(name);
            }
        }
    }

    let rows: Vec<Vec<String>> = records
        .iter()
        .map(|fields| {
            columns
                .iter()
                .map(|col| {
                    fields
                        .iter()
                        .find(|(name, _)| name == col)
                        .map(|(_, v)| cell_text(v))
                        .unwrap_or_default()
                })
                .collect()
        })
        .collect();

    let widths: Vec<usize> = columns
        .iter()
        .enumerate()
        .map(|(i, col)| {
            rows.iter()
                .map(|row| width(&row[i]))
                .chain([width(col)])
                .max()
                .unwrap_or(0)
        })
        .collect();

    let border = format!(
        "+{}+",
        widths
            .iter()
            .map(|w| "-".repeat(w + 2))
            .collect::<Vec<_>>()
            .join("+")
    );
    let line = |cells: Vec<&str>| {
        let padded: Vec<String> = cells
            .iter()
            .zip(&widths)
            .map(|(cell, w)| format!(" {cell}{} ", " ".repeat(w - width(cell))))
            .collect();
        format!("|{}|", padded.join("|"))
    };

    let mut out = vec![border.clone(), line(columns.clone()), border.clone()];
    for row in &rows {
        out.push(line(row.iter().map(String::as_str).collect()));
    }
    out.push(border);
    let n = rows.len();
    out.push(format!("({n} {})", if n == 1 { "row" } else { "rows" }));
    out.join("\n")
}

/// One value as grid cell text: newlines shown as `\n` so a row stays on one
/// line, and long text cut short.
fn cell_text(value: &Value) -> String {
    let text = value.to_string().replace('\n', "\\n");
    if width(&text) > MAX_CELL_WIDTH {
        let kept: String = text.chars().take(MAX_CELL_WIDTH - 3).collect();
        format!("{kept}...")
    } else {
        text
    }
}

/// Display width, counted in characters rather than bytes so names like
/// "José" line up.
fn width(s: &str) -> usize {
    s.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(fields: &[(&str, Value)]) -> Value {
        Value::Record(
            fields
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        )
    }

    #[test]
    fn renders_a_list_of_records_as_a_grid() {
        let v = Value::List(vec![
            rec(&[("name", Value::Str("Ann".into())), ("age", Value::Int(34))]),
            rec(&[("name", Value::Str("Cleo".into())), ("age", Value::Int(52))]),
        ]);
        assert_eq!(
            render_for_repl(&v),
            "+------+-----+\n\
             | name | age |\n\
             +------+-----+\n\
             | Ann  | 34  |\n\
             | Cleo | 52  |\n\
             +------+-----+\n\
             (2 rows)"
        );
    }

    #[test]
    fn one_row_says_row_not_rows() {
        let v = Value::List(vec![rec(&[("n", Value::Int(1))])]);
        assert!(render_for_repl(&v).ends_with("(1 row)"));
    }

    #[test]
    fn null_shows_as_null_and_a_missing_field_as_blank() {
        let v = Value::List(vec![
            rec(&[("a", Value::Null), ("b", Value::Int(1))]),
            rec(&[("a", Value::Int(2))]),
        ]);
        let out = render_for_repl(&v);
        assert!(out.contains("| null | 1 |"), "{out}");
        assert!(out.contains("| 2    |   |"), "{out}");
    }

    #[test]
    fn other_values_keep_their_normal_form() {
        assert_eq!(render_for_repl(&Value::Int(3)), "3");
        assert_eq!(render_for_repl(&Value::List(vec![])), "[]");
        let mixed = Value::List(vec![Value::Int(1), rec(&[("a", Value::Int(1))])]);
        assert_eq!(render_for_repl(&mixed), mixed.to_string());
    }

    #[test]
    fn long_and_multiline_text_stays_on_one_short_line() {
        let long = "x".repeat(100);
        let v = Value::List(vec![rec(&[
            ("note", Value::Str(long)),
            ("lines", Value::Str("a\nb".into())),
        ])]);
        let out = render_for_repl(&v);
        assert!(out.contains(&format!("{}...", "x".repeat(37))), "{out}");
        assert!(out.contains("a\\nb"), "{out}");
        assert_eq!(out.lines().count(), 6);
    }

    #[test]
    fn widths_count_characters_not_bytes() {
        let v = Value::List(vec![
            rec(&[("name", Value::Str("José".into()))]),
            rec(&[("name", Value::Str("Bo".into()))]),
        ]);
        let out = render_for_repl(&v);
        assert!(out.contains("| José |"), "{out}");
        assert!(out.contains("| Bo   |"), "{out}");
    }
}
