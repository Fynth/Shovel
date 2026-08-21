#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExplorerNodeKind {
    Schema,
    Table,
    View,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplorerNode {
    pub name: String,
    pub kind: ExplorerNodeKind,
    pub schema: Option<String>,
    pub qualified_name: String,
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
