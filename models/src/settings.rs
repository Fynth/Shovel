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
}

impl Default for AppUiSettings {
    fn default() -> Self {
        Self {
            theme: AppThemePreference::Dark,
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AppUiSettings;

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
}
