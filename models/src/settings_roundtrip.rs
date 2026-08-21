//! Settings round-trip tests.
//!
//! These tests cover the *serialization* contract of the persisted settings
//! types. They are deliberately written as pure-Rust tests that do not touch
//! the filesystem or the keyring — actual `storage::save_app_ui_settings`
//! round-trips would require an isolated data directory and are out of
//! scope for this file.
//!
//! Why this still matters: `#[serde(default)]` on the settings structs means
//! that older JSON files (without the newer fields) must deserialize
//! cleanly into a fully-populated `AppUiSettings`. If that contract
//! breaks, every Shovel user who upgrades will see reset settings.

use crate::{
    AppThemePreference, AppUiSettings, SqlFormatSettings, WorkspaceToolDock, WorkspaceToolLayout,
    WorkspaceToolPanel,
};
use serde_json::{Value, json};

#[test]
fn app_ui_settings_default_round_trips_through_json() {
    let original = AppUiSettings::default();
    let serialized = serde_json::to_string(&original).expect("serialize default AppUiSettings");
    let deserialized: AppUiSettings =
        serde_json::from_str(&serialized).expect("deserialize default AppUiSettings");
    assert_eq!(original, deserialized);
}

#[test]
fn app_ui_settings_handles_legacy_json_without_new_fields() {
    // Simulate a JSON blob from a build that predates the `read_only_mode`
    // and `tool_panel_layout` fields. Both should fall back to their
    // `#[serde(default)]` values without errors.
    let legacy = json!({
        "theme": "Light",
        "ai_features_enabled": false,
        "restore_session_on_launch": false,
        "show_saved_queries": false,
        "show_connections": true,
        "show_explorer": false,
        "show_history": true,
        "show_sql_editor": true,
        "show_agent_panel": true,
        "default_page_size": 50,
        "codestral": {},
        "deepseek": {}
    });

    let parsed: AppUiSettings =
        serde_json::from_value(legacy).expect("legacy JSON must deserialize");
    assert_eq!(parsed.theme, AppThemePreference::Light);
    assert!(!parsed.ai_features_enabled);
    assert_eq!(parsed.default_page_size, 50);
    // Defaults for fields the legacy JSON omitted:
    assert!(!parsed.read_only_mode);
    assert_eq!(parsed.tool_panel_layout, WorkspaceToolLayout::default());
    // Explicitly-set fields in the legacy JSON should be honored:
    assert!(!parsed.show_saved_queries);
    assert!(!parsed.restore_session_on_launch);
    assert!(parsed.show_connections);
}

#[test]
fn app_ui_settings_handles_completely_empty_object() {
    let parsed: AppUiSettings =
        serde_json::from_value(json!({})).expect("empty JSON must deserialize to defaults");
    let defaults = AppUiSettings::default();
    assert_eq!(parsed, defaults);
}

#[test]
fn sql_format_settings_default_round_trips_through_json() {
    let original = SqlFormatSettings::default();
    let serialized = serde_json::to_string(&original).expect("serialize default SqlFormatSettings");
    let deserialized: SqlFormatSettings =
        serde_json::from_str(&serialized).expect("deserialize default SqlFormatSettings");
    assert_eq!(original, deserialized);
}

#[test]
fn workspace_tool_layout_normalized_dedupes_panels() {
    let layout = WorkspaceToolLayout {
        sidebar: vec![
            WorkspaceToolPanel::Connections,
            WorkspaceToolPanel::Connections, // duplicate
            WorkspaceToolPanel::Explorer,
        ],
        inspector: vec![WorkspaceToolPanel::Agent, WorkspaceToolPanel::Agent],
    };
    let normalized = layout.normalized();
    let sidebar_count = normalized
        .sidebar
        .iter()
        .filter(|p| **p == WorkspaceToolPanel::Connections)
        .count();
    assert_eq!(sidebar_count, 1, "duplicate panels must be removed");
    let agent_count_in_inspector = normalized
        .inspector
        .iter()
        .filter(|p| **p == WorkspaceToolPanel::Agent)
        .count();
    assert_eq!(agent_count_in_inspector, 1);
}

#[test]
fn workspace_tool_layout_dock_for_follows_inspector_membership() {
    let layout = WorkspaceToolLayout {
        sidebar: vec![WorkspaceToolPanel::Explorer],
        inspector: vec![WorkspaceToolPanel::Agent],
    };
    assert_eq!(
        layout.dock_for(WorkspaceToolPanel::Agent),
        WorkspaceToolDock::Inspector
    );
    assert_eq!(
        layout.dock_for(WorkspaceToolPanel::Explorer),
        WorkspaceToolDock::Sidebar
    );
}

#[test]
fn app_ui_settings_with_modified_values_round_trip() {
    let mut original = AppUiSettings {
        theme: AppThemePreference::Light,
        read_only_mode: true,
        default_page_size: 250,
        ..AppUiSettings::default()
    };
    original
        .tool_panel_layout
        .sidebar
        .push(WorkspaceToolPanel::History);

    let serialized = serde_json::to_string(&original).expect("serialize modified");
    let parsed: AppUiSettings = serde_json::from_str(&serialized).expect("deserialize modified");
    assert_eq!(parsed, original);
}

#[test]
fn unknown_fields_in_settings_json_are_tolerated() {
    // Forward-compat: a future build may add new fields. Older builds must
    // not crash on those fields — they just ignore them.
    let future = json!({
        "theme": "Dark",
        "show_saved_queries": true,
        "some_future_field_we_dont_know_about": 42,
        "another_unknown": {"nested": true}
    });
    let parsed: AppUiSettings =
        serde_json::from_value(future).expect("unknown fields must be ignored");
    assert_eq!(parsed.theme, AppThemePreference::Dark);
    assert!(parsed.show_saved_queries);
}

#[test]
fn settings_json_value_uses_snake_case_field_names() {
    // If this ever breaks, users with existing settings files will see
    // reset preferences on the next launch.
    let value: Value = serde_json::to_value(AppUiSettings::default()).expect("to_value");
    let obj = value
        .as_object()
        .expect("AppUiSettings must serialize to object");
    let required_snake_case = [
        "theme",
        "ai_features_enabled",
        "restore_session_on_launch",
        "read_only_mode",
        "show_saved_queries",
        "default_page_size",
        "tool_panel_layout",
    ];
    for field in required_snake_case {
        assert!(obj.contains_key(field), "missing snake_case field: {field}");
    }
}
