mod agent_panel;
mod blob_viewer;
mod chart;
mod data_diff;
mod er_diagram;
mod execution_plan;
mod explorer;
mod history;
mod icon_button;
mod result_table;
mod saved_queries;
mod session_rail;
mod sql_editor;
mod sql_format_settings;
mod table_editor;
mod tabs;

pub(crate) use agent_panel::{
    AcpAgentPanel,
    AgentSqlExecutionMode,
    apply_acp_events,
    default_acp_panel_state,
    ensure_default_sql_agent_connected,
    execute_agent_sql_request,
    extract_sql_candidate,
    preferred_sql_target_tab_id,
    replace_messages,
    send_sql_generation_request,
};
pub use chart::ResultChart;
pub use execution_plan::ExecutionPlanView;
pub use explorer::{ExplorerConnectionSection, SidebarConnectionTree};
pub use history::QueryHistoryPanel;
pub use icon_button::{ActionIcon, IconButton};
pub use result_table::ResultTable;
pub use saved_queries::SavedQueriesPanel;
pub use session_rail::SessionRail;
pub use sql_editor::SqlEditor;
pub use sql_format_settings::SqlFormatSettingsFields;
pub use tabs::TabsManager;
