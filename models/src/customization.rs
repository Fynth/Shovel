use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Structured overrides for the app's CSS design tokens. Every field is
/// optional; `None` leaves the default token untouched. When applied, these
/// are rendered into `:root { --color-*: ...; --font-*: ...; }` CSS variables
/// that the existing components already consume, so a theme override restyles
/// the whole app without touching individual components.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeOverrides {
    // ── Colors ─────────────────────────────────────────────────────
    pub primary: Option<String>,
    pub primary_hover: Option<String>,
    pub primary_active: Option<String>,
    pub danger: Option<String>,
    pub success: Option<String>,
    pub warning: Option<String>,
    pub info: Option<String>,
    pub text: Option<String>,
    pub text_muted: Option<String>,
    pub text_dim: Option<String>,
    pub border: Option<String>,
    pub border_strong: Option<String>,
    pub background: Option<String>,
    pub panel: Option<String>,
    pub panel_2: Option<String>,
    pub panel_3: Option<String>,

    // ── Typography ─────────────────────────────────────────────────
    pub font_family: Option<String>,
    pub font_family_mono: Option<String>,
    pub font_size: Option<u32>,
    pub font_size_small: Option<u32>,
    pub font_size_large: Option<u32>,

    // ── Geometry ───────────────────────────────────────────────────
    pub radius_small: Option<u32>,
    pub radius_medium: Option<u32>,
    pub radius_large: Option<u32>,
    pub spacing: Option<u32>,
}

impl ThemeOverrides {
    /// Render these overrides as a CSS `:root { ... }` block. Only the
    /// tokens that are set are emitted; the rest fall back to the defaults
    /// already defined in the stylesheet.
    pub fn to_css(&self) -> String {
        let mut rules = Vec::new();
        let mut push = |name: &str, value: &str| rules.push(format!("  --{name}: {value};"));

        if let Some(v) = &self.primary {
            push("color-primary", v);
        }
        if let Some(v) = &self.primary_hover {
            push("color-primary-hover", v);
        }
        if let Some(v) = &self.primary_active {
            push("color-primary-active", v);
        }
        if let Some(v) = &self.danger {
            push("color-danger", v);
        }
        if let Some(v) = &self.success {
            push("color-success", v);
        }
        if let Some(v) = &self.warning {
            push("color-warning", v);
        }
        if let Some(v) = &self.info {
            push("color-info", v);
        }
        if let Some(v) = &self.text {
            push("color-text", v);
        }
        if let Some(v) = &self.text_muted {
            push("color-text-muted", v);
        }
        if let Some(v) = &self.text_dim {
            push("color-text-dim", v);
        }
        if let Some(v) = &self.border {
            push("color-border", v);
        }
        if let Some(v) = &self.border_strong {
            push("color-border-strong", v);
        }
        if let Some(v) = &self.background {
            push("color-background", v);
        }
        if let Some(v) = &self.panel {
            push("color-panel", v);
        }
        if let Some(v) = &self.panel_2 {
            push("color-panel-2", v);
        }
        if let Some(v) = &self.panel_3 {
            push("color-panel-3", v);
        }
        if let Some(v) = &self.font_family {
            push("font-family-sans", v);
        }
        if let Some(v) = &self.font_family_mono {
            push("font-family-mono", v);
        }
        if let Some(v) = self.font_size {
            push("font-size-md", &format!("{v}px"));
        }
        if let Some(v) = self.font_size_small {
            push("font-size-sm", &format!("{v}px"));
        }
        if let Some(v) = self.font_size_large {
            push("font-size-lg", &format!("{v}px"));
        }
        if let Some(v) = self.radius_small {
            push("radius-sm", &format!("{v}px"));
        }
        if let Some(v) = self.radius_medium {
            push("radius-md", &format!("{v}px"));
        }
        if let Some(v) = self.radius_large {
            push("radius-lg", &format!("{v}px"));
        }
        if let Some(v) = self.spacing {
            push("spacing-base", &format!("{v}px"));
        }

        if rules.is_empty() {
            String::new()
        } else {
            format!(":root {{\n{}\n}}", rules.join("\n"))
        }
    }
}

/// A user-defined keyboard shortcut, e.g. `"Ctrl+Shift+F"`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Keybinding {
    pub action: String,
    pub combo: String,
}

/// Map of action id → key combo, parsed from `[keybindings]` in config.toml.
pub type KeybindingMap = HashMap<String, String>;

/// Editor behavior overrides applied at runtime.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorBehavior {
    pub font_size: Option<u32>,
    pub tab_size: Option<u32>,
    pub auto_format_on_run: Option<bool>,
    pub auto_complete: Option<bool>,
    pub word_wrap: Option<bool>,
    pub show_line_numbers: Option<bool>,
}

/// Panel behavior overrides applied at runtime.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelBehavior {
    pub auto_open_explorer: Option<bool>,
    pub auto_open_agent: Option<bool>,
    pub default_sidebar_width: Option<u32>,
    pub default_bottom_panel_height: Option<u32>,
}

/// App-level behavior overrides applied at runtime.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppBehavior {
    pub auto_connect_on_launch: Option<bool>,
    pub auto_save_queries: Option<bool>,
    pub confirm_before_drop: Option<bool>,
    pub confirm_before_truncate: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_theme_renders_no_css() {
        assert_eq!(ThemeOverrides::default().to_css(), "");
    }

    #[test]
    fn theme_renders_set_tokens_only() {
        let theme = ThemeOverrides {
            primary: Some("#ff0000".to_string()),
            font_size: Some(14),
            radius_medium: Some(8),
            ..ThemeOverrides::default()
        };
        let css = theme.to_css();
        assert!(css.contains("--color-primary: #ff0000;"));
        assert!(css.contains("--font-size-md: 14px;"));
        assert!(css.contains("--radius-md: 8px;"));
        assert!(!css.contains("--color-danger"));
    }

    #[test]
    fn keybinding_map_roundtrips() {
        let mut map = KeybindingMap::new();
        map.insert("format_sql".to_string(), "Ctrl+Shift+F".to_string());
        let json = serde_json::to_string(&map).expect("serialize");
        let back: KeybindingMap = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back["format_sql"], "Ctrl+Shift+F");
    }
}
