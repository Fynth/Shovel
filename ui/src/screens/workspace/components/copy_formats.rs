use serde_json::{Map, Value};

pub fn format_row_json(columns: &[String], row: &[String]) -> String {
    let mut object = Map::with_capacity(columns.len());
    for (column, value) in columns.iter().zip(row.iter()) {
        object.insert(column.clone(), detail_json_value(value));
    }

    serde_json::to_string_pretty(&Value::Object(object)).unwrap_or_else(|_| "{}".to_string())
}

/// Serialize a row as TSV (header + body), suitable for pasting into spreadsheets.
pub fn format_row_tsv(columns: &[String], row: &[String]) -> String {
    let header = columns.join("\t");
    let body = row
        .iter()
        .map(|value| value.replace(['\t', '\n', '\r'], " "))
        .collect::<Vec<_>>()
        .join("\t");
    format!("{header}\n{body}")
}

/// Quote a single CSV field per RFC 4180. A trailing CR/LF is preserved
/// inside the quoted form so that multi-line cells survive the round-trip.
pub fn csv_quote(field: &str) -> String {
    let needs_quote = field
        .as_bytes()
        .iter()
        .any(|&byte| matches!(byte, b',' | b'"' | b'\n' | b'\r'));
    if !needs_quote {
        return field.to_string();
    }
    let escaped = field.replace('"', "\"\"");
    format!("\"{escaped}\"")
}

/// Serialize a row as a single CSV line. The header row is intentionally
/// omitted so a single copy-paste fills one cell range.
pub fn format_row_csv(_columns: &[String], row: &[String]) -> String {
    row.iter()
        .map(|value| csv_quote(value))
        .collect::<Vec<_>>()
        .join(",")
}

/// Serialize a full result page as CSV: header row + one data row per record,
/// with every field quoted per RFC 4180. Empty input produces a header line
/// and nothing else.
pub fn format_all_rows_csv(columns: &[String], rows: &[Vec<String>]) -> String {
    let header = columns
        .iter()
        .map(|column| csv_quote(column))
        .collect::<Vec<_>>()
        .join(",");
    if rows.is_empty() {
        return header;
    }
    let body = rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|value| csv_quote(value))
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("{header}\n{body}")
}

/// Serialize a full result page as a compact JSON array of objects.
/// Empty input serializes to `[]`; per-cell coercion matches `format_row_json`.
pub fn format_all_rows_json(columns: &[String], rows: &[Vec<String>]) -> String {
    let mut array = Vec::with_capacity(rows.len());
    for row in rows {
        let mut object = Map::with_capacity(columns.len());
        for (column, value) in columns.iter().zip(row.iter()) {
            object.insert(column.clone(), detail_json_value(value));
        }
        array.push(Value::Object(object));
    }
    serde_json::to_string(&Value::Array(array)).unwrap_or_else(|_| "[]".to_string())
}

/// Escape a single Markdown table cell. The pipe `|` is the column
/// separator, so any literal one has to be escaped to `\|` to keep the
/// row shape intact. Embedded newlines are replaced with `<br>` so a
/// multi-line value stays in a single row when rendered by GitHub or
/// other GFM viewers; carriage returns collapse to a space because they
/// have no Markdown equivalent and would otherwise produce an empty cell
/// artifact.
pub fn markdown_escape_cell(field: &str) -> String {
    let mut escaped = String::with_capacity(field.len());
    for character in field.chars() {
        match character {
            '|' => escaped.push_str(r"\|"),
            '\n' => escaped.push_str("<br>"),
            '\r' => escaped.push(' '),
            other => escaped.push(other),
        }
    }
    escaped
}

/// Serialize a full result page as a GitHub-flavored Markdown table:
/// header row, separator row (`---` per column), and one row per record.
/// Empty input produces just the header and separator.
pub fn format_all_rows_markdown(columns: &[String], rows: &[Vec<String>]) -> String {
    let header_cells = columns
        .iter()
        .map(|column| markdown_escape_cell(column))
        .collect::<Vec<_>>();
    let header = format!("| {} |", header_cells.join(" | "));

    let separator_cells = vec!["---".to_string(); columns.len()];
    let separator = format!("| {} |", separator_cells.join(" | "));

    let body = rows
        .iter()
        .map(|row| {
            let cells = row
                .iter()
                .map(|value| markdown_escape_cell(value))
                .collect::<Vec<_>>();
            format!("| {} |", cells.join(" | "))
        })
        .collect::<Vec<_>>()
        .join("\n");

    if body.is_empty() {
        format!("{header}\n{separator}")
    } else {
        format!("{header}\n{separator}\n{body}")
    }
}

pub fn detail_json_value(value: &str) -> Value {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("null") {
        Value::Null
    } else if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        serde_json::from_str::<Value>(trimmed).unwrap_or_else(|_| Value::String(value.to_string()))
    } else {
        Value::String(value.to_string())
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[test]
    fn csv_quote_passes_plain_fields_through_unchanged() {
        assert_eq!(csv_quote("hello"), "hello");
        assert_eq!(csv_quote(""), "");
        assert_eq!(csv_quote("plain-text_42"), "plain-text_42");
    }

    #[test]
    fn csv_quote_wraps_fields_containing_commas() {
        assert_eq!(csv_quote("a,b"), "\"a,b\"");
    }

    #[test]
    fn csv_quote_doubles_internal_double_quotes() {
        assert_eq!(csv_quote("she said \"hi\""), "\"she said \"\"hi\"\"\"");
    }

    #[test]
    fn csv_quote_preserves_newlines_inside_quoted_field() {
        assert_eq!(csv_quote("line1\nline2"), "\"line1\nline2\"");
        assert_eq!(csv_quote("line1\r\nline2"), "\"line1\r\nline2\"");
    }

    #[test]
    fn format_row_csv_omits_header() {
        let columns = vec!["id".to_string(), "name".to_string()];
        let row = vec!["1".to_string(), "Ada".to_string()];
        assert_eq!(format_row_csv(&columns, &row), "1,Ada");
    }

    #[test]
    fn format_row_csv_quotes_each_field_independently() {
        let columns = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let row = vec![
            "plain".to_string(),
            "with,comma".to_string(),
            "with\"quote".to_string(),
        ];
        assert_eq!(
            format_row_csv(&columns, &row),
            "plain,\"with,comma\",\"with\"\"quote\""
        );
    }

    #[test]
    fn format_all_rows_csv_includes_header_and_quote_columns() {
        let columns = vec!["id".to_string(), "note,detail".to_string()];
        let rows = vec![
            vec!["1".to_string(), "alpha".to_string()],
            vec!["2".to_string(), "she said \"hi\"".to_string()],
        ];
        let output = format_all_rows_csv(&columns, &rows);
        assert_eq!(
            output,
            "id,\"note,detail\"\n1,alpha\n2,\"she said \"\"hi\"\"\""
        );
    }

    #[test]
    fn format_all_rows_csv_emits_only_header_when_no_rows() {
        let columns = vec!["id".to_string(), "name".to_string()];
        let rows: Vec<Vec<String>> = Vec::new();
        assert_eq!(format_all_rows_csv(&columns, &rows), "id,name");
    }

    #[test]
    fn format_all_rows_json_serializes_array_of_objects() {
        let columns = vec!["id".to_string(), "name".to_string()];
        let rows = vec![
            vec!["1".to_string(), "Ada".to_string()],
            vec!["2".to_string(), "Linus".to_string()],
        ];
        let output = format_all_rows_json(&columns, &rows);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(
            parsed,
            serde_json::json!([
                {"id": "1", "name": "Ada"},
                {"id": "2", "name": "Linus"},
            ])
        );
    }

    #[test]
    fn format_all_rows_json_emits_empty_array_when_no_rows() {
        let columns = vec!["id".to_string()];
        let rows: Vec<Vec<String>> = Vec::new();
        assert_eq!(format_all_rows_json(&columns, &rows), "[]");
    }

    #[test]
    fn format_all_rows_json_is_compact_single_line() {
        let columns = vec!["a".to_string()];
        let rows = vec![vec!["x".to_string()], vec!["y".to_string()]];
        let output = format_all_rows_json(&columns, &rows);
        // Compact: no internal newlines or excessive whitespace.
        assert!(!output.contains('\n'));
        assert_eq!(output, r#"[{"a":"x"},{"a":"y"}]"#);
    }

    #[test]
    fn format_all_rows_markdown_emits_header_separator_and_data_rows() {
        let columns = vec!["name".to_string(), "age".to_string()];
        let rows = vec![
            vec!["Ada".to_string(), "36".to_string()],
            vec!["Bob".to_string(), "40".to_string()],
        ];
        assert_eq!(
            format_all_rows_markdown(&columns, &rows),
            "| name | age |\n| --- | --- |\n| Ada | 36 |\n| Bob | 40 |"
        );
    }

    #[test]
    fn format_all_rows_markdown_emits_header_and_separator_only_when_no_rows() {
        let columns = vec!["id".to_string(), "name".to_string()];
        let rows: Vec<Vec<String>> = Vec::new();
        assert_eq!(
            format_all_rows_markdown(&columns, &rows),
            "| id | name |\n| --- | --- |"
        );
    }

    #[test]
    fn format_all_rows_markdown_escapes_pipe_characters_in_cells() {
        let columns = vec!["value".to_string()];
        let rows = vec![vec!["a|b|c".to_string()]];
        assert_eq!(
            format_all_rows_markdown(&columns, &rows),
            "| value |\n| --- |\n| a\\|b\\|c |"
        );
    }

    #[test]
    fn format_all_rows_markdown_escapes_pipe_characters_in_headers() {
        let columns = vec!["col|name".to_string(), "ok".to_string()];
        let rows = vec![vec!["v".to_string(), "w".to_string()]];
        assert_eq!(
            format_all_rows_markdown(&columns, &rows),
            "| col\\|name | ok |\n| --- | --- |\n| v | w |"
        );
    }

    #[test]
    fn format_all_rows_markdown_replaces_newlines_with_br_tags() {
        let columns = vec!["note".to_string()];
        let rows = vec![vec!["line1\nline2\r\nline3".to_string()]];
        // `\r` collapses to a space and `\n` becomes `<br>`, so the
        // `\r\n` sequence in the middle renders as `<space><br>`.
        assert_eq!(
            format_all_rows_markdown(&columns, &rows),
            "| note |\n| --- |\n| line1<br>line2 <br>line3 |"
        );
    }

    #[test]
    fn format_all_rows_markdown_replaces_lone_newlines_with_br_tags() {
        let columns = vec!["note".to_string()];
        let rows = vec![vec!["line1\nline2".to_string()]];
        assert_eq!(
            format_all_rows_markdown(&columns, &rows),
            "| note |\n| --- |\n| line1<br>line2 |"
        );
    }

    #[test]
    fn format_all_rows_markdown_handles_single_column() {
        let columns = vec!["only".to_string()];
        let rows = vec![vec!["x".to_string()], vec!["y".to_string()]];
        assert_eq!(
            format_all_rows_markdown(&columns, &rows),
            "| only |\n| --- |\n| x |\n| y |"
        );
    }

    #[test]
    fn format_all_rows_markdown_handles_empty_columns() {
        let columns: Vec<String> = Vec::new();
        let rows: Vec<Vec<String>> = Vec::new();
        // Zero-column tables still render with the surrounding pipes —
        // the cells just collapse to an empty join, which leaves the
        // separator/row pipes intact.
        assert_eq!(format_all_rows_markdown(&columns, &rows), "|  |\n|  |");
    }

    #[test]
    fn format_all_rows_markdown_handles_empty_columns_with_rows() {
        let columns: Vec<String> = Vec::new();
        let rows = vec![vec![], vec![]];
        assert_eq!(
            format_all_rows_markdown(&columns, &rows),
            "|  |\n|  |\n|  |\n|  |"
        );
    }

    #[test]
    fn format_all_rows_markdown_renders_cleanly_into_gfm_table() {
        let columns = vec!["id".to_string(), "name".to_string(), "note".to_string()];
        let rows = vec![
            vec!["1".to_string(), "Ada".to_string(), "hello".to_string()],
            vec![
                "2".to_string(),
                "Bob".to_string(),
                "with | pipe".to_string(),
            ],
        ];
        let output = format_all_rows_markdown(&columns, &rows);

        let lines: Vec<&str> = output.split('\n').collect();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], "| id | name | note |");
        assert_eq!(lines[1], "| --- | --- | --- |");
        assert_eq!(lines[2], "| 1 | Ada | hello |");
        assert_eq!(lines[3], "| 2 | Bob | with \\| pipe |");

        for line in &lines {
            assert!(line.starts_with('|'), "line {line:?} missing leading pipe");
            assert!(line.ends_with('|'), "line {line:?} missing trailing pipe");
            // GFM separator must contain exactly `---` cells.
            if line.contains("---") {
                let cells: Vec<&str> = line.trim_matches('|').split("|").collect();
                assert_eq!(cells.len(), 3);
                for cell in cells {
                    assert_eq!(cell.trim(), "---");
                }
            }
        }
    }
}
