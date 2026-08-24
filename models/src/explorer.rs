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
    /// Column child of a table or view. Only present in the tree when
    /// the explorer backend opts to populate column children; the
    /// render path is gated by [`models::ExplorerViewSettings::show_columns`]
    /// so legacy tree-loaders (which never populate columns) stay
    /// unchanged when the toggle is off.
    Column,
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

    /// Буква-иконка для строки дерева (как в DBeaver). Сохранено для
    /// обратной совместимости с тестами и мест, где текстовый
    /// fallback всё ещё нужен; новый рендерер использует
    /// [`ExplorerNodeKind::badge_class`] + [`ObjectIcon`].
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
            ExplorerNodeKind::Column => "C",
        }
    }

    /// Стабильный kebab-case-идентификатор типа для CSS-модификатора
    /// (`tree__object-badge--table` и т.п.). Используется рендерером,
    /// чтобы подсветить таблицу/вью/etc. разными цветовыми токенами
    /// при наведении и в выбранном состоянии.
    pub fn badge_class(self) -> &'static str {
        match self {
            ExplorerNodeKind::Schema => "schema",
            ExplorerNodeKind::Table => "table",
            ExplorerNodeKind::View => "view",
            ExplorerNodeKind::MaterializedView => "materialized-view",
            ExplorerNodeKind::Sequence => "sequence",
            ExplorerNodeKind::Function => "function",
            ExplorerNodeKind::Procedure => "procedure",
            ExplorerNodeKind::Trigger => "trigger",
            ExplorerNodeKind::Column => "column",
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
            ExplorerNodeKind::Column => "Column",
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
