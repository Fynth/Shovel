//! Example: format a SQL string with `query::format_sql`.
//!
//! This example demonstrates the simplest standalone use of the `query` crate
//! without any database connection. It reads a SQL string from a CLI argument,
//! formats it according to `SqlFormatSettings` defaults, and prints the result.
//!
//! Run with:
//!
//! ```text
//! cargo run --example format_sql -- "select id, name from users where active = true"
//! ```

use models::{DatabaseKind, SqlFormatSettings, SqlKeywordCase};
use query::format_sql;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let sql = args.get(1).cloned().unwrap_or_else(|| {
        "select id, name from users where active = true order by id".to_string()
    });

    // The default formatting rules used by the Shovel UI.
    let settings = SqlFormatSettings {
        keyword_case: SqlKeywordCase::Uppercase,
        indent_width: 2,
        lines_between_queries: 1,
        inline: false,
        joins_as_top_level: true,
        max_inline_block: 50,
        max_inline_arguments: Some(2),
        max_inline_top_level: Some(40),
    };

    let formatted = format_sql(Some(DatabaseKind::Postgres), &sql, &settings);
    println!("--- input ---");
    println!("{sql}");
    println!("--- formatted ---");
    println!("{formatted}");
}
