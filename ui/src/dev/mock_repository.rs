//! Dev-only [`MockDatabaseRepository`] for UI work that does not need a
//! running database.
//!
//! The mock produces a stable, hand-crafted snapshot of fake
//! [`models::ExplorerConnectionSection`]s and [`models::QueryOutput`]
//! results. It is gated behind `debug_assertions` so release builds
//! cannot accidentally expose dev-only behavior.
//!
//! The repository is intentionally a plain data struct (no Dioxus /
//! signals) so its helpers stay unit-testable and easy to swap for the
//! real `services::load_connection_tree` + `services::execute_query`
//! pair. The UI surface in [`crate::dev::install_mock_explorer`] is the
//! only piece that touches app state.

use models::{
    EditableTableContext,
    ExplorerNode,
    ExplorerNodeKind,
    QueryOutput,
    QueryPage,
    TablePreviewSource,
};

/// Stable identity key for the in-memory SQLite session the UI creates
/// when the developer activates mock mode. We use `:memory:` as the path
/// so the real `SqliteDriver::connect` returns a fresh in-memory pool
/// (see `driver-sqlite::SqliteDriver::connect`); the mock explorer cache
/// then overlays the real (empty) tree with hand-crafted nodes, and the
/// workspace's preview / query actions short-circuit to
/// [`MockDatabaseRepository`] so the in-memory pool never has to
/// actually answer a SQL statement.
pub const MOCK_CONNECTION_IDENTITY_KEY: &str = "sqlite::memory:";
pub const MOCK_CONNECTION_DISPLAY_NAME: &str = "Mock (dev)";

/// Repository that returns hand-crafted fake data shaped to match the
/// production [`services`] facade's outputs.
///
/// The struct is a zero-sized marker — the data is fully derived from
/// module-level constants so two instances always produce the same
/// tree. Constructing one is free.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MockDatabaseRepository;

impl MockDatabaseRepository {
    pub fn new() -> Self {
        Self
    }

    /// `Vec<ExplorerConnectionSection>` mimicking the real
    /// `services::load_connection_tree` output, but with a small
    /// hand-built `public` schema.
    ///
    /// The shape (one section per session, `nodes: Vec<ExplorerNode>`)
    /// matches what the explorer panel already renders, so the mock
    /// drops in without changing tree-view code.
    pub fn tree_sections(
        &self,
        session_id: u64,
    ) -> Vec<crate::screens::workspace::ExplorerConnectionSection> {
        use crate::screens::workspace::ExplorerConnectionSection;

        vec![ExplorerConnectionSection {
            session_id,
            name: MOCK_CONNECTION_DISPLAY_NAME.to_string(),
            kind_label: "SQLite (mock)".to_string(),
            status: "Ready (mock data)".to_string(),
            is_active: true,
            nodes: mock_schema_nodes(),
        }]
    }

    /// Returns a [`QueryOutput`] for a [`TablePreviewSource`] that names
    /// one of the mock tables (`public.users`, `public.orders`,
    /// `public.products`). Returns `None` for unknown sources so the
    /// caller can fall through to the real preview path.
    pub fn preview_for(&self, source: &TablePreviewSource) -> Option<QueryOutput> {
        let name = source.table_name.to_ascii_lowercase();
        let rows = match name.as_str() {
            "users" => sample_users(),
            "orders" => sample_orders(),
            "products" => sample_products(),
            _ => return None,
        };
        let columns = column_defs_for(&name)
            .into_iter()
            .map(|(label, _)| label.to_string())
            .collect::<Vec<_>>();
        let schema = source
            .schema
            .clone()
            .unwrap_or_else(|| "public".to_string());
        let qualified = format!("{schema}.{name}");
        let row_locators = rows
            .iter()
            .filter_map(|row| row.first().cloned())
            .collect::<Vec<_>>();

        let page = QueryPage {
            columns,
            rows: rows.clone(),
            editable: Some(EditableTableContext {
                source: TablePreviewSource {
                    schema: Some(schema),
                    table_name: name,
                    qualified_name: qualified,
                },
                row_locators,
            }),
            offset: 0,
            page_size: rows.len() as u32,
            has_previous: false,
            has_next: false,
        };
        Some(QueryOutput::Table(page))
    }

    /// `Vec<Vec<String>>` of pre-baked rows for a known mock table name.
    /// Returns an empty vector for unknown names.
    #[allow(dead_code)]
    pub fn rows_for_table(&self, table_name: &str) -> Vec<Vec<String>> {
        match table_name.to_ascii_lowercase().as_str() {
            "users" => sample_users(),
            "orders" => sample_orders(),
            "products" => sample_products(),
            _ => Vec::new(),
        }
    }

    /// Best-effort [`QueryOutput`] for an ad-hoc SQL string. We only
    /// handle the simple `select * from <known_table>` shape that the
    /// explorer double-click path already generates; everything else
    /// returns `None` so the caller can fall through to the real
    /// query executor.
    pub fn query_for(&self, sql: &str) -> Option<QueryOutput> {
        let trimmed = sql.trim().trim_end_matches(';').trim();
        let lower = trimmed.to_ascii_lowercase();
        let prefix = "select * from";
        if !lower.starts_with(prefix) {
            return None;
        }
        let rest = trimmed[prefix.len()..].trim();
        // Strip schema prefix if present (`public.users` -> `users`).
        let bare = rest.rsplit('.').next().unwrap_or(rest).trim_matches('"');
        let source = TablePreviewSource {
            schema: Some("public".to_string()),
            table_name: bare.to_string(),
            qualified_name: format!("public.{bare}"),
        };
        self.preview_for(&source)
    }
}

/// (label, kind) column list for a known mock table. The kind is
/// retained for future use (e.g. richer cell rendering); the preview
/// builder only needs the label today.
fn column_defs_for(table: &str) -> Vec<(&'static str, &'static str)> {
    match table {
        "users" => vec![
            ("id", "integer"),
            ("email", "text"),
            ("name", "text"),
            ("created_at", "timestamptz"),
        ],
        "orders" => vec![
            ("id", "integer"),
            ("user_id", "integer"),
            ("total_cents", "integer"),
            ("status", "text"),
            ("placed_at", "timestamptz"),
        ],
        "products" => vec![
            ("id", "integer"),
            ("sku", "text"),
            ("name", "text"),
            ("price_cents", "integer"),
            ("stock", "integer"),
        ],
        _ => Vec::new(),
    }
}

fn sample_users() -> Vec<Vec<String>> {
    vec![
        vec![
            "1".into(),
            "ada@example.com".into(),
            "Ada Lovelace".into(),
            "2024-01-12 09:00".into(),
        ],
        vec![
            "2".into(),
            "alan@example.com".into(),
            "Alan Turing".into(),
            "2024-02-04 14:30".into(),
        ],
        vec![
            "3".into(),
            "grace@example.com".into(),
            "Grace Hopper".into(),
            "2024-02-19 10:15".into(),
        ],
        vec![
            "4".into(),
            "edsger@example.com".into(),
            "Edsger Dijkstra".into(),
            "2024-03-02 16:45".into(),
        ],
        vec![
            "5".into(),
            "donald@example.com".into(),
            "Donald Knuth".into(),
            "2024-03-22 11:20".into(),
        ],
    ]
}

fn sample_orders() -> Vec<Vec<String>> {
    vec![
        vec![
            "100".into(),
            "1".into(),
            "1299".into(),
            "paid".into(),
            "2024-04-01 12:00".into(),
        ],
        vec![
            "101".into(),
            "2".into(),
            "4599".into(),
            "paid".into(),
            "2024-04-02 08:30".into(),
        ],
        vec![
            "102".into(),
            "1".into(),
            "899".into(),
            "refunded".into(),
            "2024-04-03 17:10".into(),
        ],
        vec![
            "103".into(),
            "3".into(),
            "2450".into(),
            "paid".into(),
            "2024-04-04 09:55".into(),
        ],
    ]
}

fn sample_products() -> Vec<Vec<String>> {
    vec![
        vec![
            "11".into(),
            "BK-001".into(),
            "Crafting Interpreters".into(),
            "3999".into(),
            "42".into(),
        ],
        vec![
            "12".into(),
            "BK-002".into(),
            "The Pragmatic Programmer".into(),
            "3499".into(),
            "17".into(),
        ],
        vec![
            "13".into(),
            "BK-003".into(),
            "Structure and Interpretation of Computer Programs".into(),
            "5999".into(),
            "8".into(),
        ],
        vec![
            "14".into(),
            "BK-004".into(),
            "Database Internals".into(),
            "4299".into(),
            "23".into(),
        ],
    ]
}

fn mock_schema_nodes() -> Vec<ExplorerNode> {
    vec![ExplorerNode {
        name: "public".to_string(),
        kind: ExplorerNodeKind::Schema,
        schema: None,
        qualified_name: "public".to_string(),
        row_count: None,
        children: vec![
            ExplorerNode {
                name: "users".to_string(),
                kind: ExplorerNodeKind::Table,
                schema: Some("public".to_string()),
                qualified_name: "public.users".to_string(),
                row_count: Some(5),
                children: user_columns(),
            },
            ExplorerNode {
                name: "orders".to_string(),
                kind: ExplorerNodeKind::Table,
                schema: Some("public".to_string()),
                qualified_name: "public.orders".to_string(),
                row_count: Some(4),
                children: order_columns(),
            },
            ExplorerNode {
                name: "products".to_string(),
                kind: ExplorerNodeKind::Table,
                schema: Some("public".to_string()),
                qualified_name: "public.products".to_string(),
                row_count: Some(4),
                children: product_columns(),
            },
            ExplorerNode {
                name: "v_active_users".to_string(),
                kind: ExplorerNodeKind::View,
                schema: Some("public".to_string()),
                qualified_name: "public.v_active_users".to_string(),
                row_count: None,
                children: Vec::new(),
            },
            ExplorerNode {
                name: "user_id_seq".to_string(),
                kind: ExplorerNodeKind::Sequence,
                schema: Some("public".to_string()),
                qualified_name: "public.user_id_seq".to_string(),
                row_count: None,
                children: Vec::new(),
            },
        ],
    }]
}

fn user_columns() -> Vec<ExplorerNode> {
    ["id", "email", "name", "created_at"]
        .into_iter()
        .map(|col| ExplorerNode {
            name: col.to_string(),
            kind: ExplorerNodeKind::Column,
            schema: Some("public".to_string()),
            qualified_name: format!("public.users.{col}"),
            row_count: None,
            children: Vec::new(),
        })
        .collect()
}

fn order_columns() -> Vec<ExplorerNode> {
    ["id", "user_id", "total_cents", "status", "placed_at"]
        .into_iter()
        .map(|col| ExplorerNode {
            name: col.to_string(),
            kind: ExplorerNodeKind::Column,
            schema: Some("public".to_string()),
            qualified_name: format!("public.orders.{col}"),
            row_count: None,
            children: Vec::new(),
        })
        .collect()
}

fn product_columns() -> Vec<ExplorerNode> {
    ["id", "sku", "name", "price_cents", "stock"]
        .into_iter()
        .map(|col| ExplorerNode {
            name: col.to_string(),
            kind: ExplorerNodeKind::Column,
            schema: Some("public".to_string()),
            qualified_name: format!("public.products.{col}"),
            row_count: None,
            children: Vec::new(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_key_is_stable() {
        assert_eq!(MOCK_CONNECTION_IDENTITY_KEY, "sqlite::memory:");
        assert_eq!(MOCK_CONNECTION_DISPLAY_NAME, "Mock (dev)");
    }

    #[test]
    fn tree_sections_produces_one_section_with_public_schema() {
        let repo = MockDatabaseRepository::new();
        let sections = repo.tree_sections(42);
        assert_eq!(sections.len(), 1);
        let section = &sections[0];
        assert_eq!(section.session_id, 42);
        assert!(section.is_active);
        assert_eq!(section.nodes.len(), 1);
        assert_eq!(section.nodes[0].name, "public");
        assert_eq!(section.nodes[0].kind, ExplorerNodeKind::Schema);
        // public schema has 5 children: 3 tables, 1 view, 1 sequence
        assert_eq!(section.nodes[0].children.len(), 5);
    }

    #[test]
    fn tree_includes_known_tables_view_and_sequence() {
        let repo = MockDatabaseRepository::new();
        let section = &repo.tree_sections(1)[0];
        let kinds: Vec<ExplorerNodeKind> =
            section.nodes[0].children.iter().map(|c| c.kind).collect();
        assert!(kinds.iter().any(|k| matches!(k, ExplorerNodeKind::Table)));
        assert!(kinds.iter().any(|k| matches!(k, ExplorerNodeKind::View)));
        assert!(
            kinds
                .iter()
                .any(|k| matches!(k, ExplorerNodeKind::Sequence))
        );
    }

    #[test]
    fn preview_for_unknown_source_returns_none() {
        let repo = MockDatabaseRepository::new();
        let source = TablePreviewSource {
            schema: Some("public".to_string()),
            table_name: "nope".to_string(),
            qualified_name: "public.nope".to_string(),
        };
        assert!(repo.preview_for(&source).is_none());
    }

    #[test]
    fn preview_for_users_returns_editable_table() {
        let repo = MockDatabaseRepository::new();
        let source = TablePreviewSource {
            schema: Some("public".to_string()),
            table_name: "users".to_string(),
            qualified_name: "public.users".to_string(),
        };
        let output = repo.preview_for(&source).expect("users preview");
        let QueryOutput::Table(page) = output else {
            panic!("expected QueryOutput::Table, got AffectedRows");
        };
        assert_eq!(page.columns, vec!["id", "email", "name", "created_at"]);
        assert_eq!(page.rows.len(), 5);
        // editable context is present and locators align with the id column
        let editable = page.editable.expect("editable context");
        assert_eq!(editable.row_locators.len(), 5);
        assert_eq!(editable.row_locators[0], page.rows[0][0]);
    }

    #[test]
    fn preview_for_orders_and_products_have_rows() {
        let repo = MockDatabaseRepository::new();
        let orders = TablePreviewSource {
            schema: Some("public".to_string()),
            table_name: "orders".to_string(),
            qualified_name: "public.orders".to_string(),
        };
        let products = TablePreviewSource {
            schema: Some("public".to_string()),
            table_name: "products".to_string(),
            qualified_name: "public.products".to_string(),
        };
        assert!(repo.preview_for(&orders).is_some());
        assert!(repo.preview_for(&products).is_some());
    }

    #[test]
    fn rows_for_table_unknown_returns_empty() {
        let repo = MockDatabaseRepository::new();
        assert!(repo.rows_for_table("nope").is_empty());
    }

    #[test]
    fn rows_for_users_match_preview() {
        let repo = MockDatabaseRepository::new();
        let rows = repo.rows_for_table("users");
        assert_eq!(rows.len(), 5);
        // First id is "1" — matches the editable locator at index 0.
        assert_eq!(rows[0][0], "1");
    }

    #[test]
    fn table_nodes_carry_qualified_name() {
        let repo = MockDatabaseRepository::new();
        let section = &repo.tree_sections(7)[0];
        let public = &section.nodes[0];
        for table in public
            .children
            .iter()
            .filter(|c| matches!(c.kind, ExplorerNodeKind::Table))
        {
            assert!(table.qualified_name.starts_with("public."));
            assert_eq!(table.schema.as_deref(), Some("public"));
        }
    }
}
