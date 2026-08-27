#![cfg_attr(not(test), allow(dead_code))]

use std::ops::Range;

use models::DatabaseKind;

use super::query::CompletionQuery;

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionKind {
    Keyword,
    Schema,
    Table,
    View,
    Column,
    Function,
    Procedure,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionItem {
    pub label: String,
    pub detail: String,
    pub kind: CompletionKind,
    pub replace: Range<usize>,
}

const SHARED_KEYWORDS: &[&str] = &[
    "SELECT", "INSERT", "UPDATE", "DELETE", "FROM", "WHERE", "JOIN", "INNER", "LEFT", "RIGHT",
    "OUTER", "ON", "AS", "AND", "OR", "NOT", "IN", "IS", "NULL", "LIKE", "BETWEEN", "ORDER",
    "GROUP", "HAVING", "LIMIT", "OFFSET", "CREATE", "ALTER", "DROP", "TABLE", "INDEX", "VIEW",
    "INTO", "VALUES", "SET", "DISTINCT", "UNION", "ALL", "CASE", "WHEN", "THEN", "ELSE", "END",
    "WITH", "EXISTS",
];

const POSTGRES_KEYWORDS: &[&str] = &["ILIKE", "RETURNING", "LATERAL"];
const MYSQL_KEYWORDS: &[&str] = &["AUTO_INCREMENT", "ENGINE"];
const CLICKHOUSE_KEYWORDS: &[&str] = &["ENGINE", "PREWHERE", "SETTINGS", "FINAL"];
const SQLITE_KEYWORDS: &[&str] = &["AUTOINCREMENT", "PRAGMA"];

pub fn match_keyword_case(keyword: &str, typed: &str) -> String {
    if !typed.is_empty() && typed.chars().all(|ch| ch.is_ascii_lowercase()) {
        keyword.to_ascii_lowercase()
    } else if !typed.is_empty() && typed.chars().all(|ch| ch.is_ascii_uppercase()) {
        keyword.to_ascii_uppercase()
    } else {
        keyword.to_string()
    }
}

pub fn keyword_items(kind: DatabaseKind, query: &CompletionQuery) -> Vec<CompletionItem> {
    SHARED_KEYWORDS
        .iter()
        .copied()
        .chain(dialect_keywords(kind).iter().copied())
        .map(|keyword| CompletionItem {
            label: match_keyword_case(keyword, &query.token),
            detail: "keyword".to_string(),
            kind: CompletionKind::Keyword,
            replace: query.token_range.start..query.token_range.end,
        })
        .collect()
}

fn dialect_keywords(kind: DatabaseKind) -> &'static [&'static str] {
    match kind {
        DatabaseKind::Postgres => POSTGRES_KEYWORDS,
        DatabaseKind::MySql => MYSQL_KEYWORDS,
        DatabaseKind::ClickHouse => CLICKHOUSE_KEYWORDS,
        DatabaseKind::Sqlite => SQLITE_KEYWORDS,
    }
}

#[cfg(test)]
mod tests {
    use super::{super::query::parse_completion_query, *};

    #[test]
    fn keyword_case_follows_typed_prefix() {
        assert_eq!(match_keyword_case("SELECT", "sel"), "select");
        assert_eq!(match_keyword_case("SELECT", "SEL"), "SELECT");
        assert_eq!(match_keyword_case("SELECT", "Sel"), "SELECT");
        assert_eq!(match_keyword_case("SELECT", ""), "SELECT");
    }

    #[test]
    fn postgres_includes_ilike() {
        let sql = "SELECT * FROM t WHERE name IL";
        let q = parse_completion_query(sql, sql.len());
        let items = keyword_items(DatabaseKind::Postgres, &q);
        let ilike = items
            .iter()
            .find(|item| item.label.eq_ignore_ascii_case("ILIKE"))
            .expect("ILIKE");
        assert_eq!(ilike.detail, "keyword");
        assert_eq!(ilike.kind, CompletionKind::Keyword);
        assert_eq!(ilike.replace, q.token_range);
        assert!(
            items
                .iter()
                .any(|item| item.label.eq_ignore_ascii_case("SELECT")),
            "keyword_items must not filter by prefix"
        );
    }

    #[test]
    fn clickhouse_includes_engine() {
        let sql = "CREATE TABLE t EN";
        let q = parse_completion_query(sql, sql.len());
        let items = keyword_items(DatabaseKind::ClickHouse, &q);
        assert!(
            items
                .iter()
                .any(|item| item.label.eq_ignore_ascii_case("ENGINE"))
        );
    }
}
