mod clickhouse;
mod mysql;
mod postgres;
mod sqlite;
mod ssh_tunnel;

pub use clickhouse::ClickHouseForm;
pub use mysql::MySqlForm;
pub use postgres::PostgresForm;
pub use sqlite::SqliteForm;
pub use ssh_tunnel::SshTunnelFields;

pub(super) fn connection_status_class(status: &str) -> &'static str {
    let normalized = status.trim();

    if normalized.starts_with("Error:") {
        "connect-screen__status connect-screen__status--error"
    } else if normalized.eq_ignore_ascii_case("connecting...")
        || normalized.eq_ignore_ascii_case("testing...")
    {
        "connect-screen__status connect-screen__status--busy"
    } else if normalized.starts_with("Connected") {
        "connect-screen__status connect-screen__status--success"
    } else {
        "connect-screen__status connect-screen__status--hint"
    }
}

pub(super) fn format_connection_error(err: impl std::fmt::Display) -> String {
    format!("Error: {err}")
}

/// Проверяет обязательные поля формы подключения. Возвращает ошибку с
/// названием первого пустого поля, иначе `Ok(())`. Используется и кнопкой
/// «Connect», и кнопкой «Test», чтобы не отправлять заведомо неполный запрос.
pub(super) fn validate_required_fields(fields: &[(&str, &str)]) -> Result<(), String> {
    for (label, value) in fields {
        if value.trim().is_empty() {
            return Err(format!("{label} is required"));
        }
    }
    Ok(())
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn should_render_status(status: &str) -> bool {
    !status.trim().is_empty()
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn status_text_for_display(status: &str) -> &str {
    status.strip_prefix("Status: ").unwrap_or(status)
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn contains_debug_formatting(text: &str) -> bool {
    text.contains(":?") || text.contains("ErrorKind")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_class_error() {
        assert_eq!(
            connection_status_class("Error: connection failed"),
            "connect-screen__status connect-screen__status--error"
        );
        assert_eq!(
            connection_status_class("Error: timeout"),
            "connect-screen__status connect-screen__status--error"
        );
    }

    #[test]
    fn test_status_class_connecting() {
        assert_eq!(
            connection_status_class("Connecting..."),
            "connect-screen__status connect-screen__status--busy"
        );
        assert_eq!(
            connection_status_class("connecting..."),
            "connect-screen__status connect-screen__status--busy"
        );
    }

    #[test]
    fn test_status_class_testing() {
        assert_eq!(
            connection_status_class("Testing..."),
            "connect-screen__status connect-screen__status--busy"
        );
        assert_eq!(
            connection_status_class("testing..."),
            "connect-screen__status connect-screen__status--busy"
        );
    }

    #[test]
    fn test_validate_required_fields_ok() {
        assert!(validate_required_fields(&[("Host", "localhost"), ("User", "postgres")]).is_ok());
    }

    #[test]
    fn test_validate_required_fields_reports_first_empty() {
        let err = validate_required_fields(&[("Host", "  "), ("User", "postgres")]).unwrap_err();
        assert_eq!(err, "Host is required");
    }

    #[test]
    fn test_validate_required_fields_empty_slice_ok() {
        assert!(validate_required_fields(&[]).is_ok());
    }

    #[test]
    fn test_status_class_connected() {
        assert_eq!(
            connection_status_class("Connected"),
            "connect-screen__status connect-screen__status--success"
        );
        assert_eq!(
            connection_status_class("Connected, but failed to save connection: disk full"),
            "connect-screen__status connect-screen__status--success"
        );
    }

    #[test]
    fn test_status_class_fallback() {
        assert_eq!(
            connection_status_class("Idle"),
            "connect-screen__status connect-screen__status--hint"
        );
        assert_eq!(
            connection_status_class(""),
            "connect-screen__status connect-screen__status--hint"
        );
        assert_eq!(
            connection_status_class("Config is empty"),
            "connect-screen__status connect-screen__status--hint"
        );
    }

    #[test]
    fn test_format_connection_error() {
        let err = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "connection refused");
        let formatted = format_connection_error(err);
        assert!(formatted.starts_with("Error:"));
        assert!(!formatted.contains("ErrorKind"));
    }

    #[test]
    fn test_format_connection_error_simple() {
        let formatted = format_connection_error("timeout");
        assert_eq!(formatted, "Error: timeout");
    }

    #[test]
    fn test_should_render_status() {
        assert!(should_render_status("Connected"));
        assert!(should_render_status("Error: failed"));
        assert!(should_render_status("  Connecting...  "));
        assert!(!should_render_status(""));
        assert!(!should_render_status("   "));
    }

    #[test]
    fn test_status_text_removes_status_prefix() {
        assert_eq!(status_text_for_display("Status: Connected"), "Connected");
        assert_eq!(
            status_text_for_display("Status: Error: failed"),
            "Error: failed"
        );
    }

    #[test]
    fn test_status_text_preserves_text_without_prefix() {
        assert_eq!(status_text_for_display("Connected"), "Connected");
        assert_eq!(status_text_for_display("Error: failed"), "Error: failed");
    }

    #[test]
    fn test_error_format_has_no_debug_syntax() {
        let formatted = format_connection_error("test error");
        assert!(!contains_debug_formatting(&formatted));
    }

    #[test]
    fn test_detects_debug_formatting() {
        assert!(contains_debug_formatting(
            "Error: ErrorKind(ConnectionRefused)"
        ));
        assert!(contains_debug_formatting("error: {err:?}"));
        assert!(contains_debug_formatting("Error: Some(ErrorKind::Timeout)"));
        assert!(!contains_debug_formatting("Error: connection refused"));
        assert!(!contains_debug_formatting("Error: ConnectionRefused"));
    }
}
