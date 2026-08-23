mod tree_views;

// `create_table_modal` and `duplicate_table_modal` are mounted both inline
// (legacy) and from the native window roots in `crate::windows`. Their
// `pub` surfaces (the modal component + the `*Target` data structs) need
// to be reachable across the `explorer` module boundary.
pub mod create_table_modal;
pub mod duplicate_table_modal;

use crate::{
    app_state::{APP_READ_ONLY_MODE, APP_STATE, APP_THEME, activate_session, remove_session},
    screens::workspace::components::{ActionIcon, IconButton},
};
use dioxus::prelude::*;
use models::{DatabaseKind, ExplorerNode, ExplorerNodeKind, QueryTabState};

use create_table_modal::CreateTableTarget;

#[derive(Clone, Debug, PartialEq)]
pub struct ExplorerConnectionSection {
    pub session_id: u64,
    pub name: String,
    pub kind_label: String,
    pub status: String,
    pub is_active: bool,
    pub nodes: Vec<ExplorerNode>,
}

#[component]
pub fn SidebarConnectionTree(
    sections: Vec<ExplorerConnectionSection>,
    tree_reload: Signal<u64>,
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: Signal<u64>,
    next_tab_id: Signal<u64>,
) -> Element {
    let selected_node = use_signal(String::new);
    let mut filter_query = use_signal(String::new);
    let query = filter_query();
    let active_create_target = active_create_table_target(&sections);
    let filtered_sections = filter_connection_sections(&sections, &query);
    let entity_count = filtered_sections
        .iter()
        .map(|section| count_objects(&section.nodes))
        .sum::<usize>();
    let read_only_mode = APP_READ_ONLY_MODE();

    rsx! {
        div { class: "tree",
            div {
                class: "tree__header",
                div {
                    class: "tree__header-copy",
                    span { class: "tree__header-label", "Entities" }
                    span { class: "tree__header-count", "{entity_count}" }
                }
                div {
                    class: "tree__header-actions",
                    IconButton {
                        icon: ActionIcon::CreateTable,
                        label: if read_only_mode {
                            "Create table is blocked by read-only mode".to_string()
                        } else {
                            "Create table".to_string()
                        },
                        small: true,
                        disabled: active_create_target.is_none() || read_only_mode,
                        onclick: {
                            let target = active_create_target.clone();
                            let mut tree_reload = tree_reload;
                            move |_| {
                                let Some(target) = target.clone() else {
                                    return;
                                };
                                let connection = crate::app_state::session_connection(target.session_id);
                                let (bridge, mut rx) = crate::windows::create_table_bridge();
                                spawn(async move {
                                    while rx.recv().await.is_some() {
                                        tree_reload += 1;
                                    }
                                });
                                crate::windows::open_create_table_window(
                                    bridge,
                                    target,
                                    connection,
                                    read_only_mode,
                                    APP_THEME(),
                                );
                            }
                        },
                    }
                }
            }

            if sections.is_empty() {
                div {
                    class: "tree__body",
                    p { class: "empty-state", "No active connections." }
                }
            } else {
                div {
                    class: "tree__filter",
                    input {
                        class: "input tree__filter-input",
                        value: "{query}",
                        placeholder: "Filter entities",
                        oninput: move |event| filter_query.set(event.value()),
                    }
                }

                div {
                    class: "tree__body",
                    if filtered_sections.is_empty() {
                        p { class: "empty-state", "No matching tables or views." }
                    } else {
                        for section in filtered_sections {
                            tree_views::ExplorerConnectionView {
                                section,
                                tree_reload,
                                tabs,
                                active_tab_id,
                                next_tab_id,
                                selected_node,
                            }
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Shared helpers (used by sub-modules)
// ---------------------------------------------------------------------------

fn active_create_table_target(sections: &[ExplorerConnectionSection]) -> Option<CreateTableTarget> {
    let section = sections
        .iter()
        .find(|section| section.is_active)
        .or_else(|| sections.first())?;
    let kind = APP_STATE.read().session(section.session_id)?.kind;
    let mut schemas = section
        .nodes
        .iter()
        .filter(|node| node.kind == ExplorerNodeKind::Schema)
        .map(|node| node.name.clone())
        .collect::<Vec<_>>();
    schemas.sort();
    schemas.dedup();

    if schemas.is_empty() {
        schemas.push(default_schema_name(kind));
    }

    Some(CreateTableTarget {
        session_id: section.session_id,
        connection_name: section.name.clone(),
        kind,
        schemas,
    })
}

pub(super) fn count_objects(nodes: &[ExplorerNode]) -> usize {
    nodes.iter().map(|node| node.children.len()).sum()
}

/// Сгруппированные дочерние объекты схемы для дерева (как в DBeaver):
/// каждая группа отображается отдельной секцией. Порядок групп
/// фиксирован и соответствует значимости.
pub(super) struct ExplorerChildGroups {
    pub tables: Vec<ExplorerNode>,
    pub views: Vec<ExplorerNode>,
    pub materialized_views: Vec<ExplorerNode>,
    pub sequences: Vec<ExplorerNode>,
    pub functions: Vec<ExplorerNode>,
    pub procedures: Vec<ExplorerNode>,
    pub triggers: Vec<ExplorerNode>,
}

impl ExplorerChildGroups {
    /// Группы в порядке отображения, пропуская пустые. Возвращает
    /// (заголовок группы, узлы) для рендера.
    pub fn non_empty(&self) -> Vec<(&'static str, &Vec<ExplorerNode>)> {
        let mut out = Vec::new();
        if !self.tables.is_empty() {
            out.push(("Tables", &self.tables));
        }
        if !self.views.is_empty() {
            out.push(("Views", &self.views));
        }
        if !self.materialized_views.is_empty() {
            out.push(("Materialized Views", &self.materialized_views));
        }
        if !self.sequences.is_empty() {
            out.push(("Sequences", &self.sequences));
        }
        if !self.functions.is_empty() {
            out.push(("Functions", &self.functions));
        }
        if !self.procedures.is_empty() {
            out.push(("Procedures", &self.procedures));
        }
        if !self.triggers.is_empty() {
            out.push(("Triggers", &self.triggers));
        }
        out
    }

    pub fn total(&self) -> usize {
        self.tables.len()
            + self.views.len()
            + self.materialized_views.len()
            + self.sequences.len()
            + self.functions.len()
            + self.procedures.len()
            + self.triggers.len()
    }
}

pub(super) fn split_children(children: &[ExplorerNode]) -> ExplorerChildGroups {
    let mut groups = ExplorerChildGroups {
        tables: Vec::new(),
        views: Vec::new(),
        materialized_views: Vec::new(),
        sequences: Vec::new(),
        functions: Vec::new(),
        procedures: Vec::new(),
        triggers: Vec::new(),
    };

    for child in children {
        match child.kind {
            ExplorerNodeKind::Table => groups.tables.push(child.clone()),
            ExplorerNodeKind::View => groups.views.push(child.clone()),
            ExplorerNodeKind::MaterializedView => groups.materialized_views.push(child.clone()),
            ExplorerNodeKind::Sequence => groups.sequences.push(child.clone()),
            ExplorerNodeKind::Function => groups.functions.push(child.clone()),
            ExplorerNodeKind::Procedure => groups.procedures.push(child.clone()),
            ExplorerNodeKind::Trigger => groups.triggers.push(child.clone()),
            ExplorerNodeKind::Schema => {}
        }
    }

    let sort_group =
        |vec: &mut Vec<ExplorerNode>| vec.sort_by(|left, right| left.name.cmp(&right.name));
    sort_group(&mut groups.tables);
    sort_group(&mut groups.views);
    sort_group(&mut groups.materialized_views);
    sort_group(&mut groups.sequences);
    sort_group(&mut groups.functions);
    sort_group(&mut groups.procedures);
    sort_group(&mut groups.triggers);

    groups
}

pub(super) fn disconnect_session(
    mut tabs: Signal<Vec<QueryTabState>>,
    mut active_tab_id: Signal<u64>,
    session_id: u64,
) {
    tabs.with_mut(|all_tabs| all_tabs.retain(|tab| tab.session_id != session_id));
    if let Some(first_tab) = tabs.read().first() {
        active_tab_id.set(first_tab.id);
        activate_session(first_tab.session_id);
    } else {
        active_tab_id.set(0);
    }
    remove_session(session_id);
}

pub(super) fn default_schema_name(kind: DatabaseKind) -> String {
    match kind {
        DatabaseKind::Sqlite => "main".to_string(),
        DatabaseKind::Postgres => "public".to_string(),
        DatabaseKind::MySql => "mysql".to_string(),
        DatabaseKind::ClickHouse => "default".to_string(),
    }
}

pub(super) fn quote_sql_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

pub(super) fn quote_clickhouse_identifier(identifier: &str) -> String {
    format!("`{}`", identifier.replace('`', "``"))
}

pub(super) fn quoted_table_name_preview(
    kind: DatabaseKind,
    schema: Option<&str>,
    table_name: &str,
) -> String {
    match kind {
        DatabaseKind::Sqlite | DatabaseKind::Postgres => match schema {
            Some(schema) => format!(
                "{}.{}",
                quote_sql_identifier(schema),
                quote_sql_identifier(table_name)
            ),
            None => quote_sql_identifier(table_name),
        },
        DatabaseKind::MySql => match schema {
            Some(schema) => format!(
                "{}.{}",
                quote_clickhouse_identifier(schema),
                quote_clickhouse_identifier(table_name)
            ),
            None => quote_clickhouse_identifier(table_name),
        },
        DatabaseKind::ClickHouse => {
            let schema = schema.unwrap_or("default");
            format!(
                "{}.{}",
                quote_clickhouse_identifier(schema),
                quote_clickhouse_identifier(table_name)
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Filter helpers
// ---------------------------------------------------------------------------

fn filter_connection_sections(
    sections: &[ExplorerConnectionSection],
    query: &str,
) -> Vec<ExplorerConnectionSection> {
    let query = query.trim();
    if query.is_empty() {
        return sections.to_vec();
    }

    let normalized = query.to_ascii_lowercase();
    sections
        .iter()
        .filter_map(|section| {
            let section_matches = matches_query(&section.name, &normalized)
                || matches_query(&section.kind_label, &normalized);
            let nodes = if section_matches {
                section.nodes.clone()
            } else {
                filter_nodes(&section.nodes, &normalized)
            };

            if section_matches || !nodes.is_empty() {
                let mut section = section.clone();
                section.nodes = nodes;
                Some(section)
            } else {
                None
            }
        })
        .collect()
}

fn filter_nodes(nodes: &[ExplorerNode], query: &str) -> Vec<ExplorerNode> {
    nodes
        .iter()
        .filter_map(|node| filter_node(node, query))
        .collect()
}

fn filter_node(node: &ExplorerNode, query: &str) -> Option<ExplorerNode> {
    match node.kind {
        ExplorerNodeKind::Schema => {
            let schema_matches = matches_query(&node.name, query);
            let mut filtered = node.clone();
            filtered.children = if schema_matches {
                node.children.clone()
            } else {
                filter_nodes(&node.children, query)
            };

            if schema_matches || !filtered.children.is_empty() {
                Some(filtered)
            } else {
                None
            }
        }
        ExplorerNodeKind::Table
        | ExplorerNodeKind::View
        | ExplorerNodeKind::MaterializedView
        | ExplorerNodeKind::Sequence
        | ExplorerNodeKind::Function
        | ExplorerNodeKind::Procedure
        | ExplorerNodeKind::Trigger => {
            if matches_query(&node.name, query) || matches_query(&node.qualified_name, query) {
                Some(node.clone())
            } else {
                None
            }
        }
    }
}

fn matches_query(value: &str, query: &str) -> bool {
    value.to_ascii_lowercase().contains(query)
}

#[cfg(test)]
mod tests {
    use super::{
        ExplorerConnectionSection, ExplorerNodeKind, filter_connection_sections, filter_node,
        filter_nodes, matches_query,
    };
    use models::ExplorerNode;

    fn make_node(name: &str, kind: ExplorerNodeKind, children: Vec<ExplorerNode>) -> ExplorerNode {
        let schema = if kind == ExplorerNodeKind::Schema {
            Some(name.to_string())
        } else {
            Some("public".to_string())
        };
        let qualified_name = if kind == ExplorerNodeKind::Schema {
            format!("\"{name}\"")
        } else {
            format!("\"public\".\"{name}\"")
        };
        ExplorerNode {
            name: name.to_string(),
            kind,
            schema,
            qualified_name,
            children,
        }
    }

    fn make_section(name: &str, nodes: Vec<ExplorerNode>) -> ExplorerConnectionSection {
        ExplorerConnectionSection {
            session_id: 1,
            name: name.to_string(),
            kind_label: "PostgreSQL".to_string(),
            status: "Connected".to_string(),
            is_active: true,
            nodes,
        }
    }

    #[test]
    fn matches_query_is_case_insensitive() {
        assert!(matches_query("Users", "users"));
        assert!(matches_query("USERS", "users"));
        assert!(matches_query("UserEvents", "userevents"));
    }

    #[test]
    fn matches_query_matches_substring() {
        assert!(matches_query("user_events", "event"));
        assert!(matches_query("order_items", "item"));
        assert!(!matches_query("users", "orders"));
    }

    #[test]
    fn empty_query_returns_all_sections_unchanged() {
        let sections = vec![
            make_section(
                "prod",
                vec![make_node(
                    "public",
                    ExplorerNodeKind::Schema,
                    vec![
                        make_node("users", ExplorerNodeKind::Table, vec![]),
                        make_node("orders", ExplorerNodeKind::Table, vec![]),
                    ],
                )],
            ),
            make_section(
                "staging",
                vec![make_node(
                    "public",
                    ExplorerNodeKind::Schema,
                    vec![make_node("logs", ExplorerNodeKind::Table, vec![])],
                )],
            ),
        ];

        let result = filter_connection_sections(&sections, "");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].nodes[0].children.len(), 2);
        assert_eq!(result[1].nodes[0].children.len(), 1);
    }

    #[test]
    fn whitespace_only_query_returns_all_sections() {
        let sections = vec![make_section(
            "db",
            vec![make_node(
                "public",
                ExplorerNodeKind::Schema,
                vec![make_node("users", ExplorerNodeKind::Table, vec![])],
            )],
        )];

        let result = filter_connection_sections(&sections, "   ");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].nodes[0].children.len(), 1);
    }

    #[test]
    fn partial_match_filters_tables_within_schema() {
        let schema = make_node(
            "public",
            ExplorerNodeKind::Schema,
            vec![
                make_node("users", ExplorerNodeKind::Table, vec![]),
                make_node("user_settings", ExplorerNodeKind::Table, vec![]),
                make_node("orders", ExplorerNodeKind::Table, vec![]),
                make_node("order_items", ExplorerNodeKind::Table, vec![]),
            ],
        );
        let sections = vec![make_section("db", vec![schema])];

        let result = filter_connection_sections(&sections, "user");
        assert_eq!(result.len(), 1);
        let schema_children = &result[0].nodes[0].children;
        assert_eq!(schema_children.len(), 2);
        let names: Vec<&str> = schema_children.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"users"));
        assert!(names.contains(&"user_settings"));
    }

    #[test]
    fn partial_match_qualified_name() {
        let schema = make_node(
            "analytics",
            ExplorerNodeKind::Schema,
            vec![
                make_node("events", ExplorerNodeKind::Table, vec![]),
                make_node("sessions", ExplorerNodeKind::Table, vec![]),
            ],
        );
        let sections = vec![make_section("db", vec![schema])];

        let result = filter_connection_sections(&sections, "analytics");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].nodes[0].children.len(), 2);
    }

    #[test]
    fn schema_name_match_preserves_all_children() {
        let schema = make_node(
            "analytics",
            ExplorerNodeKind::Schema,
            vec![
                make_node("events", ExplorerNodeKind::Table, vec![]),
                make_node("sessions", ExplorerNodeKind::Table, vec![]),
                make_node("page_views", ExplorerNodeKind::View, vec![]),
            ],
        );
        let nodes = vec![schema];

        let result = filter_nodes(&nodes, "analytics");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].children.len(), 3);
    }

    #[test]
    fn schema_name_mismatch_filters_children() {
        let schema = make_node(
            "public",
            ExplorerNodeKind::Schema,
            vec![
                make_node("user_events", ExplorerNodeKind::Table, vec![]),
                make_node("orders", ExplorerNodeKind::Table, vec![]),
            ],
        );
        let nodes = vec![schema];

        let result = filter_nodes(&nodes, "event");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].children.len(), 1);
        assert_eq!(result[0].children[0].name, "user_events");
    }

    #[test]
    fn section_name_match_preserves_all_nodes() {
        let sections = vec![make_section(
            "production_db",
            vec![make_node(
                "public",
                ExplorerNodeKind::Schema,
                vec![
                    make_node("users", ExplorerNodeKind::Table, vec![]),
                    make_node("orders", ExplorerNodeKind::Table, vec![]),
                ],
            )],
        )];

        let result = filter_connection_sections(&sections, "production");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].nodes[0].children.len(), 2);
    }

    #[test]
    fn section_kind_label_match_preserves_all_nodes() {
        let sections = vec![make_section(
            "mydb",
            vec![make_node(
                "public",
                ExplorerNodeKind::Schema,
                vec![make_node("users", ExplorerNodeKind::Table, vec![])],
            )],
        )];

        let result = filter_connection_sections(&sections, "postgresql");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].nodes[0].children.len(), 1);
    }

    #[test]
    fn no_matching_query_returns_empty() {
        let sections = vec![make_section(
            "db",
            vec![make_node(
                "public",
                ExplorerNodeKind::Schema,
                vec![
                    make_node("users", ExplorerNodeKind::Table, vec![]),
                    make_node("orders", ExplorerNodeKind::Table, vec![]),
                ],
            )],
        )];

        let result = filter_connection_sections(&sections, "nonexistent");
        assert!(result.is_empty());
    }

    #[test]
    fn filter_table_node_matches_name() {
        let node = make_node("users", ExplorerNodeKind::Table, vec![]);
        assert!(filter_node(&node, "user").is_some());
        assert!(filter_node(&node, "order").is_none());
    }

    #[test]
    fn filter_view_node_matches_name() {
        let node = make_node("active_users", ExplorerNodeKind::View, vec![]);
        assert!(filter_node(&node, "active").is_some());
        assert!(filter_node(&node, "deleted").is_none());
    }

    #[test]
    fn filters_across_multiple_sections() {
        let sections = vec![
            make_section(
                "prod",
                vec![make_node(
                    "public",
                    ExplorerNodeKind::Schema,
                    vec![
                        make_node("users", ExplorerNodeKind::Table, vec![]),
                        make_node("orders", ExplorerNodeKind::Table, vec![]),
                    ],
                )],
            ),
            make_section(
                "analytics",
                vec![make_node(
                    "public",
                    ExplorerNodeKind::Schema,
                    vec![
                        make_node("user_events", ExplorerNodeKind::Table, vec![]),
                        make_node("page_views", ExplorerNodeKind::View, vec![]),
                    ],
                )],
            ),
        ];

        let result = filter_connection_sections(&sections, "user");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].nodes[0].children.len(), 1);
        assert_eq!(result[0].nodes[0].children[0].name, "users");
        assert_eq!(result[1].nodes[0].children.len(), 1);
        assert_eq!(result[1].nodes[0].children[0].name, "user_events");
    }

    #[test]
    fn filter_distinguishes_views_from_tables_by_name() {
        let schema = make_node(
            "public",
            ExplorerNodeKind::Schema,
            vec![
                make_node("active_sessions", ExplorerNodeKind::View, vec![]),
                make_node("archived_sessions", ExplorerNodeKind::Table, vec![]),
                make_node("orders", ExplorerNodeKind::Table, vec![]),
            ],
        );
        let nodes = vec![schema];

        let result = filter_nodes(&nodes, "active_session");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].children.len(), 1);
        assert_eq!(result[0].children[0].name, "active_sessions");
        assert_eq!(result[0].children[0].kind, ExplorerNodeKind::View);
    }
}
