use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkspaceToolPanel {
    Connections,
    Explorer,
    SavedQueries,
    History,
    Agent,
}

impl WorkspaceToolPanel {
    pub const ALL: [Self; 5] = [
        Self::Connections,
        Self::Explorer,
        Self::SavedQueries,
        Self::History,
        Self::Agent,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Connections => "Connections",
            Self::Explorer => "Explorer",
            Self::SavedQueries => "Saved Queries",
            Self::History => "History",
            Self::Agent => "ACP Agent",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkspaceToolDock {
    Sidebar,
    Inspector,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceToolLayout {
    pub sidebar: Vec<WorkspaceToolPanel>,
    pub inspector: Vec<WorkspaceToolPanel>,
}

impl WorkspaceToolLayout {
    pub fn normalized(&self) -> Self {
        let defaults = Self::default();
        let mut sidebar = Vec::with_capacity(WorkspaceToolPanel::ALL.len());
        let mut inspector = Vec::with_capacity(WorkspaceToolPanel::ALL.len());
        let mut seen = Vec::with_capacity(WorkspaceToolPanel::ALL.len());

        let mut push_unique = |items: &[WorkspaceToolPanel],
                               target: &mut Vec<WorkspaceToolPanel>| {
            for panel in items {
                if seen.contains(panel) {
                    continue;
                }
                seen.push(*panel);
                target.push(*panel);
            }
        };

        push_unique(&self.sidebar, &mut sidebar);
        push_unique(&self.inspector, &mut inspector);
        push_unique(&defaults.sidebar, &mut sidebar);
        push_unique(&defaults.inspector, &mut inspector);

        Self { sidebar, inspector }
    }

    pub fn dock_for(&self, panel: WorkspaceToolPanel) -> WorkspaceToolDock {
        if self.inspector.contains(&panel) {
            WorkspaceToolDock::Inspector
        } else {
            WorkspaceToolDock::Sidebar
        }
    }
}

impl Default for WorkspaceToolLayout {
    fn default() -> Self {
        Self {
            sidebar: vec![
                WorkspaceToolPanel::Connections,
                WorkspaceToolPanel::Explorer,
                WorkspaceToolPanel::SavedQueries,
                WorkspaceToolPanel::History,
            ],
            inspector: vec![WorkspaceToolPanel::Agent],
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppThemePreference {
    #[default]
    Dark,
    Light,
}

impl AppThemePreference {
    pub fn css_class(self) -> &'static str {
        match self {
            Self::Dark => "theme-dark",
            Self::Light => "theme-light",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
        }
    }
}

/// UI density preset controlling toolbar / tab / row heights, font size
/// and icon size for an IDE-like compact layout. Compact is the default
/// and is what new installs land on; switching to Comfortable trades
/// density for touch-friendly tap targets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UiDensity {
    #[default]
    Compact,
    Normal,
    Comfortable,
}

impl UiDensity {
    pub const ALL: [Self; 3] = [Self::Compact, Self::Normal, Self::Comfortable];

    pub fn css_class(self) -> &'static str {
        match self {
            Self::Compact => "density-compact",
            Self::Normal => "density-normal",
            Self::Comfortable => "density-comfortable",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Normal => "Normal",
            Self::Comfortable => "Comfortable",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CodeStralSettings {
    pub enabled: bool,
    #[serde(skip_serializing)]
    pub api_key: String,
    pub model: String,
}

impl Default for CodeStralSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key: String::new(),
            model: "codestral-latest".to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeepSeekSettings {
    pub enabled: bool,
    #[serde(skip_serializing)]
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub thinking_enabled: bool,
    pub reasoning_effort: String,
}

impl Default for DeepSeekSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key: String::new(),
            base_url: "https://api.deepseek.com".to_string(),
            model: "deepseek-chat".to_string(),
            thinking_enabled: false,
            reasoning_effort: "medium".to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppUiSettings {
    pub theme: AppThemePreference,
    pub density: UiDensity,
    pub ai_features_enabled: bool,
    pub restore_session_on_launch: bool,
    pub read_only_mode: bool,
    pub show_saved_queries: bool,
    pub show_connections: bool,
    pub show_explorer: bool,
    pub show_history: bool,
    pub show_sql_editor: bool,
    pub show_agent_panel: bool,
    pub default_page_size: u32,
    pub tool_panel_layout: WorkspaceToolLayout,

    pub codestral: CodeStralSettings,
    pub deepseek: DeepSeekSettings,
    pub ai_response_language: String,
    /// When `true`, inline AI completions are inserted automatically
    /// after the user stops typing for a short idle pause; otherwise
    /// completions stay as ghost text until the user presses Tab.
    pub ai_auto_apply_completions: bool,
}

impl Default for AppUiSettings {
    fn default() -> Self {
        Self {
            theme: AppThemePreference::Dark,
            density: UiDensity::Compact,
            ai_features_enabled: true,
            restore_session_on_launch: true,
            read_only_mode: false,
            show_saved_queries: true,
            show_connections: false,
            show_explorer: true,
            show_history: false,
            show_sql_editor: false,
            show_agent_panel: false,
            default_page_size: 100,
            tool_panel_layout: WorkspaceToolLayout::default(),
            codestral: CodeStralSettings::default(),
            deepseek: DeepSeekSettings::default(),
            ai_response_language: "English".to_string(),
            ai_auto_apply_completions: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AppThemePreference, AppUiSettings, UiDensity};

    #[test]
    fn fresh_default_keeps_sql_editor_collapsed() {
        let defaults = AppUiSettings::default();
        assert!(!defaults.show_sql_editor);
    }

    #[test]
    fn fresh_default_ai_response_language_is_english() {
        assert_eq!(AppUiSettings::default().ai_response_language, "English");
    }

    #[test]
    fn fresh_default_density_is_compact() {
        assert_eq!(AppUiSettings::default().density, UiDensity::Compact);
        assert_eq!(UiDensity::default(), UiDensity::Compact);
    }

    #[test]
    fn density_enum_round_trips_via_json() {
        for variant in UiDensity::ALL {
            let serialized = serde_json::to_string(&variant).expect("density should serialize");
            let reloaded: UiDensity =
                serde_json::from_str(&serialized).expect("density should deserialize");
            assert_eq!(reloaded, variant);
        }
    }

    #[test]
    fn density_css_class_matches_each_variant() {
        assert_eq!(UiDensity::Compact.css_class(), "density-compact");
        assert_eq!(UiDensity::Normal.css_class(), "density-normal");
        assert_eq!(UiDensity::Comfortable.css_class(), "density-comfortable");
    }

    #[test]
    fn legacy_settings_missing_density_default_to_compact() {
        let settings: AppUiSettings = serde_json::from_str(r#"{"theme":"Dark"}"#)
            .expect("legacy settings fixture should deserialize");
        assert_eq!(settings.density, UiDensity::Compact);
    }

    #[test]
    fn legacy_settings_with_all_persisted_fields_but_no_density_defaults_to_compact() {
        let settings: AppUiSettings = serde_json::from_str(
            r#"{
                "theme":"Dark",
                "ai_features_enabled":true,
                "restore_session_on_launch":true,
                "show_saved_queries":true,
                "show_connections":false,
                "show_explorer":true,
                "show_history":false,
                "show_sql_editor":false,
                "show_agent_panel":false,
                "default_page_size":100,
                "tool_panel_layout":{
                    "sidebar":["Connections","Explorer","SavedQueries","History"],
                    "inspector":["Agent"]
                }
            }"#,
        )
        .expect("legacy settings fixture should deserialize");

        assert_eq!(settings.density, UiDensity::Compact);
    }

    #[test]
    fn legacy_settings_missing_ai_response_language_defaults_to_english() {
        let settings: AppUiSettings = serde_json::from_str(r#"{"theme":"Dark"}"#)
            .expect("legacy settings fixture should deserialize");
        assert_eq!(settings.ai_response_language, "English");
    }

    #[test]
    fn fresh_default_keeps_read_only_mode_disabled() {
        let defaults = AppUiSettings::default();
        assert!(!defaults.read_only_mode);
    }

    #[test]
    fn fresh_default_ai_auto_apply_completions_is_enabled() {
        assert!(AppUiSettings::default().ai_auto_apply_completions);
    }

    #[test]
    fn legacy_settings_missing_ai_auto_apply_completions_defaults_to_true() {
        // Settings written before the auto-apply feature shipped should
        // still deserialize — the missing field must default to `true` so
        // existing users get auto-apply behaviour on first launch.
        let settings: AppUiSettings = serde_json::from_str(
            r#"{
                "theme":"Dark",
                "ai_features_enabled":true,
                "restore_session_on_launch":true,
                "show_saved_queries":true,
                "show_connections":false,
                "show_explorer":true,
                "show_history":false,
                "show_sql_editor":false,
                "show_agent_panel":false,
                "default_page_size":100,
                "tool_panel_layout":{
                    "sidebar":["Connections","Explorer","SavedQueries","History"],
                    "inspector":["Agent"]
                }
            }"#,
        )
        .expect("legacy settings fixture should deserialize");

        assert!(settings.ai_auto_apply_completions);
    }

    #[test]
    fn persisted_read_only_mode_true_is_preserved() {
        let settings: AppUiSettings = serde_json::from_str(
            r#"{
                "theme":"Dark",
                "ai_features_enabled":true,
                "restore_session_on_launch":true,
                "read_only_mode":true,
                "show_saved_queries":true,
                "show_connections":false,
                "show_explorer":true,
                "show_history":false,
                "show_sql_editor":true,
                "show_agent_panel":false,
                "default_page_size":100,
                "tool_panel_layout":{
                    "sidebar":["Connections","Explorer","SavedQueries","History"],
                    "inspector":["Agent"]
                }
            }"#,
        )
        .expect("settings fixture should deserialize");

        assert!(settings.read_only_mode);
    }

    #[test]
    fn persisted_show_sql_editor_true_is_preserved() {
        let settings: AppUiSettings = serde_json::from_str(
            r#"{
                "theme":"Dark",
                "ai_features_enabled":true,
                "restore_session_on_launch":true,
                "show_saved_queries":true,
                "show_connections":false,
                "show_explorer":true,
                "show_history":false,
                "show_sql_editor":true,
                "show_agent_panel":false,
                "default_page_size":100,
                "tool_panel_layout":{
                    "sidebar":["Connections","Explorer","SavedQueries","History"],
                    "inspector":["Agent"]
                }
            }"#,
        )
        .expect("settings fixture should deserialize");

        assert!(settings.show_sql_editor);
    }

    #[test]
    fn persisted_settings_without_saved_queries_flag_keep_it_visible() {
        let settings: AppUiSettings = serde_json::from_str(
            r#"{
                "theme":"Dark",
                "ai_features_enabled":true,
                "restore_session_on_launch":true,
                "show_connections":false,
                "show_explorer":true,
                "show_history":false,
                "show_sql_editor":false,
                "show_agent_panel":false,
                "default_page_size":100,
                "tool_panel_layout":{
                    "sidebar":["Connections","Explorer","SavedQueries","History"],
                    "inspector":["Agent"]
                }
            }"#,
        )
        .expect("legacy settings fixture should deserialize");

        assert!(settings.show_saved_queries);
    }

    #[test]
    fn codestral_api_key_is_not_serialized_to_plaintext_settings() {
        let mut settings = AppUiSettings::default();
        settings.codestral.api_key = "top-secret".to_string();

        let serialized = serde_json::to_string(&settings).expect("settings should serialize");

        assert!(!serialized.contains("top-secret"));
        assert!(!serialized.contains("\"api_key\""));
    }

    #[test]
    fn deepseek_api_key_is_not_serialized_to_plaintext_settings() {
        let mut settings = AppUiSettings::default();
        settings.deepseek.api_key = "deepseek-secret".to_string();

        let serialized = serde_json::to_string(&settings).expect("settings should serialize");

        assert!(!serialized.contains("deepseek-secret"));
        assert!(!serialized.contains("\"api_key\""));
    }

    #[test]
    fn legacy_codestral_api_key_still_deserializes_for_migration() {
        let settings: AppUiSettings = serde_json::from_str(
            r#"{
                "theme":"Dark",
                "ai_features_enabled":true,
                "restore_session_on_launch":true,
                "show_saved_queries":true,
                "show_connections":false,
                "show_explorer":true,
                "show_history":false,
                "show_sql_editor":false,
                "show_agent_panel":false,
                "default_page_size":100,
                "tool_panel_layout":{
                    "sidebar":["Connections","Explorer","SavedQueries","History"],
                    "inspector":["Agent"]
                },
                "codestral":{
                    "enabled":true,
                    "api_key":"legacy-secret",
                    "model":"codestral-latest"
                }
            }"#,
        )
        .expect("legacy settings fixture should deserialize");

        assert_eq!(settings.codestral.api_key, "legacy-secret");
    }

    #[test]
    fn legacy_deepseek_api_key_still_deserializes_for_migration() {
        let settings: AppUiSettings = serde_json::from_str(
            r#"{
                "theme":"Dark",
                "ai_features_enabled":true,
                "restore_session_on_launch":true,
                "show_saved_queries":true,
                "show_connections":false,
                "show_explorer":true,
                "show_history":false,
                "show_sql_editor":false,
                "show_agent_panel":false,
                "default_page_size":100,
                "tool_panel_layout":{
                    "sidebar":["Connections","Explorer","SavedQueries","History"],
                    "inspector":["Agent"]
                },
                "deepseek":{
                    "enabled":true,
                    "api_key":"legacy-deepseek-secret",
                    "base_url":"https://api.deepseek.com",
                    "model":"deepseek-v4-pro",
                    "thinking_enabled":true,
                    "reasoning_effort":"high"
                }
            }"#,
        )
        .expect("legacy settings fixture should deserialize");

        assert_eq!(settings.deepseek.api_key, "legacy-deepseek-secret");
    }

    #[test]
    fn toggle_single_field_round_trip_preserves_all_persisted_fields() {
        type ToggleFn = Box<dyn Fn(&mut AppUiSettings)>;

        let mut settings = AppUiSettings {
            theme: AppThemePreference::Light,
            density: UiDensity::Comfortable,
            ai_features_enabled: false,
            restore_session_on_launch: false,
            read_only_mode: true,
            show_saved_queries: false,
            show_connections: true,
            show_explorer: false,
            show_history: true,
            show_sql_editor: true,
            show_agent_panel: true,
            default_page_size: 250,
            // Default for ai_auto_apply_completions is `true`; flip it so the
            // round-trip explicitly exercises the new field.
            ai_auto_apply_completions: false,
            ..AppUiSettings::default()
        };
        settings.codestral.enabled = true;
        settings.codestral.model = "codestral-22b".to_string();
        settings.deepseek.enabled = true;
        settings.deepseek.model = "deepseek-v4-flash".to_string();
        settings.deepseek.thinking_enabled = true;
        settings.deepseek.reasoning_effort = "high".to_string();
        settings.ai_response_language = "Deutsch".to_string();

        let toggle_mutations: Vec<(&str, ToggleFn)> = vec![
            ("theme", Box::new(|s| s.theme = AppThemePreference::Dark)),
            (
                "ai_features_enabled",
                Box::new(|s| s.ai_features_enabled = true),
            ),
            (
                "restore_session_on_launch",
                Box::new(|s| s.restore_session_on_launch = true),
            ),
            ("read_only_mode", Box::new(|s| s.read_only_mode = false)),
            (
                "show_saved_queries",
                Box::new(|s| s.show_saved_queries = true),
            ),
            ("show_connections", Box::new(|s| s.show_connections = false)),
            ("show_explorer", Box::new(|s| s.show_explorer = true)),
            ("show_history", Box::new(|s| s.show_history = false)),
            ("show_sql_editor", Box::new(|s| s.show_sql_editor = false)),
            ("show_agent_panel", Box::new(|s| s.show_agent_panel = false)),
            ("default_page_size", Box::new(|s| s.default_page_size = 500)),
        ];

        for (field_name, mutate) in toggle_mutations {
            mutate(&mut settings);
            let serialized = serde_json::to_string(&settings).expect("settings should serialize");
            let reloaded: AppUiSettings = serde_json::from_str(&serialized)
                .unwrap_or_else(|err| panic!("settings should reload after {field_name}: {err}"));

            // The toggled field already moved to its new value; assert the
            // *rest* of the fields still match the prior mutated state.
            // We compare against the in-memory `settings` snapshot pre-toggle.
            // For brevity, just assert the structural fields survived.
            assert_eq!(
                reloaded.theme, settings.theme,
                "{field_name} toggle dropped theme"
            );
            assert_eq!(
                reloaded.density, settings.density,
                "{field_name} toggle dropped density"
            );
            assert_eq!(
                reloaded.ai_features_enabled, settings.ai_features_enabled,
                "{field_name} toggle dropped ai_features_enabled"
            );
            assert_eq!(
                reloaded.restore_session_on_launch, settings.restore_session_on_launch,
                "{field_name} toggle dropped restore_session_on_launch"
            );
            assert_eq!(
                reloaded.read_only_mode, settings.read_only_mode,
                "{field_name} toggle dropped read_only_mode"
            );
            assert_eq!(
                reloaded.show_saved_queries, settings.show_saved_queries,
                "{field_name} toggle dropped show_saved_queries"
            );
            assert_eq!(
                reloaded.show_connections, settings.show_connections,
                "{field_name} toggle dropped show_connections"
            );
            assert_eq!(
                reloaded.show_explorer, settings.show_explorer,
                "{field_name} toggle dropped show_explorer"
            );
            assert_eq!(
                reloaded.show_history, settings.show_history,
                "{field_name} toggle dropped show_history"
            );
            assert_eq!(
                reloaded.show_sql_editor, settings.show_sql_editor,
                "{field_name} toggle dropped show_sql_editor"
            );
            assert_eq!(
                reloaded.show_agent_panel, settings.show_agent_panel,
                "{field_name} toggle dropped show_agent_panel"
            );
            assert_eq!(
                reloaded.default_page_size, settings.default_page_size,
                "{field_name} toggle dropped default_page_size"
            );
            assert_eq!(
                reloaded.ai_response_language, settings.ai_response_language,
                "{field_name} toggle dropped ai_response_language"
            );
            assert_eq!(
                reloaded.codestral.enabled, settings.codestral.enabled,
                "{field_name} toggle dropped codestral.enabled"
            );
            assert_eq!(
                reloaded.codestral.model, settings.codestral.model,
                "{field_name} toggle dropped codestral.model"
            );
            assert_eq!(
                reloaded.deepseek.enabled, settings.deepseek.enabled,
                "{field_name} toggle dropped deepseek.enabled"
            );
            assert_eq!(
                reloaded.deepseek.model, settings.deepseek.model,
                "{field_name} toggle dropped deepseek.model"
            );
            assert_eq!(
                reloaded.deepseek.thinking_enabled, settings.deepseek.thinking_enabled,
                "{field_name} toggle dropped deepseek.thinking_enabled"
            );
            assert_eq!(
                reloaded.deepseek.reasoning_effort, settings.deepseek.reasoning_effort,
                "{field_name} toggle dropped deepseek.reasoning_effort"
            );
            assert_eq!(
                reloaded.tool_panel_layout, settings.tool_panel_layout,
                "{field_name} toggle dropped tool_panel_layout"
            );
            assert_eq!(
                reloaded.ai_auto_apply_completions, settings.ai_auto_apply_completions,
                "{field_name} toggle dropped ai_auto_apply_completions"
            );
        }
    }
}
