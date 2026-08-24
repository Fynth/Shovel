use crate::{
    app_state::{APP_LAST_QUERY, APP_STATE, LastQuerySummary},
    screens::workspace::helpers::format_duration,
};
use dioxus::prelude::*;
use models::DatabaseKind;

#[cfg_attr(not(test), allow(dead_code))]
pub fn status_bar_session_label(session_name: Option<&str>) -> String {
    match session_name {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => "No connection".to_string(),
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn status_bar_connection_label(name: Option<&str>, kind: Option<DatabaseKind>) -> String {
    match (name, kind) {
        (Some(name), Some(kind)) if !name.is_empty() => format!("{name} · {}", kind.display_name()),
        _ => "No connection".to_string(),
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn status_bar_session_count(count: usize) -> String {
    format!("Sessions {count}")
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn status_bar_last_query(summary: Option<&LastQuerySummary>) -> Option<String> {
    let summary = summary?;
    let label = match summary.duration_ms {
        Some(ms) => format!("Last: {} · {}", summary.label, format_duration(ms)),
        None => format!("Last: {}", summary.label),
    };
    Some(label)
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn is_allowed_status_bar_item(text: &str) -> bool {
    let text = text.trim();

    if text.contains("Rust + Dioxus") || text.starts_with("Theme:") {
        return false;
    }

    if text.starts_with("Active:") {
        return false;
    }

    true
}

#[component]
pub fn StatusBar() -> Element {
    let (connection_label, session_count, last_query) = {
        let app_state = APP_STATE.read();
        let label = status_bar_connection_label(
            app_state
                .active_session()
                .map(|session| session.name.as_str()),
            app_state.active_session().map(|session| session.kind),
        );
        (label, app_state.sessions.len(), APP_LAST_QUERY())
    };
    let last_query_text = status_bar_last_query(last_query.as_ref());
    let last_query_class = match last_query.as_ref().map(|summary| summary.failed) {
        Some(true) => "statusbar__item statusbar__item--error",
        _ => "statusbar__item",
    };

    rsx! {
        footer {
            class: "statusbar",
            span { class: "statusbar__item", "{connection_label}" }
            span { class: "statusbar__item", "Sessions {session_count}" }
            if let Some(text) = last_query_text {
                span { class: "{last_query_class}", "{text}" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_label_shows_name_without_prefix() {
        assert_eq!(status_bar_session_label(Some("My Database")), "My Database");
    }

    #[test]
    fn session_label_falls_back_to_no_connection() {
        assert_eq!(status_bar_session_label(None), "No connection");
        assert_eq!(status_bar_session_label(Some("")), "No connection");
    }

    #[test]
    fn connection_label_joins_name_and_kind() {
        assert_eq!(
            status_bar_connection_label(Some("MyDB"), Some(DatabaseKind::Postgres)),
            "MyDB · PostgreSQL"
        );
        assert_eq!(
            status_bar_connection_label(Some("shop"), Some(DatabaseKind::MySql)),
            "shop · MySQL"
        );
    }

    #[test]
    fn connection_label_falls_back_without_session() {
        assert_eq!(status_bar_connection_label(None, None), "No connection");
        assert_eq!(
            status_bar_connection_label(Some(""), Some(DatabaseKind::Sqlite)),
            "No connection"
        );
    }

    #[test]
    fn last_query_formats_label_with_duration() {
        let summary = LastQuerySummary {
            label: "Loaded rows 1-50".to_string(),
            duration_ms: Some(12),
            failed: false,
        };
        assert_eq!(
            status_bar_last_query(Some(&summary)).as_deref(),
            Some("Last: Loaded rows 1-50 · 12ms")
        );
    }

    #[test]
    fn last_query_formats_without_duration() {
        let summary = LastQuerySummary {
            label: "Rows affected: 3".to_string(),
            duration_ms: None,
            failed: false,
        };
        assert_eq!(
            status_bar_last_query(Some(&summary)).as_deref(),
            Some("Last: Rows affected: 3")
        );
    }

    #[test]
    fn last_query_none_when_no_summary() {
        assert_eq!(status_bar_last_query(None), None);
    }

    #[test]
    fn session_count_formats_compactly() {
        assert_eq!(status_bar_session_count(0), "Sessions 0");
        assert_eq!(status_bar_session_count(3), "Sessions 3");
    }

    #[test]
    fn rejects_rust_dioxus_metadata() {
        assert!(!is_allowed_status_bar_item("Rust + Dioxus 0.7"));
        assert!(!is_allowed_status_bar_item("Rust + Dioxus 0.7.1"));
    }

    #[test]
    fn rejects_theme_metadata() {
        assert!(!is_allowed_status_bar_item("Theme: dark"));
        assert!(!is_allowed_status_bar_item("Theme: light"));
    }

    #[test]
    fn rejects_active_prefix() {
        assert!(!is_allowed_status_bar_item("Active: My Database"));
    }

    #[test]
    fn allows_session_and_connection_labels() {
        assert!(is_allowed_status_bar_item("My Database"));
        assert!(is_allowed_status_bar_item("No connection"));
        assert!(is_allowed_status_bar_item("Sessions 3"));
    }
}
