#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExplorerNodeKind {
    Schema,
    Table,
    View,
    /// Materialized view (PostgreSQL `relkind = 'm'`). Queryable like a view.
    MaterializedView,
    /// Sequence (PostgreSQL `relkind = 'S'`).
    Sequence,
    /// Function (PostgreSQL `pg_proc prokind = 'f'`, MySQL routine type FUNCTION).
    Function,
    /// Procedure (PostgreSQL `pg_proc prokind = 'p'`, MySQL routine type PROCEDURE).
    Procedure,
    /// Trigger (PostgreSQL/MySQL/SQLite).
    Trigger,
}

impl ExplorerNodeKind {
    /// true, если объект можно открыть как табличный preview
    /// (SELECT *). Таблицы, представления и материализованные
    /// представления поддерживают выборку; остальные — нет.
    pub fn is_queryable(self) -> bool {
        matches!(
            self,
            ExplorerNodeKind::Table | ExplorerNodeKind::View | ExplorerNodeKind::MaterializedView
        )
    }

    /// Буква-иконка для строки дерева (как в DBeaver).
    pub fn tree_badge(self) -> &'static str {
        match self {
            ExplorerNodeKind::Schema => "",
            ExplorerNodeKind::Table => "T",
            ExplorerNodeKind::View => "V",
            ExplorerNodeKind::MaterializedView => "M",
            ExplorerNodeKind::Sequence => "S",
            ExplorerNodeKind::Function => "F",
            ExplorerNodeKind::Procedure => "P",
            ExplorerNodeKind::Trigger => "R",
        }
    }

    /// Человеческое название типа объекта.
    pub fn display_label(self) -> &'static str {
        match self {
            ExplorerNodeKind::Schema => "Schema",
            ExplorerNodeKind::Table => "Table",
            ExplorerNodeKind::View => "View",
            ExplorerNodeKind::MaterializedView => "Materialized View",
            ExplorerNodeKind::Sequence => "Sequence",
            ExplorerNodeKind::Function => "Function",
            ExplorerNodeKind::Procedure => "Procedure",
            ExplorerNodeKind::Trigger => "Trigger",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplorerNode {
    pub name: String,
    pub kind: ExplorerNodeKind,
    pub schema: Option<String>,
    pub qualified_name: String,
    /// Best-effort row-count estimate from DB statistics (e.g. `pg_class.reltuples`,
    /// `information_schema.TABLES.TABLE_ROWS`, ClickHouse `system.tables.total_rows`,
    /// SQLite `count(*)`). `None` when unknown or not cheaply available. Never
    /// blocks tree loading — a failed or expensive lookup yields `None`.
    pub row_count: Option<u64>,
    pub children: Vec<ExplorerNode>,
}

/// Структурированное описание внешнего ключа для ER-диаграммы.
/// В отличие от текстового `pg_get_constraintdef`, даёт явно поля
/// «откуда» и «куда», чтобы UI мог рисовать линии связей между таблицами.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableForeignKey {
    pub name: String,
    pub from_schema: String,
    pub from_table: String,
    pub from_column: String,
    pub to_schema: String,
    pub to_table: String,
    pub to_column: String,
}
