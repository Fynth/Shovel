pub(crate) mod agent_panel;
mod batch_results;
mod blob_viewer;
mod bottom_panel;
mod chart;
mod copy_formats;
mod data_diff;
mod er_diagram;
mod execution_plan;
pub(crate) mod explorer;
mod history;
mod icon_button;
mod object_icon;
mod result_table;
mod saved_queries;
mod session_rail;
mod sql_editor;
mod sql_format_settings;
mod table_ddl_builder;
mod table_editor;
mod table_structure;
mod tabs;
mod value_editor;

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
    send_describe_object_request,
    send_sql_explanation_request,
    send_sql_generation_request,
    send_sql_plan_request,
};
pub(crate) use batch_results::BatchResultsView;
pub(crate) use blob_viewer::{BlobData, BlobViewer};
pub use bottom_panel::{BottomPanelDock, BottomPanelTab};
pub use chart::ResultChart;
pub(crate) use data_diff::DataDiffViewer;
pub(crate) use er_diagram::{ErDiagramState, ErDiagramViewer, ErRelationship, ErTable};
pub use execution_plan::ExecutionPlanView;
pub use explorer::{ExplorerConnectionSection, SidebarConnectionTree};
pub use history::QueryHistoryPanel;
pub use icon_button::{ActionIcon, Chevron, IconButton, IconGlyph};
pub(crate) use object_icon::ObjectIcon;
pub use result_table::ResultTable;
pub use saved_queries::SavedQueriesPanel;
pub use session_rail::SessionRail;
pub use sql_editor::SqlEditor;
pub use sql_format_settings::SqlFormatSettingsFields;
pub use table_editor::TableEditor;
pub use tabs::TabsManager;
pub(crate) use value_editor::{ValueEditor, ValueEditorMode, ValueEditorState};
