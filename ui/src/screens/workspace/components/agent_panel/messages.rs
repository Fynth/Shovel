use ammonia::clean as sanitize_html;
use dioxus::prelude::*;
use pulldown_cmark::{Event, Options, Parser, html};

use models::{AcpMessageKind, AcpPanelState, AcpUiMessage, ChatArtifact};

use super::prompt::extract_sql_candidate;

thread_local! {
    // Keep clipboard ownership alive for Linux/X11/Wayland instead of dropping it right after copy.
    static PERSISTENT_CLIPBOARD: std::cell::RefCell<Option<arboard::Clipboard>> =
        const { std::cell::RefCell::new(None) };
}

pub(super) const AGENT_MESSAGE_BATCH: usize = 32;

pub(super) fn copy_text_to_clipboard(
    mut panel_state: Signal<AcpPanelState>,
    text: String,
    label: &str,
) {
    let text = text.trim().to_string();
    if text.is_empty() {
        panel_state.with_mut(|state| {
            state.status = format!("Nothing to copy as {label}.");
        });
        return;
    }

    let result = write_text_to_clipboard(&text);

    match result {
        Ok(()) => {
            panel_state.with_mut(|state| {
                state.status = format!("Copied {label} to clipboard.");
            });
        }
        Err(native_err) => {
            let Some(script) = clipboard_copy_script(&text) else {
                panel_state.with_mut(|state| {
                    state.status = format!("Clipboard error: {native_err}");
                });
                return;
            };

            let label = label.to_string();
            spawn(async move {
                let result = document::eval(&script).join::<bool>().await;
                panel_state.with_mut(|state| {
                    state.status = match result {
                        Ok(true) => format!("Copied {label} to clipboard."),
                        Ok(false) => format!("Clipboard error: {native_err}"),
                        Err(err) => format_clipboard_fallback_error(&native_err, err),
                    };
                });
            });
        }
    }
}

fn write_text_to_clipboard(text: &str) -> Result<(), String> {
    PERSISTENT_CLIPBOARD.with(|clipboard| {
        let mut clipboard = clipboard.borrow_mut();
        if clipboard.is_none() {
            *clipboard = Some(arboard::Clipboard::new().map_err(|err| err.to_string())?);
        }

        let clipboard = clipboard
            .as_mut()
            .ok_or_else(|| "Clipboard is unavailable.".to_string())?;

        clipboard
            .set_text(text.to_string())
            .map_err(|err| err.to_string())
    })
}

fn clipboard_copy_script(text: &str) -> Option<String> {
    let value = serde_json::to_string(text).ok()?;
    Some(format!(
        r#"
        (() => {{
            const value = {value};
            const copyWithExecCommand = () => {{
                const textarea = document.createElement("textarea");
                textarea.value = value;
                textarea.setAttribute("readonly", "");
                textarea.style.position = "fixed";
                textarea.style.opacity = "0";
                textarea.style.pointerEvents = "none";
                document.body.appendChild(textarea);
                textarea.focus();
                textarea.select();
                const copied = document.execCommand("copy");
                textarea.remove();
                return copied;
            }};

            if (navigator.clipboard && window.isSecureContext) {{
                return navigator.clipboard.writeText(value)
                    .then(() => true)
                    .catch(() => copyWithExecCommand());
            }}

            return copyWithExecCommand();
        }})()
        "#
    ))
}

#[derive(Clone, PartialEq, Eq)]
pub(super) enum MessageChunk {
    Text(String),
    Code {
        language: Option<String>,
        code: String,
    },
}

pub(super) fn parse_message_chunks(text: &str) -> Vec<MessageChunk> {
    let mut chunks = Vec::new();
    let mut cursor = 0;

    while let Some(start_offset) = text[cursor..].find("```") {
        let start = cursor + start_offset;
        let before = &text[cursor..start];
        if !before.trim().is_empty() {
            chunks.push(MessageChunk::Text(trim_message_chunk(before)));
        }

        let fence_meta_start = start + 3;
        let Some(meta_end_offset) = text[fence_meta_start..].find('\n') else {
            break;
        };
        let meta_end = fence_meta_start + meta_end_offset;
        let language = text[fence_meta_start..meta_end].trim().to_string();
        let code_start = meta_end + 1;
        let Some(code_end_offset) = text[code_start..].find("```") else {
            break;
        };
        let code_end = code_start + code_end_offset;
        let code = text[code_start..code_end].trim();

        if !code.is_empty() {
            chunks.push(MessageChunk::Code {
                language: (!language.is_empty()).then_some(language),
                code: code.to_string(),
            });
        }

        cursor = code_end + 3;
    }

    let remaining = &text[cursor..];
    if !remaining.trim().is_empty() {
        chunks.push(MessageChunk::Text(trim_message_chunk(remaining)));
    }

    if chunks.is_empty() && !text.trim().is_empty() {
        chunks.push(MessageChunk::Text(trim_message_chunk(text)));
    }

    chunks
}

fn trim_message_chunk(text: &str) -> String {
    text.trim_matches(|character| matches!(character, '\n' | '\r'))
        .to_string()
}

pub(super) fn render_message_markdown_html(text: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_SMART_PUNCTUATION);

    let parser = Parser::new_ext(text, options).map(|event| match event {
        Event::SoftBreak => Event::HardBreak,
        other => other,
    });

    let mut rendered_html = String::new();
    html::push_html(&mut rendered_html, parser);

    sanitize_html(&rendered_html)
}

pub(super) fn code_chunk_sql(language: Option<&str>, code: &str) -> Option<String> {
    if language.is_some_and(|value| value.eq_ignore_ascii_case("sql")) {
        return Some(code.trim().to_string());
    }

    extract_sql_candidate(code)
        .filter(|candidate| candidate.trim() == code.trim())
        .map(|candidate| candidate.trim().to_string())
}

pub(super) fn is_connection_notice(kind: &AcpMessageKind, text: &str) -> bool {
    matches!(kind, AcpMessageKind::System) && text.starts_with("Connected to ")
}

pub(super) fn is_internal_status_message(text: &str) -> bool {
    text.starts_with("Connected to ")
        || text.starts_with("Selected permission option:")
        || text == "Cancelled permission request."
        || text.starts_with("Blocked ACP tool request")
}

pub(super) fn is_visible_message(message: &AcpUiMessage) -> bool {
    match message.kind {
        AcpMessageKind::Tool => false,
        AcpMessageKind::System => {
            message.artifact.is_some()
                && !matches!(message.artifact, Some(ChatArtifact::QuerySummary { .. }))
                && !is_internal_status_message(&message.text)
        }
        _ => !is_connection_notice(&message.kind, &message.text),
    }
}

pub(super) fn compact_header_title(title: &str) -> String {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return "New chat".to_string();
    }

    trimmed
        .strip_prefix("New chat · ")
        .map(|_| "New chat".to_string())
        .unwrap_or_else(|| trimmed.to_string())
}

fn compact_connection_part(part: &str) -> String {
    let trimmed = part.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let last_segment = trimmed
        .rsplit(['/', '\\'])
        .next()
        .filter(|segment| !segment.is_empty())
        .unwrap_or(trimmed);

    if last_segment != trimmed {
        return last_segment.to_string();
    }

    if trimmed.chars().count() <= 48 {
        trimmed.to_string()
    } else {
        format!("{}...", trimmed.chars().take(45).collect::<String>())
    }
}

pub(super) fn compact_connection_label(label: &str) -> String {
    label
        .split('·')
        .map(compact_connection_part)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

pub(super) fn is_noisy_header_status(status: &str) -> bool {
    status.is_empty()
        || status == "Ready"
        || status == "Connect an agent to continue."
        || status == "ACP agent is disconnected."
        || status.starts_with("Connected to ")
        || status.starts_with("Executed agent SQL")
        || status.starts_with("Executed read-only SQL")
        || status.starts_with("Prompt finished:")
        || status.starts_with("Finished:")
}

pub(super) fn build_thread_meta(thread_connection_name: &str, state: &AcpPanelState) -> String {
    let connection = compact_connection_label(thread_connection_name);
    let status = state.status.trim();

    if state.busy || state.pending_permission.is_some() {
        if connection.is_empty() {
            status.to_string()
        } else {
            format!("{connection} · {status}")
        }
    } else if is_noisy_header_status(status) || status.is_empty() {
        connection
    } else if connection.is_empty() {
        status.to_string()
    } else if state.connected {
        format!("{connection} · {status}")
    } else {
        connection
    }
}

pub(super) fn should_render_message_text(message: &AcpUiMessage) -> bool {
    if matches!(message.kind, AcpMessageKind::Thought) {
        return false;
    }

    match &message.artifact {
        Some(ChatArtifact::QuerySummary { summary, .. }) => message.text.trim() != summary.trim(),
        _ => true,
    }
}

pub(super) fn artifact_title(artifact: &ChatArtifact) -> &'static str {
    match artifact {
        ChatArtifact::SqlDraft { .. } => "SQL Draft",
        ChatArtifact::QuerySummary { .. } => "SQL",
    }
}

pub fn format_clipboard_fallback_error(
    native_err: &str,
    fallback_err: impl std::fmt::Display,
) -> String {
    format!("Clipboard error: {native_err}; fallback failed: {fallback_err}")
}

pub fn acp_registry_loading_text() -> &'static str {
    "Loading agents..."
}

pub fn acp_registry_preparing_text(name: &str) -> String {
    format!("Preparing {name}...")
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn acp_registry_connecting_text(name: &str) -> String {
    format!("Connecting to {name}...")
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn is_verbose_acp_registry_loading_text(text: &str) -> bool {
    text == "Loading ACP registry..."
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn is_verbose_acp_registry_preparing_text(text: &str) -> bool {
    text.starts_with("Preparing ") && text.contains(" from the ACP registry")
}

#[cfg(test)]
mod tests {
    use super::{
        acp_registry_connecting_text, acp_registry_loading_text, artifact_title, build_thread_meta,
        compact_connection_label, compact_header_title, format_clipboard_fallback_error,
        is_verbose_acp_registry_loading_text, is_verbose_acp_registry_preparing_text,
        is_visible_message, render_message_markdown_html, should_render_message_text,
    };
    use models::{AcpMessageKind, AcpOllamaConfig, AcpPanelState, AcpUiMessage, ChatArtifact};

    #[test]
    fn hides_internal_system_messages_without_artifacts() {
        let message = AcpUiMessage {
            id: 1,
            kind: AcpMessageKind::System,
            text: "Connected to OpenCode".to_string(),
            created_at: 0,
            artifact: None,
        };
        assert!(!is_visible_message(&message));
    }

    #[test]
    fn keeps_system_messages_with_artifacts_visible() {
        let message = AcpUiMessage {
            id: 1,
            kind: AcpMessageKind::System,
            text: "Prepared SQL draft.".to_string(),
            created_at: 0,
            artifact: Some(ChatArtifact::SqlDraft {
                sql: "select 1".to_string(),
            }),
        };
        assert!(is_visible_message(&message));
    }

    #[test]
    fn hides_query_summary_system_messages() {
        let message = AcpUiMessage {
            id: 1,
            kind: AcpMessageKind::System,
            text: "Executed agent SQL in the active SQL tab.".to_string(),
            created_at: 0,
            artifact: Some(ChatArtifact::QuerySummary {
                sql: "select 1".to_string(),
                summary: "Executed agent SQL in the active SQL tab.".to_string(),
            }),
        };
        assert!(!is_visible_message(&message));
    }

    #[test]
    fn hides_duplicate_query_summary_message_text() {
        let message = AcpUiMessage {
            id: 1,
            kind: AcpMessageKind::System,
            text: "Automatically executed read-only SQL from ACP agent.".to_string(),
            created_at: 0,
            artifact: Some(ChatArtifact::QuerySummary {
                sql: "select 1".to_string(),
                summary: "Automatically executed read-only SQL from ACP agent.".to_string(),
            }),
        };

        assert!(!should_render_message_text(&message));
    }

    #[test]
    fn query_summary_artifact_title_is_compact() {
        assert_eq!(
            artifact_title(&ChatArtifact::QuerySummary {
                sql: "select 1".to_string(),
                summary: "Executed agent SQL in the active SQL tab.".to_string(),
            }),
            "SQL"
        );
    }

    #[test]
    fn compacts_new_chat_header_title() {
        assert_eq!(
            compact_header_title("New chat · SQLite · /home/rasul/Documents/data.sqlite"),
            "New chat"
        );
    }

    #[test]
    fn compacts_connection_paths_in_header_meta() {
        assert_eq!(
            compact_connection_label("SQLite · /home/rasul/Documents/data.sqlite"),
            "SQLite · data.sqlite"
        );
    }

    #[test]
    fn hides_idle_connected_status_from_header_meta() {
        let mut state = AcpPanelState::new(
            models::AcpLaunchRequest {
                command: String::new(),
                args: String::new(),
                cwd: ".".to_string(),
                env: Vec::new(),
            },
            AcpOllamaConfig {
                base_url: String::new(),
                model: String::new(),
                api_key: String::new(),
            },
        );
        state.connected = true;
        state.status = "Ready".to_string();

        assert_eq!(
            build_thread_meta("SQLite · /home/rasul/Documents/data.sqlite", &state),
            "SQLite · data.sqlite"
        );
    }

    #[test]
    fn hides_sql_execution_status_from_header_meta() {
        let mut state = AcpPanelState::new(
            models::AcpLaunchRequest {
                command: String::new(),
                args: String::new(),
                cwd: ".".to_string(),
                env: Vec::new(),
            },
            AcpOllamaConfig {
                base_url: String::new(),
                model: String::new(),
                api_key: String::new(),
            },
        );
        state.connected = true;
        state.status = "Executed read-only SQL from the ACP agent.".to_string();

        assert_eq!(
            build_thread_meta("SQLite · /home/rasul/Documents/data.sqlite", &state),
            "SQLite · data.sqlite"
        );
    }

    #[test]
    fn hides_disconnected_prompt_from_header_meta() {
        let state = AcpPanelState::new(
            models::AcpLaunchRequest {
                command: String::new(),
                args: String::new(),
                cwd: ".".to_string(),
                env: Vec::new(),
            },
            AcpOllamaConfig {
                base_url: String::new(),
                model: String::new(),
                api_key: String::new(),
            },
        );

        assert_eq!(
            build_thread_meta("SQLite · /home/rasul/Documents/data.sqlite", &state),
            "SQLite · data.sqlite"
        );
    }

    #[test]
    fn renders_markdown_emphasis_for_agent_messages() {
        let html =
            render_message_markdown_html("**Assumptions:** None - it doesn't query any tables");

        assert!(html.contains("<strong>Assumptions:</strong>"));
    }

    #[test]
    fn preserves_inline_code_while_rendering_markdown() {
        let html = render_message_markdown_html("Run `SELECT 1` against the active connection.");

        assert!(html.contains("<code>SELECT 1</code>"));
    }

    #[test]
    fn sanitizes_raw_html_in_markdown_messages() {
        let html = render_message_markdown_html("hello <script>alert(1)</script> world");
        assert!(!html.contains("<script>"));
        assert!(html.contains("hello"));
        assert!(html.contains("world"));
    }

    #[test]
    fn clipboard_fallback_uses_display_not_debug() {
        let formatted = format_clipboard_fallback_error("native error", "eval failed");
        assert_eq!(
            formatted,
            "Clipboard error: native error; fallback failed: eval failed"
        );
        assert!(!formatted.contains(":?"));
    }

    #[test]
    fn registry_loading_text_is_compact() {
        let text = acp_registry_loading_text();
        assert!(!is_verbose_acp_registry_loading_text(text));
        assert_eq!(text, "Loading agents...");
    }

    #[test]
    fn verbose_registry_preparing_text_is_detected() {
        assert!(is_verbose_acp_registry_preparing_text(
            "Preparing OpenCode from the ACP registry..."
        ));
        assert!(is_verbose_acp_registry_preparing_text(
            "Preparing some agent from the ACP registry..."
        ));
        assert!(!is_verbose_acp_registry_preparing_text(
            "Preparing agents..."
        ));
    }

    #[test]
    fn registry_connecting_text_is_compact() {
        let text = acp_registry_connecting_text("OpenCode");
        assert_eq!(text, "Connecting to OpenCode...");
    }
}
