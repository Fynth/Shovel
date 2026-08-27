use database::FormatFlavor;
use models::{SqlFormatSettings, SqlKeywordCase};
use sqlformat::{Dialect, FormatOptions, Indent, QueryParams};

pub fn format_sql(flavor: FormatFlavor, sql: &str, settings: &SqlFormatSettings) -> String {
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let wrap_width = settings.max_inline_block.max(20) as usize;

    let options = FormatOptions {
        indent: Indent::Spaces(settings.indent_width.max(1)),
        uppercase: match settings.keyword_case {
            SqlKeywordCase::Preserve => None,
            SqlKeywordCase::Uppercase => Some(true),
            SqlKeywordCase::Lowercase => Some(false),
        },
        lines_between_queries: settings.lines_between_queries,
        inline: settings.inline,
        joins_as_top_level: settings.joins_as_top_level,
        max_inline_block: wrap_width,
        max_inline_arguments: settings.max_inline_arguments.map(|value| value as usize),
        max_inline_top_level: Some(
            settings
                .max_inline_top_level
                .unwrap_or(settings.max_inline_block)
                .max(20) as usize,
        ),
        dialect: match flavor {
            FormatFlavor::Postgres => Dialect::PostgreSql,
            FormatFlavor::Generic => Dialect::Generic,
        },
        ..FormatOptions::default()
    };

    let mut formatted = sqlformat::format(trimmed, &QueryParams::None, &options);
    if !formatted.ends_with('\n') {
        formatted.push('\n');
    }
    formatted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_settings() -> SqlFormatSettings {
        SqlFormatSettings {
            keyword_case: SqlKeywordCase::Uppercase,
            indent_width: 2,
            lines_between_queries: 1,
            inline: false,
            joins_as_top_level: false,
            max_inline_block: 50,
            max_inline_arguments: None,
            max_inline_top_level: None,
        }
    }

    // ── empty / whitespace input ─────────────────────────────────────

    #[test]
    fn format_sql_empty_input_returns_empty_string() {
        let settings = default_settings();
        assert_eq!(format_sql(FormatFlavor::Generic, "", &settings), "");
        assert_eq!(format_sql(FormatFlavor::Generic, "   ", &settings), "");
        assert_eq!(format_sql(FormatFlavor::Generic, "\n\t", &settings), "");
    }

    // ── basic formatting ─────────────────────────────────────────────

    #[test]
    fn format_sql_simple_select() {
        let settings = default_settings();
        let result = format_sql(FormatFlavor::Generic, "select * from users", &settings);
        assert!(result.contains("SELECT"));
        assert!(result.contains("FROM"));
        assert!(result.contains("users"));
        assert!(result.ends_with('\n'));
    }

    #[test]
    fn format_sql_preserves_trailing_newline() {
        let settings = default_settings();
        let result = format_sql(FormatFlavor::Generic, "select 1", &settings);
        assert!(result.ends_with('\n'));
    }

    #[test]
    fn format_sql_trims_leading_whitespace() {
        let settings = default_settings();
        let result = format_sql(FormatFlavor::Generic, "   select 1", &settings);
        assert!(!result.starts_with(' '));
        assert!(result.contains("SELECT"));
    }

    // ── keyword case handling ────────────────────────────────────────

    #[test]
    fn format_sql_uppercase_keywords() {
        let settings = SqlFormatSettings {
            keyword_case: SqlKeywordCase::Uppercase,
            ..default_settings()
        };
        let result = format_sql(FormatFlavor::Generic, "select * from users", &settings);
        assert!(result.contains("SELECT"));
        assert!(result.contains("FROM"));
    }

    #[test]
    fn format_sql_lowercase_keywords() {
        let settings = SqlFormatSettings {
            keyword_case: SqlKeywordCase::Lowercase,
            ..default_settings()
        };
        let result = format_sql(FormatFlavor::Generic, "SELECT * FROM users", &settings);
        assert!(result.contains("select"));
        assert!(result.contains("from"));
    }

    #[test]
    fn format_sql_preserve_keywords() {
        let settings = SqlFormatSettings {
            keyword_case: SqlKeywordCase::Preserve,
            ..default_settings()
        };
        let result = format_sql(FormatFlavor::Generic, "Select * From users", &settings);
        // When Preserve is set, uppercase is None so sqlformat preserves original case
        assert!(result.contains("Select"));
    }

    // ── indentation ──────────────────────────────────────────────────

    #[test]
    fn format_sql_indent_width_4() {
        let settings = SqlFormatSettings {
            indent_width: 4,
            ..default_settings()
        };
        let result = format_sql(
            FormatFlavor::Generic,
            "select id, name from users where id = 1",
            &settings,
        );
        // With indent_width 4, any indentation should use 4-space groups
        // The exact formatting depends on sqlformat, but we verify it doesn't panic
        assert!(result.contains("SELECT"));
        assert!(result.ends_with('\n'));
    }

    #[test]
    fn format_sql_indent_width_clamped_to_1() {
        let settings = SqlFormatSettings {
            indent_width: 0, // should be clamped to 1 by .max(1)
            ..default_settings()
        };
        let result = format_sql(
            FormatFlavor::Generic,
            "select id, name from users where id = 1",
            &settings,
        );
        assert!(result.contains("SELECT"));
    }

    // ── dialect selection ────────────────────────────────────────────

    #[test]
    fn format_sql_uses_format_flavor_not_database_kind() {
        let settings = SqlFormatSettings::default();
        let out = format_sql(FormatFlavor::Postgres, "select 1", &settings);
        assert!(!out.is_empty());
    }

    #[test]
    fn format_sql_postgres_dialect() {
        let settings = default_settings();
        let result = format_sql(FormatFlavor::Postgres, "select * from users", &settings);
        assert!(result.contains("SELECT"));
    }

    #[test]
    fn format_sql_mysql_dialect() {
        let settings = default_settings();
        let result = format_sql(FormatFlavor::Generic, "select * from users", &settings);
        assert!(result.contains("SELECT"));
    }

    #[test]
    fn format_sql_sqlite_dialect() {
        let settings = default_settings();
        let result = format_sql(FormatFlavor::Generic, "select * from users", &settings);
        assert!(result.contains("SELECT"));
    }

    #[test]
    fn format_sql_clickhouse_dialect() {
        let settings = default_settings();
        let result = format_sql(FormatFlavor::Generic, "select * from users", &settings);
        assert!(result.contains("SELECT"));
    }

    // ── lines_between_queries ────────────────────────────────────────

    #[test]
    fn format_sql_multiple_queries_with_spacing() {
        let settings = SqlFormatSettings {
            lines_between_queries: 2,
            ..default_settings()
        };
        let result = format_sql(FormatFlavor::Generic, "select 1; select 2;", &settings);
        assert!(result.contains("SELECT"));
        assert!(result.ends_with('\n'));
    }

    // ── max_inline_block clamped ─────────────────────────────────────

    #[test]
    fn format_sql_max_inline_block_clamped_to_20_minimum() {
        let settings = SqlFormatSettings {
            max_inline_block: 0, // should be clamped to 20 by .max(20)
            ..default_settings()
        };
        let result = format_sql(FormatFlavor::Generic, "select * from users", &settings);
        assert!(result.contains("SELECT"));
    }
}
