use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Normalize a CSS hex color to `#rrggbb` (lowercase). Accepts `#RGB` and
/// `#RRGGBB`; returns `None` for anything else.
pub fn parse_hex_color(value: &str) -> Option<String> {
    let value = value.trim();
    let hex = value.strip_prefix('#')?;
    let expanded = match hex.len() {
        3 => {
            let mut out = String::with_capacity(6);
            for ch in hex.chars() {
                out.push(ch);
                out.push(ch);
            }
            out
        }
        6 => hex.to_string(),
        _ => return None,
    };
    if !expanded.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("#{}", expanded.to_ascii_lowercase()))
}

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
            if self.primary_hover.is_none() {
                push(
                    "color-primary-hover",
                    &format!("color-mix(in srgb, {v} 72%, white)"),
                );
            }
            if self.primary_active.is_none() {
                push(
                    "color-primary-active",
                    &format!("color-mix(in srgb, {v} 80%, black)"),
                );
            }
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
            push("font-sans", v);
        }
        if let Some(v) = &self.font_family_mono {
            push("font-mono", v);
        }
        if let Some(v) = self.font_size {
            push("ui-font-size", &format!("{v}px"));
        }
        if let Some(v) = self.font_size_small {
            push("ui-font-size-sm", &format!("{v}px"));
        }
        if let Some(v) = self.font_size_large {
            push("ui-font-size-lg", &format!("{v}px"));
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
        assert!(css.contains("--ui-font-size: 14px;"));
        assert!(css.contains("--radius-md: 8px;"));
        assert!(!css.contains("--color-danger"));
    }

    #[test]
    fn theme_emits_live_css_variable_names() {
        let theme = ThemeOverrides {
            primary: Some("#ff0000".to_string()),
            font_family: Some("IBM Plex Sans, sans-serif".to_string()),
            font_size: Some(14),
            radius_small: Some(4),
            ..ThemeOverrides::default()
        };
        let css = theme.to_css();
        assert!(css.contains("--color-primary: #ff0000;"));
        assert!(css.contains("--color-primary-hover:"));
        assert!(css.contains("--color-primary-active:"));
        assert!(css.contains("--font-sans: IBM Plex Sans, sans-serif;"));
        assert!(css.contains("--ui-font-size: 14px;"));
        assert!(css.contains("--radius-sm: 4px;"));
        assert!(!css.contains("--font-family-sans"));
        assert!(!css.contains("--font-size-md"));
    }

    #[test]
    fn parse_hex_color_accepts_short_and_long() {
        assert_eq!(parse_hex_color("#f00").as_deref(), Some("#ff0000"));
        assert_eq!(parse_hex_color("#FF8800").as_deref(), Some("#ff8800"));
        assert_eq!(parse_hex_color("not-a-color"), None);
        assert_eq!(parse_hex_color("#gg0000"), None);
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
