use super::components::{
    ErColumn,
    ErDiagramState,
    ErForeignKey,
    ErRelationship,
    ErTable,
    ExplorerConnectionSection,
    replace_messages,
};
use crate::app_state::APP_UI_SETTINGS;
use dioxus::prelude::*;
use models::{
    AcpPanelState,
    AcpUiMessage,
    ChatThreadSummary,
    WorkspaceToolDock,
    WorkspaceToolLayout,
    WorkspaceToolPanel,
};

pub const SIDEBAR_MIN_WIDTH: f64 = 240.0;
pub const SIDEBAR_MAX_WIDTH: f64 = 560.0;
pub const INSPECTOR_MIN_WIDTH: f64 = 260.0;
pub const INSPECTOR_MAX_WIDTH: f64 = 640.0;
pub const BOTTOM_PANEL_MIN_HEIGHT: f64 = 72.0;
pub const BOTTOM_PANEL_MAX_HEIGHT: f64 = 520.0;
pub const WORKSPACE_ROOT_ID: &str = "workspace-root";

pub fn format_explorer_error(err: impl std::fmt::Display) -> String {
    format!("Error: {err}")
}

/// Человекочитаемое форматирование длительности выполнения запроса.
/// Меньше секунды — в миллисекундах, иначе — в секундах с одним знаком.
pub fn format_duration(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else {
        format!("{:.1}s", ms as f64 / 1000.0)
    }
}

pub fn can_edit_rows(capabilities: models::Capabilities) -> bool {
    capabilities.row_editing
}

pub fn can_import_csv(capabilities: models::Capabilities) -> bool {
    capabilities.import_csv
}

pub fn can_explain(capabilities: models::Capabilities) -> bool {
    capabilities.explain
}

pub fn session_capabilities(session_id: u64) -> Option<models::Capabilities> {
    crate::app_state::APP_STATE
        .read()
        .session(session_id)
        .map(|session| session.capabilities)
}

/// Строит данные ER-диаграммы из секций дерева + загруженных внешних ключей.
/// Линии связей создаются только для FK, у которых обе таблицы (источник и
/// цель) присутствуют в дереве — чтобы не рисовать линии в никуда для FK,
/// ссылающихся на таблицы вне текущего подключения/схемы.
///
/// Между парой таблиц рисуется одна линия (составные FK и несколько FK между
/// одной парой таблиц схлопываются в одну связь), чтобы избежать пучка
/// параллельных линий.
pub fn build_er_diagram(
    sections: &[ExplorerConnectionSection],
    foreign_keys: &[models::TableForeignKey],
) -> Option<ErDiagramState> {
    let mut tables = Vec::new();
    let mut known: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();

    for section in sections {
        collect_er_tables(&section.nodes, &mut tables, &mut known);
    }

    if tables.is_empty() {
        return None;
    }

    let mut seen_pairs: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    let mut relationships = Vec::new();
    for fk in foreign_keys {
        let from = (fk.from_schema.clone(), fk.from_table.clone());
        let to = (fk.to_schema.clone(), fk.to_table.clone());
        if !known.contains(&from) || !known.contains(&to) {
            continue;
        }
        if let Some(table) = tables.iter_mut().find(|table| {
            table.name == fk.from_table
                && (table.schema == fk.from_schema || table.schema.is_empty())
        }) {
            table.foreign_keys.push(ErForeignKey {
                name: fk.name.clone(),
                from_table: fk.from_table.clone(),
                from_column: fk.from_column.clone(),
                to_table: fk.to_table.clone(),
                to_column: fk.to_column.clone(),
            });
        }
        if !seen_pairs.insert((fk.from_table.clone(), fk.to_table.clone())) {
            continue;
        }
        relationships.push(ErRelationship {
            from_table: fk.from_table.clone(),
            from_column: fk.from_column.clone(),
            to_table: fk.to_table.clone(),
            to_column: fk.to_column.clone(),
        });
    }

    Some(ErDiagramState {
        tables,
        relationships,
    })
}

fn collect_er_tables(
    nodes: &[models::ExplorerNode],
    tables: &mut Vec<ErTable>,
    known: &mut std::collections::HashSet<(String, String)>,
) {
    for node in nodes {
        if node.kind == models::ExplorerNodeKind::Table {
            let schema_name = node
                .schema
                .clone()
                .filter(|schema| !schema.is_empty())
                .unwrap_or_else(|| "main".to_string());
            if known.insert((schema_name.clone(), node.name.clone())) {
                tables.push(ErTable {
                    schema: schema_name,
                    name: node.name.clone(),
                    columns: Vec::new(),
                    primary_key: Vec::new(),
                    foreign_keys: Vec::new(),
                });
            }
        }
        if !node.children.is_empty() {
            collect_er_tables(&node.children, tables, known);
        }
    }
}

/// Fills column / PK metadata from a `describe_table` page.
pub fn apply_table_describe(
    table: &mut ErTable,
    output: &models::QueryOutput,
    relationships: &[ErRelationship],
) {
    let models::QueryOutput::Table(page) = output else {
        return;
    };
    let mut columns = Vec::new();
    let mut primary_key = Vec::new();
    for row in &page.rows {
        let section = row.first().map(String::as_str).unwrap_or_default();
        if section != "column" {
            continue;
        }
        let name = row.get(1).cloned().unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let data_type = row.get(2).cloned().unwrap_or_default();
        let target = row.get(3).cloned().unwrap_or_default();
        let details = row.get(4).cloned().unwrap_or_default();
        let is_primary_key = target.to_ascii_lowercase().contains("pk")
            || details.to_ascii_lowercase().contains("primary");
        if is_primary_key {
            primary_key.push(name.clone());
        }
        let is_foreign_key = relationships
            .iter()
            .any(|rel| rel.from_table == table.name && rel.from_column == name)
            || table.foreign_keys.iter().any(|fk| fk.from_column == name);
        let is_nullable = !details.to_ascii_uppercase().contains("NOT NULL");
        columns.push(ErColumn {
            name,
            data_type,
            is_nullable,
            is_primary_key,
            is_foreign_key,
        });
    }
    if !columns.is_empty() {
        table.columns = columns;
        table.primary_key = primary_key;
    }
}

pub async fn enrich_er_diagram(session_id: u64, diagram: &mut ErDiagramState) {
    let relationships = diagram.relationships.clone();
    for table in &mut diagram.tables {
        let schema = if table.schema.is_empty() {
            None
        } else {
            Some(table.schema.clone())
        };
        if let Ok(output) = services::describe_table(session_id, schema, table.name.clone()).await {
            apply_table_describe(table, &output, &relationships);
        }
    }
}

/// Async wrapper that runs the (potentially heavy) ER-diagram build on a
/// blocking thread so it never stalls the async executor / render loop.
pub async fn build_er_diagram_async(
    sections: Vec<ExplorerConnectionSection>,
    foreign_keys: Vec<models::TableForeignKey>,
) -> Option<ErDiagramState> {
    tokio::task::spawn_blocking(move || build_er_diagram(&sections, &foreign_keys))
        .await
        .unwrap_or(None)
}

pub fn should_render_explorer_status(status: &str) -> bool {
    let status = status.trim();
    if status.is_empty() {
        return false;
    }

    status.starts_with("Loading")
        || status.starts_with("Error:")
        || status == "Explorer hidden"
        || status == "Select or create a connection"
        || status.contains("failed")
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn is_low_signal_explorer_status(status: &str) -> bool {
    let status = status.trim();
    status == "Explorer ready for the active connection" || status == "Ready"
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DockDropTarget {
    pub dock: WorkspaceToolDock,
    pub index: usize,
}

pub fn workspace_resize_script(
    width_var: &str,
    start_x: f64,
    start_width: f64,
    min_width: f64,
    max_width: f64,
    invert_delta: bool,
) -> String {
    let delta_factor = if invert_delta { -1.0 } else { 1.0 };
    format!(
        r#"
        (() => {{
            const workspace = document.getElementById({WORKSPACE_ROOT_ID:?});
            if (!workspace) {{
                return {start_width};
            }}

            const startX = {start_x};
            const startWidth = {start_width};
            const minWidth = {min_width};
            const maxWidth = {max_width};
            const deltaFactor = {delta_factor};
            let finished = false;
            let lastWidth = startWidth;

            const clampWidth = (clientX) => {{
                const delta = (clientX - startX) * deltaFactor;
                return Math.min(maxWidth, Math.max(minWidth, startWidth + delta));
            }};

            return new Promise((resolve) => {{
                const finish = (clientX) => {{
                    if (finished) {{
                        return;
                    }}
                    finished = true;
                    const width = clientX == null ? lastWidth : clampWidth(clientX);
                    workspace.style.setProperty({width_var:?}, `${{Math.round(width)}}px`);
                    workspace.classList.remove("workspace--resizing");
                    window.removeEventListener("mousemove", onMove);
                    window.removeEventListener("mouseup", onUp);
                    window.removeEventListener("blur", onBlur);
                    resolve(width);
                }};

                const onMove = (event) => {{
                    const width = clampWidth(event.clientX);
                    lastWidth = width;
                    workspace.style.setProperty({width_var:?}, `${{Math.round(width)}}px`);
                }};

                const onUp = (event) => finish(event.clientX);
                const onBlur = () => finish(startX);

                workspace.classList.add("workspace--resizing");
                window.addEventListener("mousemove", onMove, {{ passive: true }});
                window.addEventListener("mouseup", onUp);
                window.addEventListener("blur", onBlur);
                onMove({{ clientX: startX }});
            }});
        }})()
        "#
    )
}

/// Y-axis variant of [`workspace_resize_script`] used by the bottom dock
/// resize handle. The drag axis is vertical and the delta is always
/// inverted (dragging up = taller) because the dock grows upward from
/// the bottom of the workspace.
pub fn workspace_vertical_resize_script(
    height_var: &str,
    start_y: f64,
    start_height: f64,
    min_height: f64,
    max_height: f64,
) -> String {
    format!(
        r#"
        (() => {{
            const workspace = document.getElementById({WORKSPACE_ROOT_ID:?});
            if (!workspace) {{
                return {start_height};
            }}

            const startY = {start_y};
            const startHeight = {start_height};
            const minHeight = {min_height};
            const maxHeight = {max_height};
            let finished = false;
            let lastHeight = startHeight;

            const clampHeight = (clientY) => {{
                // Drag up (clientY decreases) -> taller dock.
                const delta = startY - clientY;
                return Math.min(maxHeight, Math.max(minHeight, startHeight + delta));
            }};

            return new Promise((resolve) => {{
                const finish = (clientY) => {{
                    if (finished) {{
                        return;
                    }}
                    finished = true;
                    const height = clientY == null ? lastHeight : clampHeight(clientY);
                    workspace.style.setProperty({height_var:?}, `${{Math.round(height)}}px`);
                    workspace.classList.remove("workspace--resizing-y");
                    window.removeEventListener("mousemove", onMove);
                    window.removeEventListener("mouseup", onUp);
                    window.removeEventListener("blur", onBlur);
                    resolve(height);
                }};

                const onMove = (event) => {{
                    const height = clampHeight(event.clientY);
                    lastHeight = height;
                    workspace.style.setProperty({height_var:?}, `${{Math.round(height)}}px`);
                }};

                const onUp = (event) => finish(event.clientY);
                const onBlur = () => finish(startY);

                workspace.classList.add("workspace--resizing-y");
                window.addEventListener("mousemove", onMove, {{ passive: true }});
                window.addEventListener("mouseup", onUp);
                window.addEventListener("blur", onBlur);
                onMove({{ clientY: startY }});
            }});
        }})()
        "#
    )
}

pub async fn load_explorer_section(
    session: models::ConnectionSession,
    active_session_id: Option<u64>,
    use_cache: bool,
) -> ExplorerConnectionSection {
    let kind_label = session.kind.display_name().to_string();

    // Dev-only: the mock session uses a `:memory:` SQLite pool but
    // ships a hand-crafted tree. We short-circuit before the real
    // `services::load_connection_tree` so the in-memory pool's
    // (empty) schema does not overwrite the mock sections.
    #[cfg(debug_assertions)]
    {
        let is_mock = session.request.identity_key() == crate::dev::MOCK_CONNECTION_IDENTITY_KEY;
        if is_mock {
            let mut sections = crate::dev::mock_sections(session.id);
            if let Some(section) = sections.first_mut() {
                section.is_active = Some(session.id) == active_session_id;
            }
            if let Some(section) = sections.into_iter().next() {
                crate::app_state::cache_explorer(session.id, vec![section.clone()]).await;
                return section;
            }
        }
    }

    if use_cache
        && let Some(cached) = crate::app_state::get_cached_explorer(session.id).await
        && let Some(section) = cached.into_iter().next()
    {
        return ExplorerConnectionSection {
            is_active: Some(session.id) == active_session_id,
            ..section
        };
    }

    // Загружаем из БД
    match services::load_connection_tree(session.id).await {
        Ok(nodes) => {
            let section = ExplorerConnectionSection {
                session_id: session.id,
                name: connection_target_label(&session.request),
                kind_label: kind_label.clone(),
                status: "Ready".to_string(),
                is_active: Some(session.id) == active_session_id,
                nodes,
            };

            // Кэшируем результат
            crate::app_state::cache_explorer(session.id, vec![section.clone()]).await;

            section
        }
        Err(err) => ExplorerConnectionSection {
            session_id: session.id,
            name: connection_target_label(&session.request),
            kind_label,
            status: format_explorer_error(&err),
            is_active: Some(session.id) == active_session_id,
            nodes: Vec::new(),
        },
    }
}

fn connection_target_label(request: &models::ConnectionRequest) -> String {
    request.short_name()
}

pub fn unloaded_explorer_section(
    session: &models::ConnectionSession,
    active_session_id: Option<u64>,
    status: &str,
) -> ExplorerConnectionSection {
    let kind_label = session.kind.display_name().to_string();

    ExplorerConnectionSection {
        session_id: session.id,
        name: connection_target_label(&session.request),
        kind_label,
        status: status.to_string(),
        is_active: Some(session.id) == active_session_id,
        nodes: Vec::new(),
    }
}

pub struct ToolPanelVisibility {
    pub show_saved_queries: bool,
    pub show_connections: bool,
    pub show_explorer: bool,
    pub show_history: bool,
    pub show_agent_panel: bool,
    pub ai_features_enabled: bool,
}

fn is_tool_panel_visible(panel: WorkspaceToolPanel, vis: &ToolPanelVisibility) -> bool {
    match panel {
        WorkspaceToolPanel::SavedQueries => vis.show_saved_queries,
        WorkspaceToolPanel::Connections => vis.show_connections,
        WorkspaceToolPanel::Explorer => vis.show_explorer,
        WorkspaceToolPanel::History => vis.show_history,
        WorkspaceToolPanel::Agent => vis.ai_features_enabled && vis.show_agent_panel,
    }
}

pub fn visible_tool_panels(
    panels: &[WorkspaceToolPanel],
    vis: &ToolPanelVisibility,
) -> Vec<WorkspaceToolPanel> {
    panels
        .iter()
        .copied()
        .filter(|panel| is_tool_panel_visible(*panel, vis))
        .collect()
}

fn visible_insert_index(
    panels: &[WorkspaceToolPanel],
    target_visible_index: usize,
    vis: &ToolPanelVisibility,
) -> usize {
    if !panels
        .iter()
        .any(|panel| is_tool_panel_visible(*panel, vis))
    {
        return 0;
    }

    let mut visible_index = 0;
    for (index, panel) in panels.iter().enumerate() {
        if !is_tool_panel_visible(*panel, vis) {
            continue;
        }

        if visible_index == target_visible_index {
            return index;
        }

        visible_index += 1;
    }

    panels.len()
}

pub fn move_tool_panel_layout(
    layout: &mut WorkspaceToolLayout,
    panel: WorkspaceToolPanel,
    target: DockDropTarget,
    vis: &ToolPanelVisibility,
) {
    let mut normalized = layout.normalized();
    normalized.sidebar.retain(|existing| *existing != panel);
    normalized.inspector.retain(|existing| *existing != panel);

    let target_panels = match target.dock {
        WorkspaceToolDock::Sidebar => &mut normalized.sidebar,
        WorkspaceToolDock::Inspector => &mut normalized.inspector,
    };
    let insert_at = visible_insert_index(target_panels, target.index, vis).min(target_panels.len());
    target_panels.insert(insert_at, panel);

    *layout = normalized;
}

pub fn apply_tool_panel_drop(
    mut dragging_panel: Signal<Option<WorkspaceToolPanel>>,
    mut drop_target: Signal<Option<DockDropTarget>>,
    target: DockDropTarget,
    vis: &ToolPanelVisibility,
) {
    if let Some(panel) = dragging_panel() {
        APP_UI_SETTINGS.with_mut(|settings| {
            move_tool_panel_layout(&mut settings.tool_panel_layout, panel, target, vis);
        });
    }

    dragging_panel.set(None);
    drop_target.set(None);
}

fn compact_chat_title(text: &str, max_chars: usize) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        return "New chat".to_string();
    }

    let count = compact.chars().count();
    if count <= max_chars {
        compact
    } else {
        format!("{}...", compact.chars().take(max_chars).collect::<String>())
    }
}

pub fn derive_chat_thread_title(
    current_title: Option<&str>,
    messages: &[AcpUiMessage],
    connection_label: &str,
) -> String {
    let _ = connection_label;
    if let Some(current_title) = current_title
        .map(str::trim)
        .filter(|title| !title.is_empty() && *title != "New chat")
    {
        return current_title.to_string();
    }

    if let Some(first_user_message) = messages
        .iter()
        .find(|message| matches!(message.kind, models::AcpMessageKind::User))
        .map(|message| {
            message
                .text
                .strip_prefix("Generate SQL:")
                .unwrap_or(&message.text)
                .trim()
        })
        .filter(|text| !text.is_empty())
    {
        return compact_chat_title(first_user_message, 56);
    }

    "New chat".to_string()
}

pub fn upsert_chat_thread_summary(
    threads: &mut Vec<ChatThreadSummary>,
    summary: ChatThreadSummary,
) {
    if let Some(existing) = threads.iter_mut().find(|thread| thread.id == summary.id) {
        *existing = summary;
    } else {
        threads.push(summary);
    }

    threads.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| right.id.cmp(&left.id))
    });
}

pub fn reset_panel_for_thread(state: &mut AcpPanelState, title: &str, messages: Vec<AcpUiMessage>) {
    let _ = title;
    let launch = state.launch.clone();
    let ollama = state.ollama.clone();
    *state = AcpPanelState::new(launch, ollama);
    replace_messages(state, messages);
    state.status = "Connect an agent to continue.".to_string();
}

pub fn tool_panel_class(panel: WorkspaceToolPanel) -> &'static str {
    match panel {
        WorkspaceToolPanel::Connections => " workspace__tool-panel--connections",
        WorkspaceToolPanel::Explorer => " workspace__tool-panel--explorer",
        WorkspaceToolPanel::SavedQueries => " workspace__tool-panel--saved",
        WorkspaceToolPanel::History => " workspace__tool-panel--history",
        WorkspaceToolPanel::Agent => " workspace__tool-panel--agent",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ErRelationship,
        ErTable,
        ExplorerConnectionSection,
        apply_table_describe,
        build_er_diagram,
        can_edit_rows,
        can_import_csv,
        derive_chat_thread_title,
        format_explorer_error,
        is_low_signal_explorer_status,
        reset_panel_for_thread,
        should_render_explorer_status,
    };
    use models::{
        AcpLaunchRequest,
        AcpOllamaConfig,
        AcpPanelState,
        AcpUiMessage,
        Capabilities,
        DatabaseKind,
    };

    #[test]
    fn clickhouse_cannot_edit_rows() {
        assert!(!can_edit_rows(Capabilities::for_kind(
            DatabaseKind::ClickHouse
        )));
    }

    #[test]
    fn sqlite_can_edit_rows() {
        assert!(can_edit_rows(Capabilities::for_kind(DatabaseKind::Sqlite)));
    }

    #[test]
    fn clickhouse_can_import_csv() {
        assert!(can_import_csv(Capabilities::for_kind(
            DatabaseKind::ClickHouse
        )));
    }

    #[test]
    fn default_chat_title_stays_compact() {
        assert_eq!(
            derive_chat_thread_title(None, &[], "SQLite · /home/rasul/Documents/data.sqlite"),
            "New chat"
        );
    }

    #[test]
    fn reset_panel_uses_compact_disconnected_status() {
        let mut state = AcpPanelState::new(
            AcpLaunchRequest {
                command: String::new(),
                args: String::new(),
                cwd: ".".to_string(),
                env: Vec::new(),
            },
            AcpOllamaConfig {
                base_url: String::new(),
                model: String::new(),
                api_key: String::new(),
            },
        );

        reset_panel_for_thread(&mut state, "New chat · SQLite", Vec::<AcpUiMessage>::new());
        assert_eq!(state.status, "Connect an agent to continue.");
    }

    #[test]
    fn explorer_error_uses_display_not_debug() {
        let formatted = format_explorer_error("connection timeout");
        assert_eq!(formatted, "Error: connection timeout");
        assert!(!formatted.contains(":?"));
    }

    #[test]
    fn explorer_status_visible_for_loading_states() {
        assert!(should_render_explorer_status("Loading explorer..."));
        assert!(should_render_explorer_status("Loading..."));
    }

    #[test]
    fn explorer_status_visible_for_error_states() {
        assert!(should_render_explorer_status("Error: connection failed"));
        assert!(should_render_explorer_status(
            "Explorer failed for the active connection"
        ));
    }

    #[test]
    fn explorer_status_visible_for_hidden_and_no_connection() {
        assert!(should_render_explorer_status("Explorer hidden"));
        assert!(should_render_explorer_status(
            "Select or create a connection"
        ));
    }

    #[test]
    fn explorer_status_hidden_for_ready_state() {
        assert!(!should_render_explorer_status(
            "Explorer ready for the active connection"
        ));
        assert!(!should_render_explorer_status("Ready"));
    }

    #[test]
    fn explorer_status_hidden_for_empty() {
        assert!(!should_render_explorer_status(""));
        assert!(!should_render_explorer_status("   "));
    }

    #[test]
    fn low_signal_status_detection() {
        assert!(is_low_signal_explorer_status(
            "Explorer ready for the active connection"
        ));
        assert!(is_low_signal_explorer_status("Ready"));
        assert!(!is_low_signal_explorer_status("Loading..."));
        assert!(!is_low_signal_explorer_status("Error: failed"));
    }

    #[test]
    fn er_diagram_empty_sections_returns_none() {
        assert!(build_er_diagram(&[], &[]).is_none());
    }

    fn sample_schema_sections() -> Vec<ExplorerConnectionSection> {
        use models::ExplorerNodeKind;

        vec![ExplorerConnectionSection {
            session_id: 1,
            name: "test".to_string(),
            kind_label: "SQLite".to_string(),
            status: "Ready".to_string(),
            is_active: true,
            nodes: vec![models::ExplorerNode {
                name: "main".to_string(),
                kind: ExplorerNodeKind::Schema,
                schema: None,
                qualified_name: "main".to_string(),
                row_count: None,
                children: vec![
                    models::ExplorerNode {
                        name: "users".to_string(),
                        kind: ExplorerNodeKind::Table,
                        schema: Some("main".to_string()),
                        qualified_name: "main.users".to_string(),
                        row_count: None,
                        children: Vec::new(),
                    },
                    models::ExplorerNode {
                        name: "orders".to_string(),
                        kind: ExplorerNodeKind::Table,
                        schema: Some("main".to_string()),
                        qualified_name: "main.orders".to_string(),
                        row_count: None,
                        children: Vec::new(),
                    },
                    models::ExplorerNode {
                        name: "v_users".to_string(),
                        kind: ExplorerNodeKind::View,
                        schema: Some("main".to_string()),
                        qualified_name: "main.v_users".to_string(),
                        row_count: None,
                        children: Vec::new(),
                    },
                ],
            }],
        }]
    }

    #[test]
    fn er_diagram_builds_tables_from_schema_nodes() {
        let sections = sample_schema_sections();
        let diagram = build_er_diagram(&sections, &[]).expect("diagram should be built");
        assert_eq!(diagram.tables.len(), 2); // only tables, not views
        assert!(diagram.tables.iter().any(|t| t.name == "users"));
        assert!(diagram.tables.iter().any(|t| t.name == "orders"));
        assert!(diagram.relationships.is_empty());
    }

    #[test]
    fn er_diagram_wires_relationships_for_known_tables() {
        let sections = sample_schema_sections();
        let fks = vec![models::TableForeignKey {
            name: "orders_user_fk".to_string(),
            from_schema: "main".to_string(),
            from_table: "orders".to_string(),
            from_column: "user_id".to_string(),
            to_schema: "main".to_string(),
            to_table: "users".to_string(),
            to_column: "id".to_string(),
        }];
        let diagram = build_er_diagram(&sections, &fks).expect("diagram should be built");
        assert_eq!(diagram.relationships.len(), 1);
        let rel = &diagram.relationships[0];
        assert_eq!(rel.from_table, "orders");
        assert_eq!(rel.to_table, "users");
        assert_eq!(rel.from_column, "user_id");
        assert_eq!(rel.to_column, "id");
    }

    #[test]
    fn er_diagram_skips_fk_with_missing_endpoint() {
        let sections = sample_schema_sections();
        // Цель "profiles" отсутствует в дереве — связь не рисуется.
        let fks = vec![models::TableForeignKey {
            name: "orders_profile_fk".to_string(),
            from_schema: "main".to_string(),
            from_table: "orders".to_string(),
            from_column: "profile_id".to_string(),
            to_schema: "main".to_string(),
            to_table: "profiles".to_string(),
            to_column: "id".to_string(),
        }];
        let diagram = build_er_diagram(&sections, &fks).expect("diagram should be built");
        assert!(diagram.relationships.is_empty());
    }

    #[test]
    fn er_diagram_dedupes_composite_fk_to_one_line() {
        let sections = sample_schema_sections();
        // Составной FK из двух колонок между одной парой таблиц — одна линия.
        let fks = vec![
            models::TableForeignKey {
                name: "fk_order_owner".to_string(),
                from_schema: "main".to_string(),
                from_table: "orders".to_string(),
                from_column: "owner_a".to_string(),
                to_schema: "main".to_string(),
                to_table: "users".to_string(),
                to_column: "a".to_string(),
            },
            models::TableForeignKey {
                name: "fk_order_owner".to_string(),
                from_schema: "main".to_string(),
                from_table: "orders".to_string(),
                from_column: "owner_b".to_string(),
                to_schema: "main".to_string(),
                to_table: "users".to_string(),
                to_column: "b".to_string(),
            },
        ];
        let diagram = build_er_diagram(&sections, &fks).expect("diagram should be built");
        assert_eq!(diagram.relationships.len(), 1);
    }

    #[test]
    fn er_diagram_collects_nested_tables() {
        use models::ExplorerNodeKind;
        let sections = vec![ExplorerConnectionSection {
            session_id: 1,
            name: "db".to_string(),
            kind_label: "SQLite".to_string(),
            status: "Ready".to_string(),
            is_active: true,
            nodes: vec![models::ExplorerNode {
                name: "main".to_string(),
                kind: ExplorerNodeKind::Schema,
                schema: Some("main".to_string()),
                qualified_name: "main".to_string(),
                row_count: None,
                children: vec![models::ExplorerNode {
                    name: "TABLES".to_string(),
                    kind: ExplorerNodeKind::Schema,
                    schema: Some("main".to_string()),
                    qualified_name: "main.tables".to_string(),
                    row_count: None,
                    children: vec![models::ExplorerNode {
                        name: "users".to_string(),
                        kind: ExplorerNodeKind::Table,
                        schema: Some("main".to_string()),
                        qualified_name: "users".to_string(),
                        row_count: None,
                        children: Vec::new(),
                    }],
                }],
            }],
        }];
        let diagram = build_er_diagram(&sections, &[]).expect("nested table");
        assert_eq!(diagram.tables.len(), 1);
        assert_eq!(diagram.tables[0].name, "users");
    }

    #[test]
    fn apply_table_describe_reads_pk_and_types() {
        let mut table = ErTable {
            schema: "main".to_string(),
            name: "orders".to_string(),
            columns: Vec::new(),
            primary_key: Vec::new(),
            foreign_keys: Vec::new(),
        };
        let output = models::QueryOutput::Table(models::QueryPage {
            columns: vec![
                "section".into(),
                "name".into(),
                "type".into(),
                "target".into(),
                "details".into(),
            ],
            rows: vec![
                vec![
                    "column".into(),
                    "id".into(),
                    "INTEGER".into(),
                    "pk#1".into(),
                    "NOT NULL".into(),
                ],
                vec![
                    "column".into(),
                    "user_id".into(),
                    "INTEGER".into(),
                    String::new(),
                    "NOT NULL".into(),
                ],
            ],
            editable: None,
            offset: 0,
            page_size: 100,
            has_previous: false,
            has_next: false,
        });
        let rels = vec![ErRelationship {
            from_table: "orders".into(),
            from_column: "user_id".into(),
            to_table: "users".into(),
            to_column: "id".into(),
        }];
        apply_table_describe(&mut table, &output, &rels);
        assert_eq!(table.columns.len(), 2);
        assert!(table.columns[0].is_primary_key);
        assert!(table.columns[1].is_foreign_key);
        assert_eq!(table.columns[1].data_type, "INTEGER");
        assert_eq!(table.primary_key, vec!["id".to_string()]);
    }
}
