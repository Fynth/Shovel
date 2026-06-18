//! Pure SQL statement splitter.
//!
//! Splits a multi-statement SQL script into a list of `Statement { sql, kind, line }`.
//!
//! Honors:
//! - line comments: `-- ...` until newline
//! - block comments: `/* ... */` (nested)
//! - single-quoted strings: `'...'` with `''` escape, plus PG `E'...'` and `U&'...'`
//! - double-quoted strings: `"..."` with `""` escape
//! - backtick identifiers: `` `...` `` (MySQL)
//! - dollar quoting: `$$ ... $$` and named `$tag$ ... $tag$` (PostgreSQL, ClickHouse)

use serde::{Deserialize, Serialize};

/// What kind of top-level operation this statement looks like.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatementKind {
    /// Begins with `SELECT`, `WITH`, `SHOW`, `DESCRIBE`, `EXPLAIN`, `PRAGMA`.
    Read,
    /// Anything else (INSERT, UPDATE, DELETE, DDL, …).
    Write,
    /// Empty / whitespace / comment-only after trimming.
    Empty,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Statement {
    /// 0-based index in the original script.
    pub index: usize,
    /// 0-based line in the original script where the statement starts.
    pub line: usize,
    /// Trimmed SQL text (the `;` terminator is NOT included).
    pub sql: String,
    pub kind: StatementKind,
}

impl Statement {
    pub fn is_empty(&self) -> bool {
        matches!(self.kind, StatementKind::Empty)
    }
}

/// Split `sql` into a `Vec<Statement>`. Empty statements are preserved so the
/// caller can decide what to do (DBeaver-style: skip them, but keep the count).
pub fn split_sql(sql: &str) -> Vec<Statement> {
    let mut out: Vec<Statement> = Vec::new();
    let mut current_start_line = 0usize;
    let mut buf = String::new();
    let mut current_index = 0usize;

    // Lexer state.
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut block_comment_depth: u32 = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut in_backtick = false;
    // Dollar-quoting: tag (without `$…$`) we are inside, or None.
    let mut dollar_tag: Option<String> = None;

    let bytes = sql.as_bytes();
    let mut i = 0usize;
    let mut line = 0usize;

    while i < bytes.len() {
        let c = bytes[i];

        // Newline handling: end of line comment, line counter.
        if c == b'\n' {
            line += 1;
            if in_line_comment {
                in_line_comment = false;
            }
            buf.push(c as char);
            i += 1;
            continue;
        }

        // In a line comment: everything is body, no escapes.
        if in_line_comment {
            buf.push(c as char);
            i += 1;
            continue;
        }

        // In a block comment.
        if in_block_comment {
            buf.push(c as char);
            if c == b'/' && bytes.get(i + 1) == Some(&b'*') {
                block_comment_depth += 1;
                buf.push('*');
                i += 2;
                continue;
            }
            if c == b'*' && bytes.get(i + 1) == Some(&b'/') {
                block_comment_depth -= 1;
                buf.push('/');
                i += 2;
                if block_comment_depth == 0 {
                    in_block_comment = false;
                }
                continue;
            }
            i += 1;
            continue;
        }

        // In a single-quoted string.
        if in_single_quote {
            buf.push(c as char);
            if c == b'\\' {
                // E-style backslash escape: copy the next byte literally.
                if let Some(nx) = bytes.get(i + 1) {
                    buf.push(*nx as char);
                    i += 2;
                    continue;
                }
            }
            if c == b'\'' {
                if bytes.get(i + 1) == Some(&b'\'') {
                    // SQL doubled-quote escape: '' inside '...'
                    buf.push('\'');
                    i += 2;
                    continue;
                }
                in_single_quote = false;
            }
            i += 1;
            continue;
        }

        // In a double-quoted identifier/string.
        if in_double_quote {
            buf.push(c as char);
            if c == b'"' && bytes.get(i + 1) == Some(&b'"') {
                buf.push('"');
                i += 2;
                continue;
            }
            if c == b'"' {
                in_double_quote = false;
            }
            i += 1;
            continue;
        }

        // In a backtick identifier (MySQL).
        if in_backtick {
            buf.push(c as char);
            if c == b'`' && bytes.get(i + 1) == Some(&b'`') {
                buf.push('`');
                i += 2;
                continue;
            }
            if c == b'`' {
                in_backtick = false;
            }
            i += 1;
            continue;
        }

        // In a dollar-quoted block.
        if let Some(tag) = dollar_tag.as_ref() {
            buf.push(c as char);
            if c == b'$' {
                // Try to close: $tag$ (or $$ for empty tag).
                let mut j = i + 1;
                let mut matched = true;
                for tc in tag.bytes() {
                    if bytes.get(j) != Some(&tc) {
                        matched = false;
                        break;
                    }
                    j += 1;
                }
                if matched && bytes.get(j) == Some(&b'$') {
                    buf.push_str(tag);
                    buf.push('$');
                    i = j + 1;
                    dollar_tag = None;
                    continue;
                }
            }
            i += 1;
            continue;
        }

        // Top-level (not in any string / comment / dollar-quote).

        // Line comment: `-- ...`
        if c == b'-' && bytes.get(i + 1) == Some(&b'-') {
            in_line_comment = true;
            buf.push('-');
            buf.push('-');
            i += 2;
            continue;
        }

        // Block comment: `/* ... */`, with nesting.
        if c == b'/' && bytes.get(i + 1) == Some(&b'*') {
            in_block_comment = true;
            block_comment_depth = 1;
            buf.push('/');
            buf.push('*');
            i += 2;
            continue;
        }

        // Single-quoted string.
        if c == b'\'' {
            in_single_quote = true;
            buf.push('\'');
            i += 1;
            continue;
        }
        // E'...' or U&'...' prefixes (PostgreSQL).
        if c == b'E' && bytes.get(i + 1) == Some(&b'\'') {
            in_single_quote = true;
            buf.push('E');
            buf.push('\'');
            i += 2;
            continue;
        }
        if c == b'U' && bytes.get(i + 1) == Some(&b'&') && bytes.get(i + 2) == Some(&b'\'') {
            in_single_quote = true;
            buf.push('U');
            buf.push('&');
            buf.push('\'');
            i += 3;
            continue;
        }

        if c == b'"' {
            in_double_quote = true;
            buf.push('"');
            i += 1;
            continue;
        }

        if c == b'`' {
            in_backtick = true;
            buf.push('`');
            i += 1;
            continue;
        }

        if c == b'$' {
            if let Some((tag, end)) = try_dollar_quote(bytes, i) {
                dollar_tag = Some(tag);
                // Push the opening tag (e.g. "$$" or "$tag$") verbatim.
                if let Ok(opening) = std::str::from_utf8(&bytes[i..end]) {
                    buf.push_str(opening);
                }
                i = end;
                continue;
            }
            buf.push('$');
            i += 1;
            continue;
        }

        // Statement terminator.
        if c == b';' {
            buf.push(';');
            i += 1;
            let trimmed = buf.trim().to_string();
            let kind = classify_kind(&trimmed);
            out.push(Statement {
                index: current_index,
                line: current_start_line,
                sql: strip_trailing_semicolon(&trimmed),
                kind,
            });
            current_index += 1;
            buf.clear();
            // After a `;` we are at the start of the next statement. Skip
            // any whitespace, but track the line of the first non-whitespace char.
            while i < bytes.len() {
                let c2 = bytes[i];
                if c2 == b'\n' {
                    line += 1;
                    i += 1;
                    continue;
                }
                if c2.is_ascii_whitespace() {
                    i += 1;
                    continue;
                }
                break;
            }
            current_start_line = line;
            continue;
        }

        // Any other byte.
        buf.push(c as char);
        i += 1;
    }

    // Flush trailing buffer (no terminating `;`).
    let trimmed = buf.trim().to_string();
    if !trimmed.is_empty() {
        let is_just_comment_or_ws = trimmed
            .lines()
            .all(|l| l.trim().is_empty() || l.trim_start().starts_with("--"));
        if !is_just_comment_or_ws {
            out.push(Statement {
                index: current_index,
                line: current_start_line,
                sql: strip_trailing_semicolon(&trimmed),
                kind: classify_kind(&trimmed),
            });
        }
    }

    out
}

/// Try to read a dollar tag starting at `pos` (which points at `$`).
/// Returns Some((tag, end_pos)) if it looks like a dollar-quote opener.
fn try_dollar_quote(bytes: &[u8], pos: usize) -> Option<(String, usize)> {
    if pos + 1 >= bytes.len() {
        return None;
    }
    let c = bytes[pos + 1];
    if c == b'$' {
        return Some((String::new(), pos + 2));
    }
    if c.is_ascii_alphanumeric() || c == b'_' {
        let mut j = pos + 1;
        while j < bytes.len() {
            let cj = bytes[j];
            if cj == b'$' {
                let tag = std::str::from_utf8(&bytes[pos + 1..j]).ok()?.to_string();
                return Some((tag, j + 1));
            }
            if !(cj.is_ascii_alphanumeric() || cj == b'_') {
                return None;
            }
            j += 1;
        }
        return None;
    }
    None
}

fn strip_trailing_semicolon(s: &str) -> String {
    let trimmed_end = s.trim_end();
    if let Some(rest) = trimmed_end.strip_suffix(';') {
        rest.trim_end().to_string()
    } else {
        trimmed_end.to_string()
    }
}

fn classify_kind(sql: &str) -> StatementKind {
    let s = sql.trim();
    if s.is_empty() {
        return StatementKind::Empty;
    }
    // Reuse the canonical `leading_sql_keyword` from the parent module. It
    // understands CTE shape (`WITH x AS (...) SELECT ...` → "with") and ignores
    // leading comments and parentheses.
    let keyword = super::leading_keyword(s);
    match keyword.as_deref() {
        Some("select" | "with" | "show" | "describe" | "desc" | "explain" | "pragma") =>
            StatementKind::Read,
        Some(_) => StatementKind::Write,
        None => StatementKind::Write,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(stmts: &[Statement]) -> Vec<StatementKind> {
        stmts.iter().map(|s| s.kind).collect()
    }

    #[test]
    fn splits_simple_two_statements() {
        let stmts = split_sql("SELECT 1; SELECT 2;");
        assert_eq!(stmts.len(), 2);
        assert_eq!(stmts[0].sql, "SELECT 1");
        assert_eq!(stmts[0].kind, StatementKind::Read);
        assert_eq!(stmts[1].sql, "SELECT 2");
        assert_eq!(stmts[1].kind, StatementKind::Read);
    }

    #[test]
    fn no_trailing_semicolon() {
        let stmts = split_sql("SELECT 1");
        assert_eq!(stmts.len(), 1);
        assert_eq!(stmts[0].sql, "SELECT 1");
    }

    #[test]
    fn strips_line_comments_between_statements() {
        let stmts = split_sql("-- header comment\nSELECT 1;\n-- between\nSELECT 2;");
        assert_eq!(stmts.len(), 2);
        assert_eq!(stmts[0].sql, "-- header comment\nSELECT 1");
        assert_eq!(stmts[1].sql, "-- between\nSELECT 2");
    }

    #[test]
    fn does_not_split_inside_single_quoted_semicolon() {
        let stmts = split_sql("INSERT INTO t (s) VALUES ('a;b;c');");
        assert_eq!(stmts.len(), 1);
        assert_eq!(stmts[0].sql, "INSERT INTO t (s) VALUES ('a;b;c')");
        assert_eq!(stmts[0].kind, StatementKind::Write);
    }

    #[test]
    fn does_not_split_inside_double_quoted_identifier() {
        let stmts = split_sql("SELECT \"weird;name\" FROM t;");
        assert_eq!(stmts.len(), 1);
        assert_eq!(stmts[0].kind, StatementKind::Read);
    }

    #[test]
    fn does_not_split_inside_backtick_identifier() {
        let stmts = split_sql("SELECT `weird;name` FROM t;");
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn handles_doubled_quote_escape() {
        let stmts = split_sql("SELECT 'it''s ok'; SELECT 2;");
        assert_eq!(stmts.len(), 2);
        assert_eq!(stmts[0].sql, "SELECT 'it''s ok'");
    }

    #[test]
    fn handles_dollar_quote_postgres() {
        let stmts = split_sql(
            "CREATE FUNCTION f() RETURNS void AS $$ BEGIN SELECT 1; END; $$ LANGUAGE plpgsql;",
        );
        assert_eq!(stmts.len(), 1);
        assert!(stmts[0].sql.contains("BEGIN SELECT 1; END;"));
        assert_eq!(stmts[0].kind, StatementKind::Write);
    }

    #[test]
    fn handles_named_dollar_quote() {
        let stmts = split_sql("DO $tag$ BEGIN SELECT 1; PERFORM 2; END $tag$; SELECT 3;");
        assert_eq!(stmts.len(), 2);
        assert!(stmts[0].sql.contains("BEGIN SELECT 1; PERFORM 2; END"));
        assert_eq!(stmts[1].sql, "SELECT 3");
    }

    #[test]
    fn handles_block_comment_with_nesting() {
        let stmts = split_sql("/* outer /* inner */ still comment */ SELECT 1; SELECT 2;");
        assert_eq!(stmts.len(), 2);
    }

    #[test]
    fn classifies_write_vs_read() {
        let stmts = split_sql(
            "SELECT 1; INSERT INTO t VALUES (1); UPDATE t SET x=1; DELETE FROM t; WITH x AS (SELECT 1) SELECT * FROM x;",
        );
        assert_eq!(
            kinds(&stmts),
            vec![
                StatementKind::Read,
                StatementKind::Write,
                StatementKind::Write,
                StatementKind::Write,
                StatementKind::Read,
            ]
        );
    }

    #[test]
    fn preserves_line_numbers() {
        let stmts = split_sql("SELECT 1;\n\nSELECT 2;\nSELECT 3;");
        assert_eq!(stmts[0].line, 0);
        assert_eq!(stmts[1].line, 2);
        assert_eq!(stmts[2].line, 3);
    }

    #[test]
    fn mixed_with_inline_comment() {
        let stmts = split_sql("SELECT 1 /* trick; here */ FROM t; SELECT 2;");
        assert_eq!(stmts.len(), 2);
        assert!(stmts[0].sql.contains("trick; here"));
    }

    #[test]
    fn semicolon_in_e_string() {
        let stmts = split_sql(r"SELECT E'foo\';bar'; SELECT 2;");
        assert_eq!(stmts.len(), 2);
        assert!(stmts[0].sql.contains(r"foo\';bar"));
    }

    #[test]
    fn multiple_statements_with_blank_lines_and_comments() {
        let stmts = split_sql("-- one\n\nSELECT 1;\n\n-- two\nSELECT 2;\n");
        assert_eq!(stmts.len(), 2);
    }

    #[test]
    fn case_insensitive_keyword_classification() {
        let stmts = split_sql("select 1; SeLeCt 2; insert into t values (1);");
        assert_eq!(
            kinds(&stmts),
            vec![
                StatementKind::Read,
                StatementKind::Read,
                StatementKind::Write
            ]
        );
    }

    #[test]
    fn explain_is_read() {
        let stmts = split_sql("EXPLAIN SELECT * FROM t;");
        assert_eq!(stmts[0].kind, StatementKind::Read);
    }
}
