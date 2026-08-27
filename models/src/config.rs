use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::{
    AppBehavior,
    AppThemePreference,
    AppUiSettings,
    ClickHouseFormData,
    CodeStralSettings,
    ConnectionRequest,
    DeepSeekSettings,
    EditorBehavior,
    ExplorerViewSettings,
    GridSettings,
    KeybindingMap,
    MySqlFormData,
    OllamaSettings,
    PanelBehavior,
    PostgresFormData,
    SqliteFormData,
    ThemeOverrides,
    UiDensity,
    WorkspaceSplitMode,
    WorkspaceToolLayout,
};

/// A single connection declared in `config.toml`. Mirrors the fields the
/// connect screen collects, so a connection can be fully defined in the
/// config file and auto-connect on launch.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConfigConnection {
    pub name: String,
    pub kind: String,
    pub host: String,
    pub port: Option<u16>,
    pub database: String,
    pub username: String,
    pub password: String,
    pub ssl: bool,
    pub file: String,
    pub auto_connect: bool,
}

impl ConfigConnection {
    /// Build a [`ConnectionRequest`] from this config entry. Returns `None`
    /// when the `kind` is not a recognized database type.
    pub fn to_request(&self) -> Option<ConnectionRequest> {
        let kind = self.kind.trim().to_ascii_lowercase();
        match kind.as_str() {
            "sqlite" | "sqlite3" => Some(ConnectionRequest::Sqlite(SqliteFormData {
                path: self.file.trim().to_string(),
            })),
            "postgres" | "postgresql" | "pg" =>
                Some(ConnectionRequest::Postgres(PostgresFormData {
                    host: self.host.trim().to_string(),
                    port: self.port.unwrap_or(5432),
                    username: self.username.trim().to_string(),
                    password: self.password.clone(),
                    database: self.database.trim().to_string(),
                    ssh_tunnel: None,
                    ssl_mode: if self.ssl {
                        "require".to_string()
                    } else {
                        "prefer".to_string()
                    },
                })),
            "mysql" => Some(ConnectionRequest::MySql(MySqlFormData {
                host: self.host.trim().to_string(),
                port: self.port.unwrap_or(3306),
                username: self.username.trim().to_string(),
                password: self.password.clone(),
                database: self.database.trim().to_string(),
                ssh_tunnel: None,
                ssl_mode: if self.ssl {
                    "required".to_string()
                } else {
                    "preferred".to_string()
                },
            })),
            "clickhouse" | "click-house" =>
                Some(ConnectionRequest::ClickHouse(ClickHouseFormData {
                    host: self.host.trim().to_string(),
                    port: self.port.unwrap_or(8123),
                    username: self.username.trim().to_string(),
                    password: self.password.clone(),
                    database: self.database.trim().to_string(),
                    ssh_tunnel: None,
                })),
            _ => None,
        }
    }
}

/// Top-level `config.toml` document. Every field is optional so a partial
/// config file only overrides the settings it mentions; the rest fall back
/// to the persisted JSON settings.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShovelConfig {
    // ── Appearance ────────────────────────────────────────────────
    pub theme: Option<AppThemePreference>,
    pub density: Option<UiDensity>,
    pub split_mode: Option<WorkspaceSplitMode>,

    // ── Workspace / panels ─────────────────────────────────────────
    pub tool_panel_layout: Option<WorkspaceToolLayout>,
    pub default_page_size: Option<u32>,
    pub restore_session_on_launch: Option<bool>,
    pub read_only_mode: Option<bool>,
    pub ai_features_enabled: Option<bool>,
    pub show_agent_panel: Option<bool>,
    pub show_saved_queries: Option<bool>,
    pub show_connections: Option<bool>,
    pub show_explorer: Option<bool>,
    pub show_history: Option<bool>,
    pub show_sql_editor: Option<bool>,
    pub show_bottom_panel: Option<bool>,
    pub bottom_panel_height: Option<f64>,

    // ── AI / language ──────────────────────────────────────────────
    pub ai_response_language: Option<String>,
    pub ai_auto_apply_completions: Option<bool>,
    pub codestral: Option<CodeStralSettings>,
    pub deepseek: Option<DeepSeekSettings>,
    pub ollama: Option<OllamaSettings>,

    // ── Explorer ──────────────────────────────────────────────────
    pub explorer: Option<ExplorerViewSettings>,

    // ── Deep customization ─────────────────────────────────────────
    pub theme_overrides: Option<ThemeOverrides>,
    pub keybindings: Option<KeybindingMap>,
    pub editor: Option<EditorBehavior>,
    pub grid: Option<GridSettings>,
    pub panels: Option<PanelBehavior>,
    pub behavior: Option<AppBehavior>,

    // ── Connections ────────────────────────────────────────────────
    pub connections: Vec<ConfigConnection>,
}

impl ShovelConfig {
    /// Load and parse `config.toml` from the given path. Returns `None`
    /// when the file does not exist; returns an error when it exists but
    /// cannot be parsed.
    pub fn load(path: &PathBuf) -> Result<Option<Self>, String> {
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(format!("failed to read {}: {err}", path.display())),
        };
        toml::from_str(&content)
            .map(Some)
            .map_err(|err| format!("failed to parse {}: {err}", path.display()))
    }

    /// Serialize this config to a TOML string.
    pub fn to_toml(&self) -> Result<String, String> {
        toml::to_string_pretty(self).map_err(|err| format!("failed to serialize config: {err}"))
    }

    /// Write this config to the given path, creating parent directories.
    pub fn save(&self, path: &PathBuf) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| format!("failed to create config dir: {err}"))?;
        }
        let toml = self.to_toml()?;
        std::fs::write(path, toml)
            .map_err(|err| format!("failed to write {}: {err}", path.display()))
    }

    /// Merge this config over `base`, producing the effective settings.
    /// Fields that are `None` (or empty) leave the base value untouched.
    pub fn apply_to(&self, base: &mut AppUiSettings) {
        if let Some(theme) = self.theme {
            base.theme = theme;
        }
        if let Some(density) = self.density {
            base.density = density;
        }
        if let Some(split_mode) = self.split_mode {
            base.split_mode = split_mode;
        }
        if let Some(layout) = &self.tool_panel_layout {
            base.tool_panel_layout = layout.normalized();
        }
        if let Some(page_size) = self.default_page_size {
            base.default_page_size = page_size;
        }
        if let Some(restore) = self.restore_session_on_launch {
            base.restore_session_on_launch = restore;
        }
        if let Some(read_only) = self.read_only_mode {
            base.read_only_mode = read_only;
        }
        if let Some(ai) = self.ai_features_enabled {
            base.ai_features_enabled = ai;
        }
        if let Some(show) = self.show_agent_panel {
            base.show_agent_panel = show;
        }
        if let Some(show) = self.show_saved_queries {
            base.show_saved_queries = show;
        }
        if let Some(show) = self.show_connections {
            base.show_connections = show;
        }
        if let Some(show) = self.show_explorer {
            base.show_explorer = show;
        }
        if let Some(show) = self.show_history {
            base.show_history = show;
        }
        if let Some(show) = self.show_sql_editor {
            base.show_sql_editor = show;
        }
        if let Some(show) = self.show_bottom_panel {
            base.show_bottom_panel = show;
        }
        if let Some(height) = self.bottom_panel_height {
            base.bottom_panel_height = height;
        }
        if let Some(language) = &self.ai_response_language
            && !language.trim().is_empty()
        {
            base.ai_response_language = language.clone();
        }
        if let Some(auto) = self.ai_auto_apply_completions {
            base.ai_auto_apply_completions = auto;
        }
        if let Some(codestral) = &self.codestral {
            merge_codestral(&mut base.codestral, codestral);
        }
        if let Some(deepseek) = &self.deepseek {
            merge_deepseek(&mut base.deepseek, deepseek);
        }
        if let Some(ollama) = &self.ollama {
            merge_ollama(&mut base.ollama, ollama);
        }
        if let Some(explorer) = &self.explorer {
            merge_explorer(&mut base.explorer, explorer);
        }
        if let Some(theme) = &self.theme_overrides {
            base.theme_overrides = theme.clone();
        }
        if let Some(map) = &self.keybindings {
            base.keybindings = map.clone();
        }
        if let Some(grid) = &self.grid {
            base.grid = grid.clone();
        }
        if let Some(editor) = &self.editor {
            if let Some(v) = editor.font_size {
                base.editor.font_size = v.clamp(10, 22);
            }
            if let Some(v) = editor.tab_size {
                base.editor.tab_size = v.clamp(1, 8);
            }
            if let Some(v) = editor.auto_format_on_run {
                base.editor.auto_format_on_run = v;
            }
            if let Some(v) = editor.word_wrap {
                base.editor.word_wrap = v;
            }
            if let Some(v) = editor.show_line_numbers {
                base.editor.show_line_numbers = v;
            }
        }
        if let Some(behavior) = &self.behavior {
            if let Some(v) = behavior.confirm_before_drop {
                base.behavior.confirm_before_drop = v;
            }
            if let Some(v) = behavior.confirm_before_truncate {
                base.behavior.confirm_before_truncate = v;
            }
        }
    }
}

fn merge_codestral(target: &mut CodeStralSettings, source: &CodeStralSettings) {
    if source.enabled {
        target.enabled = true;
    }
    if !source.api_key.trim().is_empty() {
        target.api_key = source.api_key.clone();
    }
    if !source.model.trim().is_empty() {
        target.model = source.model.clone();
    }
}

fn merge_explorer(target: &mut ExplorerViewSettings, source: &ExplorerViewSettings) {
    if source.show_schemas {
        target.show_schemas = true;
    }
    if source.show_tables {
        target.show_tables = true;
    }
    if source.show_views {
        target.show_views = true;
    }
    if source.show_columns {
        target.show_columns = true;
    }
    if source.show_system_objects {
        target.show_system_objects = true;
    }
    if source.show_row_counts {
        target.show_row_counts = true;
    }
    if source.sort_alphabetical {
        target.sort_alphabetical = true;
    }
}

fn merge_deepseek(target: &mut DeepSeekSettings, source: &DeepSeekSettings) {
    if source.enabled {
        target.enabled = true;
    }
    if !source.api_key.trim().is_empty() {
        target.api_key = source.api_key.clone();
    }
    if !source.base_url.trim().is_empty() {
        target.base_url = source.base_url.clone();
    }
    if !source.model.trim().is_empty() {
        target.model = source.model.clone();
    }
    if source.thinking_enabled {
        target.thinking_enabled = true;
    }
    if !source.reasoning_effort.trim().is_empty() {
        target.reasoning_effort = source.reasoning_effort.clone();
    }
}

fn merge_ollama(target: &mut OllamaSettings, source: &OllamaSettings) {
    if source.enabled {
        target.enabled = true;
    }
    if !source.api_key.trim().is_empty() {
        target.api_key = source.api_key.clone();
    }
    if !source.base_url.trim().is_empty() {
        target.base_url = source.base_url.clone();
    }
    if !source.model.trim().is_empty() {
        target.model = source.model.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppUiSettings;

    #[test]
    fn empty_config_leaves_settings_untouched() {
        let config = ShovelConfig::default();
        let mut settings = AppUiSettings::default();
        let original = settings.clone();
        config.apply_to(&mut settings);
        assert_eq!(settings, original);
    }

    #[test]
    fn config_overrides_theme_and_page_size() {
        let config = ShovelConfig {
            theme: Some(AppThemePreference::Light),
            default_page_size: Some(500),
            ..ShovelConfig::default()
        };
        let mut settings = AppUiSettings::default();
        config.apply_to(&mut settings);
        assert_eq!(settings.theme, AppThemePreference::Light);
        assert_eq!(settings.default_page_size, 500);
    }

    #[test]
    fn config_merges_ollama_model() {
        let config = ShovelConfig {
            ollama: Some(OllamaSettings {
                enabled: true,
                model: "qwen3:latest".to_string(),
                ..OllamaSettings::default()
            }),
            ..ShovelConfig::default()
        };
        let mut settings = AppUiSettings::default();
        config.apply_to(&mut settings);
        assert!(settings.ollama.enabled);
        assert_eq!(settings.ollama.model, "qwen3:latest");
    }

    #[test]
    fn parses_toml_document() {
        let toml_str = r#"
theme = "Light"
default_page_size = 250

[ollama]
enabled = true
model = "qwen3:latest"

[[connections]]
name = "Local"
kind = "sqlite"
file = "/tmp/test.db"
auto_connect = true
"#;
        let config: ShovelConfig = toml::from_str(toml_str).expect("parse");
        assert_eq!(config.theme, Some(AppThemePreference::Light));
        assert_eq!(config.default_page_size, Some(250));
        assert!(config.ollama.as_ref().is_some_and(|o| o.enabled));
        assert_eq!(config.connections.len(), 1);
        assert_eq!(config.connections[0].name, "Local");
        assert!(config.connections[0].auto_connect);
    }

    #[test]
    fn config_covers_all_panel_visibility_toggles() {
        let config = ShovelConfig {
            show_saved_queries: Some(false),
            show_connections: Some(true),
            show_explorer: Some(false),
            show_history: Some(true),
            show_sql_editor: Some(true),
            show_bottom_panel: Some(false),
            bottom_panel_height: Some(240.0),
            ..ShovelConfig::default()
        };
        let mut settings = AppUiSettings::default();
        config.apply_to(&mut settings);
        assert!(!settings.show_saved_queries);
        assert!(settings.show_connections);
        assert!(!settings.show_explorer);
        assert!(settings.show_history);
        assert!(settings.show_sql_editor);
        assert!(!settings.show_bottom_panel);
        assert_eq!(settings.bottom_panel_height, 240.0);
    }

    #[test]
    fn config_merges_explorer_view_settings() {
        let config = ShovelConfig {
            explorer: Some(ExplorerViewSettings {
                show_columns: true,
                show_system_objects: true,
                sort_alphabetical: true,
                ..ExplorerViewSettings::default()
            }),
            ..ShovelConfig::default()
        };
        let mut settings = AppUiSettings::default();
        config.apply_to(&mut settings);
        assert!(settings.explorer.show_columns);
        assert!(settings.explorer.show_system_objects);
        assert!(settings.explorer.sort_alphabetical);
    }

    #[test]
    fn parses_full_config_with_all_sections() {
        let toml_str = r#"
theme = "Light"
density = "Comfortable"
split_mode = "Horizontal"
default_page_size = 250
show_agent_panel = true
show_bottom_panel = false
bottom_panel_height = 200.0
ai_auto_apply_completions = false

[codestral]
enabled = true
model = "codestral-latest"

[deepseek]
enabled = true
model = "deepseek-chat"

[ollama]
enabled = true
model = "qwen3:latest"

[explorer]
show_columns = true
sort_alphabetical = true

[[connections]]
name = "Test"
kind = "sqlite"
file = "/tmp/test.db"
auto_connect = true
"#;
        let config: ShovelConfig = toml::from_str(toml_str).expect("parse");
        assert_eq!(config.theme, Some(AppThemePreference::Light));
        assert_eq!(config.density, Some(UiDensity::Comfortable));
        assert_eq!(config.split_mode, Some(WorkspaceSplitMode::Horizontal));
        assert_eq!(config.default_page_size, Some(250));
        assert_eq!(config.show_agent_panel, Some(true));
        assert_eq!(config.show_bottom_panel, Some(false));
        assert_eq!(config.bottom_panel_height, Some(200.0));
        assert_eq!(config.ai_auto_apply_completions, Some(false));
        assert!(config.codestral.as_ref().is_some_and(|c| c.enabled));
        assert!(config.deepseek.as_ref().is_some_and(|d| d.enabled));
        assert!(config.ollama.as_ref().is_some_and(|o| o.enabled));
        assert!(config.explorer.as_ref().is_some_and(|e| e.show_columns));
        assert_eq!(config.connections.len(), 1);
        assert_eq!(config.connections[0].kind, "sqlite");
    }

    #[test]
    fn parses_deep_customization_sections() {
        let toml_str = r##"
[theme_overrides]
primary = "#ff0000"
font_size = 14
radius_medium = 8

[keybindings]
format_sql = "Ctrl+Shift+F"
new_tab = "Ctrl+T"

[editor]
font_size = 14
tab_size = 4
auto_format_on_run = true

[panels]
auto_open_explorer = true
default_sidebar_width = 300

[behavior]
auto_connect_on_launch = true
confirm_before_drop = true
"##;
        let config: ShovelConfig = toml::from_str(toml_str).expect("parse");
        let theme = config.theme_overrides.expect("theme_overrides");
        assert_eq!(theme.primary.as_deref(), Some("#ff0000"));
        assert_eq!(theme.font_size, Some(14));
        assert_eq!(theme.radius_medium, Some(8));

        let kb = config.keybindings.expect("keybindings");
        assert_eq!(
            kb.get("format_sql").map(String::as_str),
            Some("Ctrl+Shift+F")
        );
        assert_eq!(kb.get("new_tab").map(String::as_str), Some("Ctrl+T"));

        let editor = config.editor.expect("editor");
        assert_eq!(editor.font_size, Some(14));
        assert_eq!(editor.tab_size, Some(4));
        assert_eq!(editor.auto_format_on_run, Some(true));

        let panels = config.panels.expect("panels");
        assert_eq!(panels.auto_open_explorer, Some(true));
        assert_eq!(panels.default_sidebar_width, Some(300));

        let behavior = config.behavior.expect("behavior");
        assert_eq!(behavior.auto_connect_on_launch, Some(true));
        assert_eq!(behavior.confirm_before_drop, Some(true));
    }

    #[test]
    fn theme_overrides_render_to_css() {
        let theme = ThemeOverrides {
            primary: Some("#ff0000".to_string()),
            font_size: Some(14),
            radius_medium: Some(8),
            ..ThemeOverrides::default()
        };
        let css = theme.to_css();
        assert!(css.contains("--color-primary: #ff0000;"));
        assert!(css.contains("--ui-font-size: 14px;"));
        assert!(css.contains("--radius-md: 8px;"));
    }

    #[test]
    fn config_overlays_editor_and_keybindings() {
        let toml_str = r##"
[editor]
font_size = 16
word_wrap = true

[behavior]
confirm_before_drop = false

[theme_overrides]
primary = "#00ff00"

[keybindings]
format_sql = "Ctrl+Alt+F"

[grid]
row_height = 32
zebra = true
"##;
        let config: ShovelConfig = toml::from_str(toml_str).expect("parse");
        let mut settings = AppUiSettings::default();
        config.apply_to(&mut settings);
        assert_eq!(settings.editor.font_size, 16);
        assert!(settings.editor.word_wrap);
        assert!(!settings.behavior.confirm_before_drop);
        assert_eq!(settings.theme_overrides.primary.as_deref(), Some("#00ff00"));
        assert_eq!(
            settings.keybindings.get("format_sql").map(String::as_str),
            Some("Ctrl+Alt+F")
        );
        assert_eq!(settings.grid.row_height, 32);
        assert!(settings.grid.zebra);
    }
}
