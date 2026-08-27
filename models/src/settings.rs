use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{KeybindingMap, ThemeOverrides};

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

/// How the central editor area (SQL editor + result grid) of the active
/// query tab is laid out. Mirrors DBeaver's editor / result layout
/// switcher:
/// - `Off` keeps the default single-pane stack: editor on top, result
///   below, with the existing vertical resize handle between them.
/// - `Horizontal` places the SQL editor on the left and the result
///   grid on the right, side-by-side, with a column-resize handle
///   between them.
/// - `Vertical` is identical to `Off` in geometry but renders an
///   explicit "split" affordance (divider + grab bar) so the user
///   sees the layout as a true split rather than a single pane with
///   a drag handle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkspaceSplitMode {
    #[default]
    Off,
    Horizontal,
    Vertical,
}

impl WorkspaceSplitMode {
    /// All variants in display order. Used by the settings modal's
    /// segmented control so adding a new variant only needs a code
    /// change here.
    pub const ALL: [Self; 3] = [Self::Off, Self::Horizontal, Self::Vertical];

    /// CSS class applied to the active tab body. The class is a
    /// no-op for `Off` (default behavior) so legacy layouts render
    /// unchanged.
    pub fn css_class(self) -> &'static str {
        match self {
            Self::Off => "split-mode-off",
            Self::Horizontal => "split-mode-horizontal",
            Self::Vertical => "split-mode-vertical",
        }
    }

    /// Human-readable label for the settings modal segmented control
    /// and the toolbar cycle button.
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Single pane",
            Self::Horizontal => "Side by side",
            Self::Vertical => "Stacked split",
        }
    }

    /// Short, dense label suitable for a toolbar button.
    pub fn short_label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Horizontal => "Side",
            Self::Vertical => "Stack",
        }
    }

    /// Next mode in the cycle, used by the toolbar button to step
    /// through `Off -> Horizontal -> Vertical -> Off` on each click.
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Horizontal,
            Self::Horizontal => Self::Vertical,
            Self::Vertical => Self::Off,
        }
    }
}

/// Per-object-type display options for the connection explorer tree
/// (left panel). Mirrors the view-settings pane in DBeaver / DataGrip:
/// each toggle gates a slice of the rendered tree without re-querying
/// the database, and the whole struct is persisted as part of
/// [`AppUiSettings`] so user preferences survive a restart.
///
/// Defaults track the DBeaver starting state:
/// - schemas, tables, views and row-count badges are visible
/// - column children, system schemas (`pg_catalog`, `information_schema`,
///   `INFORMATION_SCHEMA`, `mysql`, `performance_schema`, `sys`,
///   `system`, etc.) and alphabetical ordering are off by default —
///   the existing tree shows objects in driver-natural order (typically
///   load order or alphabetical depending on the backend)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExplorerViewSettings {
    /// Render schema-level nodes at all. Turning this off collapses every
    /// schema into a flat object list (driven by driver default schema).
    pub show_schemas: bool,
    /// Render the "Tables" group. Off hides every table in the tree.
    pub show_tables: bool,
    /// Render the "Views" + "Materialized Views" groups.
    pub show_views: bool,
    /// Render column children under each table. Column data is loaded
    /// by the explorer backend; when this is off the renderer skips
    /// the column children even if they are present.
    pub show_columns: bool,
    /// Include well-known system schemas/objects (`pg_catalog`,
    /// `information_schema`, `INFORMATION_SCHEMA`, `mysql`,
    /// `performance_schema`, `sys`, `system`). When off these are
    /// filtered out at the UI level on top of the SQL-level filter
    /// the driver already applies.
    pub show_system_objects: bool,
    /// Render the `(≈N)` row-count badge next to tables. Independent
    /// of whether the backend was able to populate the count.
    pub show_row_counts: bool,
    /// Sort group members alphabetically by name. When off the
    /// natural driver order is preserved (existing behaviour).
    pub sort_alphabetical: bool,
}

impl Default for ExplorerViewSettings {
    fn default() -> Self {
        Self {
            show_schemas: true,
            show_tables: true,
            show_views: true,
            show_columns: false,
            show_system_objects: false,
            show_row_counts: true,
            sort_alphabetical: false,
        }
    }
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

/// Persisted configuration for the embedded Ollama ACP bridge. Stored in
/// `AppUiSettings` so the user configures Ollama once and it can be
/// auto-connected on launch, mirroring `DeepSeekSettings`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OllamaSettings {
    pub enabled: bool,
    #[serde(skip_serializing)]
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

impl Default for OllamaSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key: String::new(),
            base_url: "http://localhost:11434/api".to_string(),
            model: String::new(),
        }
    }
}

impl OllamaSettings {
    pub fn keyring_service() -> &'static str {
        "shovel.ollama"
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorSettings {
    pub font_size: u32,
    pub tab_size: u32,
    pub auto_format_on_run: bool,
    pub word_wrap: bool,
    pub show_line_numbers: bool,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            font_size: 13,
            tab_size: 2,
            auto_format_on_run: false,
            word_wrap: false,
            show_line_numbers: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NullDisplay {
    #[default]
    Literal,
    Empty,
    EmDash,
}

impl NullDisplay {
    pub const ALL: [Self; 3] = [Self::Literal, Self::Empty, Self::EmDash];

    pub fn label(self) -> &'static str {
        match self {
            Self::Literal => "NULL",
            Self::Empty => "Empty",
            Self::EmDash => "—",
        }
    }
}

pub fn format_null_display(raw: &str, mode: NullDisplay) -> String {
    let is_null = raw.is_empty() || raw.eq_ignore_ascii_case("null");
    if !is_null {
        return raw.to_string();
    }
    match mode {
        NullDisplay::Literal => "NULL".to_string(),
        NullDisplay::Empty => String::new(),
        NullDisplay::EmDash => "—".to_string(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GridSettings {
    pub row_height: u32,
    pub zebra: bool,
    pub null_display: NullDisplay,
    pub wrap_cells: bool,
}

impl Default for GridSettings {
    fn default() -> Self {
        Self {
            row_height: 28,
            zebra: false,
            null_display: NullDisplay::Literal,
            wrap_cells: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SqlCompletionSettings {
    pub provider: String,
    pub model: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppBehaviorSettings {
    pub confirm_before_drop: bool,
    pub confirm_before_truncate: bool,
}

impl Default for AppBehaviorSettings {
    fn default() -> Self {
        Self {
            confirm_before_drop: true,
            confirm_before_truncate: true,
        }
    }
}

/// Built-in OpenAI-compatible chat providers (API key + base URL + model).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OpenAiCompatProvider {
    OpenAi,
    Groq,
    OpenRouter,
    XAi,
    Mistral,
}

impl OpenAiCompatProvider {
    pub const ALL: [Self; 5] = [
        Self::OpenAi,
        Self::Groq,
        Self::OpenRouter,
        Self::XAi,
        Self::Mistral,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Groq => "groq",
            Self::OpenRouter => "openrouter",
            Self::XAi => "xai",
            Self::Mistral => "mistral",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI",
            Self::Groq => "Groq",
            Self::OpenRouter => "OpenRouter",
            Self::XAi => "xAI",
            Self::Mistral => "Mistral",
        }
    }

    pub fn default_base_url(self) -> &'static str {
        match self {
            Self::OpenAi => "https://api.openai.com",
            Self::Groq => "https://api.groq.com/openai",
            Self::OpenRouter => "https://openrouter.ai/api",
            Self::XAi => "https://api.x.ai",
            Self::Mistral => "https://api.mistral.ai",
        }
    }

    pub fn default_model(self) -> &'static str {
        self.models().first().copied().unwrap_or("gpt-5.6-luna")
    }

    pub fn models(self) -> &'static [&'static str] {
        match self {
            Self::OpenAi => &[
                "gpt-5.6-sol",
                "gpt-5.6-terra",
                "gpt-5.6-luna",
                "gpt-5.3-codex",
            ],
            Self::Groq => &[
                "llama-3.3-70b-versatile",
                "openai/gpt-oss-120b",
                "qwen/qwen3-32b",
            ],
            Self::OpenRouter => &[
                "openai/gpt-5.6-sol",
                "anthropic/claude-opus-5",
                "google/gemini-3.7-flash",
            ],
            Self::XAi => &["grok-4.6", "grok-4.5", "grok-4.3", "grok-build-0.1"],
            Self::Mistral => &[
                "mistral-medium-latest",
                "mistral-large-latest",
                "codestral-latest",
            ],
        }
    }

    pub fn keyring_service(self) -> &'static str {
        match self {
            Self::OpenAi => "shovel.openai",
            Self::Groq => "shovel.groq",
            Self::OpenRouter => "shovel.openrouter",
            Self::XAi => "shovel.xai",
            Self::Mistral => "shovel.mistral",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OpenAiCompatSettings {
    pub enabled: bool,
    #[serde(skip_serializing)]
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

impl OpenAiCompatSettings {
    fn for_provider(provider: OpenAiCompatProvider) -> Self {
        Self {
            enabled: false,
            api_key: String::new(),
            base_url: provider.default_base_url().to_string(),
            model: provider.default_model().to_string(),
        }
    }

    pub fn to_deepseek_bridge(self) -> DeepSeekSettings {
        DeepSeekSettings {
            enabled: self.enabled,
            api_key: self.api_key,
            base_url: self.base_url,
            model: self.model,
            thinking_enabled: false,
            reasoning_effort: "medium".to_string(),
        }
    }
}

impl Default for OpenAiCompatSettings {
    fn default() -> Self {
        Self::for_provider(OpenAiCompatProvider::OpenAi)
    }
}

fn default_openai_settings() -> OpenAiCompatSettings {
    OpenAiCompatSettings::for_provider(OpenAiCompatProvider::OpenAi)
}

fn default_groq_settings() -> OpenAiCompatSettings {
    OpenAiCompatSettings::for_provider(OpenAiCompatProvider::Groq)
}

fn default_openrouter_settings() -> OpenAiCompatSettings {
    OpenAiCompatSettings::for_provider(OpenAiCompatProvider::OpenRouter)
}

fn default_xai_settings() -> OpenAiCompatSettings {
    OpenAiCompatSettings::for_provider(OpenAiCompatProvider::XAi)
}

fn default_mistral_settings() -> OpenAiCompatSettings {
    OpenAiCompatSettings::for_provider(OpenAiCompatProvider::Mistral)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    /// Legacy vendor blob — deserialize for migration only; catalog is source of truth.
    #[serde(default, skip_serializing)]
    pub deepseek: DeepSeekSettings,
    #[serde(default, skip_serializing)]
    pub ollama: OllamaSettings,
    #[serde(default = "default_openai_settings", skip_serializing)]
    pub openai: OpenAiCompatSettings,
    #[serde(default = "default_groq_settings", skip_serializing)]
    pub groq: OpenAiCompatSettings,
    #[serde(default = "default_openrouter_settings", skip_serializing)]
    pub openrouter: OpenAiCompatSettings,
    #[serde(default = "default_xai_settings", skip_serializing)]
    pub xai: OpenAiCompatSettings,
    #[serde(default = "default_mistral_settings", skip_serializing)]
    pub mistral: OpenAiCompatSettings,
    #[serde(default)]
    pub ai_catalog: crate::AiCatalogSettings,
    /// In-memory LM API keys keyed by provider id (`deepseek`, `custom:…`).
    /// Never written to JSON — persisted via keyring `shovel.lm.<id>`.
    #[serde(skip)]
    pub lm_keys: BTreeMap<String, String>,
    pub ai_response_language: String,
    /// When `true`, inline AI completions are inserted automatically
    /// after the user stops typing for a short idle pause; otherwise
    /// completions stay as ghost text until the user presses Tab.
    pub ai_auto_apply_completions: bool,
    /// Provider/model used for SQL editor ghost-text completions.
    pub sql_completion: SqlCompletionSettings,

    pub explorer: ExplorerViewSettings,

    /// Bottom dock visibility (Output / Messages / Query Log / Transactions
    /// / Problems). Persisted alongside the tool-panel toggles so the user
    /// can decide once whether the dock starts open or closed.
    pub show_bottom_panel: bool,
    /// Persisted height of the bottom dock in pixels. Used to restore the
    /// user's last resize without coupling the layout to a CSS-only value.
    pub bottom_panel_height: f64,

    /// Layout of the central editor area (SQL editor + result grid) inside
    /// the active query tab. Mirrors DBeaver's editor / result switcher
    /// (`Off` = single stacked pane, `Horizontal` = side-by-side,
    /// `Vertical` = stacked with explicit split affordance). The default
    /// is `Off` so existing installs are visually unchanged.
    pub split_mode: WorkspaceSplitMode,

    pub theme_overrides: ThemeOverrides,
    pub keybindings: KeybindingMap,
    pub editor: EditorSettings,
    pub grid: GridSettings,
    pub behavior: AppBehaviorSettings,
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
            ollama: OllamaSettings::default(),
            openai: OpenAiCompatSettings::for_provider(OpenAiCompatProvider::OpenAi),
            groq: OpenAiCompatSettings::for_provider(OpenAiCompatProvider::Groq),
            openrouter: OpenAiCompatSettings::for_provider(OpenAiCompatProvider::OpenRouter),
            xai: OpenAiCompatSettings::for_provider(OpenAiCompatProvider::XAi),
            mistral: OpenAiCompatSettings::for_provider(OpenAiCompatProvider::Mistral),
            ai_catalog: crate::AiCatalogSettings::default(),
            lm_keys: BTreeMap::new(),
            ai_response_language: "English".to_string(),
            ai_auto_apply_completions: false,
            sql_completion: SqlCompletionSettings::default(),
            explorer: ExplorerViewSettings::default(),
            show_bottom_panel: true,
            bottom_panel_height: 120.0,
            split_mode: WorkspaceSplitMode::default(),
            theme_overrides: ThemeOverrides::default(),
            keybindings: KeybindingMap::new(),
            editor: EditorSettings::default(),
            grid: GridSettings::default(),
            behavior: AppBehaviorSettings::default(),
        }
    }
}

impl AppUiSettings {
    pub fn sql_ghost_ready(&self) -> bool {
        self.ai_features_enabled
            && !self.sql_completion.provider.trim().is_empty()
            && !self.sql_completion.model.trim().is_empty()
            && crate::is_native_http_ready(
                &self.sql_completion.provider,
                &self.lm_api_key(&self.sql_completion.provider),
                &self.ai_catalog,
            )
    }

    pub fn openai_compat(&self, provider: OpenAiCompatProvider) -> &OpenAiCompatSettings {
        match provider {
            OpenAiCompatProvider::OpenAi => &self.openai,
            OpenAiCompatProvider::Groq => &self.groq,
            OpenAiCompatProvider::OpenRouter => &self.openrouter,
            OpenAiCompatProvider::XAi => &self.xai,
            OpenAiCompatProvider::Mistral => &self.mistral,
        }
    }

    pub fn openai_compat_mut(
        &mut self,
        provider: OpenAiCompatProvider,
    ) -> &mut OpenAiCompatSettings {
        match provider {
            OpenAiCompatProvider::OpenAi => &mut self.openai,
            OpenAiCompatProvider::Groq => &mut self.groq,
            OpenAiCompatProvider::OpenRouter => &mut self.openrouter,
            OpenAiCompatProvider::XAi => &mut self.xai,
            OpenAiCompatProvider::Mistral => &mut self.mistral,
        }
    }

    /// API key for send/connect/display.
    ///
    /// An explicit `lm_keys` entry is authoritative, including empty (the user
    /// cleared the key). Only fall back to a legacy vendor blob when the
    /// provider is absent from `lm_keys`.
    pub fn lm_api_key(&self, provider: &str) -> String {
        if let Some(key) = self.lm_keys.get(provider) {
            return key.clone();
        }
        match provider {
            "deepseek" => self.deepseek.api_key.clone(),
            "openai" => self.openai.api_key.clone(),
            "groq" => self.groq.api_key.clone(),
            "openrouter" => self.openrouter.api_key.clone(),
            "xai" => self.xai.api_key.clone(),
            "mistral" => self.mistral.api_key.clone(),
            "ollama" => self.ollama.api_key.clone(),
            _ => String::new(),
        }
    }

    /// Write an LM key to `lm_keys` and the matching legacy vendor blob (if any).
    /// Empty values are stored so a clear does not snap back to a hydrated blob.
    pub fn set_lm_api_key(&mut self, provider: &str, api_key: String) {
        self.lm_keys.insert(provider.to_string(), api_key.clone());
        match provider {
            "deepseek" => self.deepseek.api_key = api_key,
            "openai" => self.openai.api_key = api_key,
            "groq" => self.groq.api_key = api_key,
            "openrouter" => self.openrouter.api_key = api_key,
            "xai" => self.xai.api_key = api_key,
            "mistral" => self.mistral.api_key = api_key,
            "ollama" => self.ollama.api_key = api_key,
            _ => {}
        }
    }

    /// Remove a custom native provider and queue an empty key so save deletes
    /// the keyring/fallback secret (`save_lm_api_key(service, "")`).
    pub fn delete_custom_native_provider(&mut self, id: &str) {
        crate::delete_custom_provider(&mut self.ai_catalog, id);
        self.set_lm_api_key(id, String::new());
    }

    /// Copy legacy per-vendor AI settings into `ai_catalog` when the catalog
    /// has no active model yet. Legacy blobs stay in memory for key migration
    /// but are not written back to JSON.
    pub fn migrate_legacy_ai_fields(&mut self) {
        if self.ai_catalog.active.is_some() {
            return;
        }

        let legacy = [
            (
                "deepseek",
                self.deepseek.enabled,
                self.deepseek.api_key.clone(),
                self.deepseek.base_url.clone(),
                self.deepseek.model.clone(),
                self.deepseek.thinking_enabled,
                self.deepseek.reasoning_effort.clone(),
            ),
            (
                "openai",
                self.openai.enabled,
                self.openai.api_key.clone(),
                self.openai.base_url.clone(),
                self.openai.model.clone(),
                false,
                "medium".to_string(),
            ),
            (
                "groq",
                self.groq.enabled,
                self.groq.api_key.clone(),
                self.groq.base_url.clone(),
                self.groq.model.clone(),
                false,
                "medium".to_string(),
            ),
            (
                "openrouter",
                self.openrouter.enabled,
                self.openrouter.api_key.clone(),
                self.openrouter.base_url.clone(),
                self.openrouter.model.clone(),
                false,
                "medium".to_string(),
            ),
            (
                "xai",
                self.xai.enabled,
                self.xai.api_key.clone(),
                self.xai.base_url.clone(),
                self.xai.model.clone(),
                false,
                "medium".to_string(),
            ),
            (
                "mistral",
                self.mistral.enabled,
                self.mistral.api_key.clone(),
                self.mistral.base_url.clone(),
                self.mistral.model.clone(),
                false,
                "medium".to_string(),
            ),
            (
                "ollama",
                self.ollama.enabled,
                self.ollama.api_key.clone(),
                self.ollama.base_url.clone(),
                self.ollama.model.clone(),
                false,
                "medium".to_string(),
            ),
        ];

        let mut active = None;
        for (slug, enabled, api_key, base_url, model, thinking_enabled, reasoning_effort) in legacy
        {
            let has_key = !api_key.trim().is_empty();
            let model_trim = model.trim();
            let default_model = crate::builtin_providers()
                .iter()
                .find(|p| p.slug == slug)
                .and_then(|p| p.builtin_models.first().map(|(id, _)| *id))
                .unwrap_or("");
            // Ollama's stock default is empty; others use the first builtin id.
            let stock_model = if slug == "ollama" {
                ""
            } else if slug == "deepseek" {
                "deepseek-chat"
            } else {
                default_model
            };
            let model_customized = !model_trim.is_empty() && model_trim != stock_model;
            if !(enabled || has_key || model_customized) {
                continue;
            }

            let builtin_ids: Vec<&str> = crate::builtin_providers()
                .iter()
                .find(|p| p.slug == slug)
                .map(|p| p.builtin_models.iter().map(|(id, _)| *id).collect())
                .unwrap_or_default();

            let mut extra_models = Vec::new();
            if !model_trim.is_empty() && !builtin_ids.contains(&model_trim) {
                extra_models.push(crate::AiModelEntry {
                    id: model_trim.to_string(),
                    label: String::new(),
                });
            }

            self.ai_catalog.overrides.insert(
                slug.to_string(),
                crate::AiProviderOverride {
                    enabled,
                    base_url,
                    extra_models,
                    hidden_builtin_ids: Vec::new(),
                    thinking_enabled,
                    reasoning_effort,
                },
            );

            if active.is_none() && enabled && !model_trim.is_empty() {
                active = Some(crate::ActiveModel {
                    provider: slug.to_string(),
                    model: model_trim.to_string(),
                });
            }
        }

        self.ai_catalog.active = active;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AppThemePreference,
        AppUiSettings,
        ExplorerViewSettings,
        NullDisplay,
        OllamaSettings,
        UiDensity,
        WorkspaceSplitMode,
        format_null_display,
    };
    use crate::ThemeOverrides;

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
    fn openai_compat_catalog_has_stable_ids_and_models() {
        for provider in super::OpenAiCompatProvider::ALL {
            assert!(!provider.id().is_empty());
            assert!(!provider.models().is_empty());
            assert!(provider.models().contains(&provider.default_model()));
            let settings = super::OpenAiCompatSettings::for_provider(provider);
            assert_eq!(settings.base_url, provider.default_base_url());
            assert_eq!(settings.model, provider.default_model());
        }
    }

    #[test]
    fn fresh_default_density_is_compact() {
        assert_eq!(AppUiSettings::default().density, UiDensity::Compact);
        assert_eq!(UiDensity::default(), UiDensity::Compact);
    }

    #[test]
    fn fresh_default_shows_bottom_panel_at_baseline_height() {
        let defaults = AppUiSettings::default();
        assert!(defaults.show_bottom_panel);
        assert!(defaults.bottom_panel_height > 0.0);
    }

    #[test]
    fn legacy_settings_missing_bottom_panel_default_to_visible_120px() {
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

        assert!(settings.show_bottom_panel);
        assert_eq!(settings.bottom_panel_height, 120.0);
    }

    #[test]
    fn persisted_bottom_panel_state_is_preserved() {
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
                "show_bottom_panel":false,
                "bottom_panel_height":312.5
            }"#,
        )
        .expect("settings fixture should deserialize");

        assert!(!settings.show_bottom_panel);
        assert!((settings.bottom_panel_height - 312.5).abs() < f64::EPSILON);
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
    fn fresh_default_ai_auto_apply_completions_is_disabled() {
        assert!(!AppUiSettings::default().ai_auto_apply_completions);
    }

    #[test]
    fn legacy_settings_missing_ai_auto_apply_completions_defaults_to_false() {
        let settings: AppUiSettings =
            serde_json::from_str(r#"{"theme":"Dark"}"#).expect("legacy JSON");
        assert!(!settings.ai_auto_apply_completions);
    }

    #[test]
    fn fresh_default_sql_completion_is_empty() {
        let settings = AppUiSettings::default();
        assert!(settings.sql_completion.provider.is_empty());
        assert!(settings.sql_completion.model.is_empty());
        assert!(!settings.sql_ghost_ready());
    }

    #[test]
    fn sql_ghost_ready_rejects_acp_slug() {
        let settings = AppUiSettings {
            ai_features_enabled: true,
            sql_completion: super::SqlCompletionSettings {
                provider: "acp:opencode".into(),
                model: "x".into(),
            },
            ..AppUiSettings::default()
        };
        assert!(!settings.sql_ghost_ready());
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
    fn ollama_keyring_service_name_is_stable() {
        assert_eq!(OllamaSettings::keyring_service(), "shovel.ollama");
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
            // Default for ai_auto_apply_completions is `false`; flip it so the
            // round-trip explicitly exercises the field.
            ai_auto_apply_completions: true,
            // Non-default sql_completion so round-trip exercises the new field.
            sql_completion: super::SqlCompletionSettings {
                provider: "openai".into(),
                model: "gpt-5.6-luna".into(),
            },
            // Explorer view settings — flip from defaults so every field is
            // explicitly exercised by the JSON round-trip.
            explorer: ExplorerViewSettings {
                show_schemas: false,
                show_tables: false,
                show_views: false,
                show_columns: true,
                show_system_objects: true,
                show_row_counts: false,
                sort_alphabetical: true,
            },
            // Bottom dock starts visible by default; flip to false so the
            // round-trip also exercises the persisted `show_bottom_panel`.
            show_bottom_panel: false,
            bottom_panel_height: 320.5,
            // Split mode defaults to Off; flip to Horizontal so the
            // round-trip exercises the persisted `split_mode`.
            split_mode: WorkspaceSplitMode::Horizontal,
            ..AppUiSettings::default()
        };
        settings.codestral.enabled = true;
        settings.codestral.model = "codestral-22b".to_string();
        // Legacy vendor blobs are skip_serializing; catalog is what persists.
        settings.ai_catalog.active = Some(crate::ActiveModel {
            provider: "deepseek".into(),
            model: "deepseek-v4-flash".into(),
        });
        settings.ai_catalog.overrides.insert(
            "deepseek".into(),
            crate::AiProviderOverride {
                enabled: true,
                base_url: "https://api.deepseek.com".into(),
                extra_models: Vec::new(),
                hidden_builtin_ids: Vec::new(),
                thinking_enabled: true,
                reasoning_effort: "high".into(),
            },
        );
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
            (
                "show_bottom_panel",
                Box::new(|s| s.show_bottom_panel = true),
            ),
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
                reloaded.ai_catalog, settings.ai_catalog,
                "{field_name} toggle dropped ai_catalog"
            );
            assert_eq!(
                reloaded.tool_panel_layout, settings.tool_panel_layout,
                "{field_name} toggle dropped tool_panel_layout"
            );
            assert_eq!(
                reloaded.ai_auto_apply_completions, settings.ai_auto_apply_completions,
                "{field_name} toggle dropped ai_auto_apply_completions"
            );
            assert_eq!(
                reloaded.sql_completion, settings.sql_completion,
                "{field_name} toggle dropped sql_completion"
            );
            assert_eq!(
                reloaded.explorer, settings.explorer,
                "{field_name} toggle dropped explorer view settings"
            );
            assert_eq!(
                reloaded.show_bottom_panel, settings.show_bottom_panel,
                "{field_name} toggle dropped show_bottom_panel"
            );
            assert_eq!(
                reloaded.bottom_panel_height, settings.bottom_panel_height,
                "{field_name} toggle dropped bottom_panel_height"
            );
            assert_eq!(
                reloaded.split_mode, settings.split_mode,
                "{field_name} toggle dropped split_mode"
            );
        }
    }

    #[test]
    fn legacy_settings_missing_explorer_default_to_dbeaver_baseline() {
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

        let explorer = settings.explorer;
        assert!(explorer.show_schemas);
        assert!(explorer.show_tables);
        assert!(explorer.show_views);
        assert!(!explorer.show_columns);
        assert!(!explorer.show_system_objects);
        assert!(explorer.show_row_counts);
        assert!(!explorer.sort_alphabetical);
    }

    #[test]
    fn explorer_view_settings_round_trip_via_json() {
        let settings = ExplorerViewSettings {
            show_schemas: false,
            show_tables: false,
            show_views: true,
            show_columns: true,
            show_system_objects: true,
            show_row_counts: false,
            sort_alphabetical: true,
        };

        let serialized = serde_json::to_string(&settings).expect("settings should serialize");
        let reloaded: ExplorerViewSettings =
            serde_json::from_str(&serialized).expect("settings should deserialize");
        assert_eq!(reloaded, settings);
    }

    #[test]
    fn fresh_default_split_mode_is_off() {
        let defaults = AppUiSettings::default();
        assert_eq!(defaults.split_mode, WorkspaceSplitMode::Off);
        assert_eq!(WorkspaceSplitMode::default(), WorkspaceSplitMode::Off);
    }

    #[test]
    fn legacy_settings_missing_split_mode_default_to_off() {
        // Settings written before the split-mode feature shipped should
        // still deserialize — the missing field must default to `Off` so
        // existing users keep the single-pane stack on first launch.
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

        assert_eq!(settings.split_mode, WorkspaceSplitMode::Off);
    }

    #[test]
    fn split_mode_round_trips_via_json() {
        for variant in WorkspaceSplitMode::ALL {
            let serialized = serde_json::to_string(&variant).expect("split mode should serialize");
            let reloaded: WorkspaceSplitMode =
                serde_json::from_str(&serialized).expect("split mode should deserialize");
            assert_eq!(reloaded, variant);
        }
    }

    #[test]
    fn split_mode_css_class_matches_each_variant() {
        assert_eq!(WorkspaceSplitMode::Off.css_class(), "split-mode-off");
        assert_eq!(
            WorkspaceSplitMode::Horizontal.css_class(),
            "split-mode-horizontal"
        );
        assert_eq!(
            WorkspaceSplitMode::Vertical.css_class(),
            "split-mode-vertical"
        );
    }

    #[test]
    fn split_mode_next_cycles_through_all_variants() {
        assert_eq!(
            WorkspaceSplitMode::Off.next(),
            WorkspaceSplitMode::Horizontal
        );
        assert_eq!(
            WorkspaceSplitMode::Horizontal.next(),
            WorkspaceSplitMode::Vertical
        );
        assert_eq!(WorkspaceSplitMode::Vertical.next(), WorkspaceSplitMode::Off);
    }

    #[test]
    fn fresh_defaults_match_settings_spec() {
        let s = AppUiSettings::default();
        assert_eq!(s.editor.font_size, 13);
        assert_eq!(s.editor.tab_size, 2);
        assert!(!s.editor.auto_format_on_run);
        assert!(!s.editor.word_wrap);
        assert!(s.editor.show_line_numbers);
        assert_eq!(s.grid.row_height, 28);
        assert!(!s.grid.zebra);
        assert_eq!(s.grid.null_display, NullDisplay::Literal);
        assert!(!s.grid.wrap_cells);
        assert!(s.behavior.confirm_before_drop);
        assert!(s.behavior.confirm_before_truncate);
        assert!(s.keybindings.is_empty());
        assert_eq!(s.theme_overrides, ThemeOverrides::default());
    }

    #[test]
    fn legacy_json_without_new_fields_gets_spec_defaults() {
        let settings: AppUiSettings = serde_json::from_str(r#"{"theme":"Dark"}"#)
            .expect("legacy settings should deserialize");
        assert_eq!(settings.editor.font_size, 13);
        assert!(settings.editor.show_line_numbers);
        assert_eq!(settings.grid.row_height, 28);
        assert!(settings.behavior.confirm_before_drop);
        assert!(settings.keybindings.is_empty());
    }

    #[test]
    fn new_settings_fields_round_trip() {
        let mut settings = AppUiSettings::default();
        settings.editor.font_size = 16;
        settings.editor.word_wrap = true;
        settings.grid.zebra = true;
        settings.grid.null_display = NullDisplay::EmDash;
        settings.behavior.confirm_before_drop = false;
        settings
            .keybindings
            .insert("format_sql".into(), "Ctrl+Alt+F".into());
        settings.theme_overrides.primary = Some("#ff8800".into());
        let json = serde_json::to_string(&settings).unwrap();
        let back: AppUiSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.editor.font_size, 16);
        assert!(back.editor.word_wrap);
        assert!(back.grid.zebra);
        assert_eq!(back.grid.null_display, NullDisplay::EmDash);
        assert!(!back.behavior.confirm_before_drop);
        assert_eq!(
            back.keybindings.get("format_sql").map(String::as_str),
            Some("Ctrl+Alt+F")
        );
        assert_eq!(back.theme_overrides.primary.as_deref(), Some("#ff8800"));
    }

    #[test]
    fn format_null_display_modes() {
        assert_eq!(format_null_display("NULL", NullDisplay::Literal), "NULL");
        assert_eq!(format_null_display("NULL", NullDisplay::Empty), "");
        assert_eq!(format_null_display("null", NullDisplay::EmDash), "—");
        assert_eq!(format_null_display("hello", NullDisplay::EmDash), "hello");
        assert_eq!(format_null_display("", NullDisplay::Literal), "NULL");
    }

    #[test]
    fn migrate_legacy_deepseek_fills_catalog_active_and_override() {
        let json = r#"{
        "theme":"Dark",
        "deepseek":{
            "enabled":true,
            "base_url":"https://api.deepseek.com",
            "model":"deepseek-chat",
            "thinking_enabled":true,
            "reasoning_effort":"high"
        }
    }"#;
        let mut settings: AppUiSettings = serde_json::from_str(json).unwrap();
        settings.migrate_legacy_ai_fields();
        let active = settings.ai_catalog.active.as_ref().expect("active");
        assert_eq!(active.provider, "deepseek");
        assert_eq!(active.model, "deepseek-chat");
        let over = settings
            .ai_catalog
            .overrides
            .get("deepseek")
            .expect("override");
        assert!(over.enabled);
        assert_eq!(over.reasoning_effort, "high");
        assert!(over.thinking_enabled);
        let dumped = serde_json::to_value(&settings).unwrap();
        assert!(dumped.get("deepseek").is_none());
        assert!(dumped.get("ai_catalog").is_some());
    }

    #[test]
    fn catalog_secrets_are_not_in_json() {
        let mut settings = AppUiSettings::default();
        settings.deepseek.api_key = "sk-legacy".into();
        let dumped = serde_json::to_string(&settings).unwrap();
        assert!(!dumped.contains("sk-legacy"));
    }

    #[test]
    fn lm_keys_are_omitted_from_json() {
        let mut settings = AppUiSettings::default();
        settings
            .lm_keys
            .insert("deepseek".into(), "sk-in-memory".into());
        settings
            .lm_keys
            .insert("custom:abc".into(), "sk-custom".into());
        let dumped = serde_json::to_value(&settings).unwrap();
        let obj = dumped.as_object().expect("object");
        assert!(
            !obj.contains_key("lm_keys"),
            "lm_keys must not appear in JSON"
        );
        let text = serde_json::to_string(&settings).unwrap();
        assert!(!text.contains("sk-in-memory"));
        assert!(!text.contains("sk-custom"));
    }

    #[test]
    fn lm_api_key_prefers_lm_keys_over_empty_vendor() {
        let mut settings = AppUiSettings::default();
        settings
            .lm_keys
            .insert("openai".into(), "sk-from-lm".into());
        settings.openai.api_key.clear();
        assert_eq!(settings.lm_api_key("openai"), "sk-from-lm");
    }

    #[test]
    fn lm_api_key_explicit_empty_does_not_snap_back_to_vendor() {
        let mut settings = AppUiSettings::default();
        settings.openai.api_key = "sk-vendor".into();
        settings.lm_keys.insert("openai".into(), String::new());
        assert_eq!(settings.lm_api_key("openai"), "");

        settings.lm_keys.remove("openai");
        assert_eq!(settings.lm_api_key("openai"), "sk-vendor");
    }

    #[test]
    fn delete_custom_native_provider_queues_empty_secret() {
        let mut settings = AppUiSettings::default();
        settings
            .ai_catalog
            .custom_native
            .push(crate::CustomNativeProvider {
                id: "custom:1".into(),
                name: "Mine".into(),
                base_url: "http://localhost:8080".into(),
                models: Vec::new(),
            });
        settings.set_lm_api_key("custom:1", "sk-custom".into());
        settings.ai_catalog.active = Some(crate::ActiveModel {
            provider: "custom:1".into(),
            model: "m".into(),
        });
        settings.delete_custom_native_provider("custom:1");
        assert!(settings.ai_catalog.custom_native.is_empty());
        assert!(settings.ai_catalog.active.is_none());
        assert_eq!(
            settings.lm_keys.get("custom:1").map(String::as_str),
            Some("")
        );
    }

    #[test]
    fn set_lm_api_key_mirrors_legacy_vendor_including_empty() {
        let mut settings = AppUiSettings::default();
        settings.openai.api_key = "sk-old".into();
        settings.set_lm_api_key("openai", "sk-new".into());
        assert_eq!(
            settings.lm_keys.get("openai").map(String::as_str),
            Some("sk-new")
        );
        assert_eq!(settings.openai.api_key, "sk-new");
        assert_eq!(settings.lm_api_key("openai"), "sk-new");

        settings.set_lm_api_key("openai", String::new());
        assert_eq!(settings.lm_keys.get("openai").map(String::as_str), Some(""));
        assert_eq!(settings.openai.api_key, "");
        assert_eq!(settings.lm_api_key("openai"), "");
    }
}
