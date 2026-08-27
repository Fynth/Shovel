use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionClause {
    From,
    Column,
    Call,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionQuery {
    pub sql: String,
    pub cursor: usize,
    pub token: String,
    pub token_range: Range<usize>,
    pub clause: CompletionClause,
    pub dotted: Vec<String>,
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn parse_completion_query(sql: &str, cursor: usize) -> CompletionQuery {
    let cursor = clamp_to_char_boundary(sql, cursor);
    let (token, token_range) = scan_token(sql, cursor);
    let (dotted, dotted_start) = scan_dotted(sql, token_range.start);
    let clause = classify_clause(sql, token_range.start, dotted_start);

    CompletionQuery {
        sql: sql.to_string(),
        cursor,
        token,
        token_range,
        clause,
        dotted,
    }
}

fn clamp_to_char_boundary(sql: &str, cursor: usize) -> usize {
    let mut index = cursor.min(sql.len());
    while index > 0 && !sql.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn scan_token(sql: &str, cursor: usize) -> (String, Range<usize>) {
    let empty = (String::new(), cursor..cursor);
    if cursor == 0 {
        return empty;
    }

    let Some(prev) = sql[..cursor].chars().next_back() else {
        return empty;
    };
    if prev == '.' || !is_ident_continue(prev) {
        return empty;
    }

    let mut start = cursor;
    for (index, ch) in sql[..cursor].char_indices().rev() {
        if is_ident_continue(ch) {
            start = index;
        } else {
            break;
        }
    }

    let Some(first) = sql[start..cursor].chars().next() else {
        return empty;
    };
    if !is_ident_start(first) {
        return empty;
    }

    (sql[start..cursor].to_string(), start..cursor)
}

fn scan_dotted(sql: &str, token_start: usize) -> (Vec<String>, usize) {
    let mut parts = Vec::new();
    let mut pos = token_start;

    loop {
        if pos == 0 {
            break;
        }
        let Some(ch) = sql[..pos].chars().next_back() else {
            break;
        };
        if ch != '.' {
            break;
        }
        pos -= 1;

        let Some(prev) = sql[..pos].chars().next_back() else {
            break;
        };
        if !is_ident_continue(prev) {
            break;
        }

        let ident_end = pos;
        let mut ident_start = ident_end;
        for (index, ch) in sql[..ident_end].char_indices().rev() {
            if is_ident_continue(ch) {
                ident_start = index;
            } else {
                break;
            }
        }

        let Some(first) = sql[ident_start..ident_end].chars().next() else {
            break;
        };
        if !is_ident_start(first) {
            break;
        }

        parts.push(sql[ident_start..ident_end].to_string());
        pos = ident_start;
    }

    parts.reverse();
    let dotted_start = if parts.is_empty() { token_start } else { pos };
    (parts, dotted_start)
}

fn classify_clause(sql: &str, token_start: usize, dotted_start: usize) -> CompletionClause {
    if previous_non_space(sql, token_start) == Some('(') {
        return CompletionClause::Call;
    }
    last_clause_keyword(sql, dotted_start).unwrap_or(CompletionClause::Other)
}

fn previous_non_space(sql: &str, end: usize) -> Option<char> {
    sql[..end].chars().rev().find(|ch| !ch.is_whitespace())
}

fn last_clause_keyword(sql: &str, mut end: usize) -> Option<CompletionClause> {
    loop {
        while end > 0 {
            let ch = sql[..end].chars().next_back()?;
            if is_ident_continue(ch) {
                break;
            }
            end -= ch.len_utf8();
        }
        if end == 0 {
            return None;
        }

        let ident_end = end;
        let mut ident_start = ident_end;
        for (index, ch) in sql[..ident_end].char_indices().rev() {
            if is_ident_continue(ch) {
                ident_start = index;
            } else {
                break;
            }
        }

        let ident = &sql[ident_start..ident_end];
        if ident.starts_with(is_ident_start)
            && let Some(clause) = clause_for_keyword(ident)
        {
            return Some(clause);
        }

        end = ident_start;
        if end == 0 {
            return None;
        }
    }
}

fn clause_for_keyword(ident: &str) -> Option<CompletionClause> {
    if ident.eq_ignore_ascii_case("FROM")
        || ident.eq_ignore_ascii_case("JOIN")
        || ident.eq_ignore_ascii_case("INTO")
        || ident.eq_ignore_ascii_case("UPDATE")
        || ident.eq_ignore_ascii_case("TABLE")
    {
        Some(CompletionClause::From)
    } else if ident.eq_ignore_ascii_case("SELECT")
        || ident.eq_ignore_ascii_case("WHERE")
        || ident.eq_ignore_ascii_case("SET")
        || ident.eq_ignore_ascii_case("ON")
        || ident.eq_ignore_ascii_case("BY")
    {
        Some(CompletionClause::Column)
    } else {
        None
    }
}

fn is_ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_ident_continue(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_from_clause_after_from() {
        let q = parse_completion_query("SELECT * FROM us", 16);
        assert_eq!(q.clause, CompletionClause::From);
        assert_eq!(q.token, "us");
        assert!(q.dotted.is_empty());
    }

    #[test]
    fn parse_dotted_table_column() {
        let sql = "SELECT * FROM users.";
        let q = parse_completion_query(sql, sql.len());
        assert_eq!(q.dotted, vec!["users".to_string()]);
        assert!(q.token.is_empty());
        assert_eq!(q.token_range, sql.len()..sql.len());
    }

    #[test]
    fn parse_select_clause() {
        let sql = "SELECT nam";
        let q = parse_completion_query(sql, sql.len());
        assert_eq!(q.clause, CompletionClause::Column);
        assert_eq!(q.token, "nam");
    }

    #[test]
    fn parse_default_other() {
        let q = parse_completion_query("SEL", 3);
        assert_eq!(q.clause, CompletionClause::Other);
        assert_eq!(q.token, "SEL");
    }

    #[test]
    fn parse_call_clause_after_paren() {
        let sql = "SELECT count(";
        let q = parse_completion_query(sql, sql.len());
        assert_eq!(q.clause, CompletionClause::Call);
        assert!(q.token.is_empty());
    }

    #[test]
    fn parse_dotted_schema_and_table() {
        let sql = "SELECT * FROM public.users.";
        let q = parse_completion_query(sql, sql.len());
        assert_eq!(q.dotted, vec!["public".to_string(), "users".to_string()]);
        assert!(q.token.is_empty());
        assert_eq!(q.clause, CompletionClause::From);
    }
}
