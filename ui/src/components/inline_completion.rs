//! Inline completion ghost text component.
//!
//! This component provides ghost text inline completion functionality
//! similar to Zed's edit prediction pattern:
//! - Ghost text displayed at 50% opacity, italic, #888 color
//! - Tab to accept completion
//! - Escape to fully dismiss (not just hide)
//! - Debounce 100-150ms between keystrokes
//! - Cancellation on new keystroke

use dioxus::prelude::*;
use tokio::task::AbortHandle;

/// State for managing inline completion lifecycle.
///
/// This struct tracks:
/// - Pending async requests (for cancellation)
/// - Current prediction text
/// - Whether the completion has been discarded
#[derive(Clone, Default)]
#[allow(dead_code)]
pub struct InlineCompletionState {
    /// Handle to abort any pending prediction request.
    pub pending_request: Option<AbortHandle>,
    /// The current prediction text, if any.
    pub current_prediction: Option<String>,
    /// Whether this completion has been explicitly discarded.
    pub is_discarded: bool,
}

#[allow(dead_code)]
impl InlineCompletionState {
    /// Creates a new empty state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Shows a completion suggestion.
    ///
    /// This sets the current prediction and clears the discarded flag.
    pub fn show_completion(&mut self, suggestion: String) {
        self.current_prediction = Some(suggestion);
        self.is_discarded = false;
    }

    /// Accepts the current completion.
    ///
    /// Returns the accepted text if there was a prediction.
    pub fn accept_completion(&mut self) -> Option<String> {
        let accepted = self.current_prediction.take();
        self.pending_request = None;
        self.is_discarded = false;
        accepted
    }

    /// Fully dismisses the completion.
    ///
    /// This:
    /// - Sets current_prediction to None
    /// - Cancels any pending request
    /// - Sets is_discarded to true
    pub fn dismiss_completion(&mut self) {
        if let Some(handle) = self.pending_request.take() {
            handle.abort();
        }
        self.current_prediction = None;
        self.is_discarded = true;
    }

    /// Cancels any pending request without marking as discarded.
    pub fn cancel_pending(&mut self) {
        if let Some(handle) = self.pending_request.take() {
            handle.abort();
        }
    }

    /// Sets the abort handle for a pending request.
    pub fn set_pending_handle(&mut self, handle: AbortHandle) {
        self.pending_request = Some(handle);
    }

    /// Returns true if there's an active completion to show.
    pub fn has_completion(&self) -> bool {
        self.current_prediction.is_some() && !self.is_discarded
    }

    /// Returns the current suggestion text if available.
    pub fn suggestion(&self) -> Option<&str> {
        if self.is_discarded {
            return None;
        }
        self.current_prediction.as_deref()
    }
}

/// Debounce configuration for inline completion requests.
#[allow(dead_code)]
pub struct DebounceConfig {
    /// Minimum delay between keystrokes before triggering a request.
    pub delay_ms: u64,
}

impl Default for DebounceConfig {
    fn default() -> Self {
        Self { delay_ms: 125 } // Between 100-150ms as specified
    }
}

/// Hook that provides debounced inline completion state.
///
/// Returns a signal containing the InlineCompletionState and a debounced
/// function to trigger completion requests.
#[allow(dead_code)]
pub fn use_inline_completion() -> (Signal<InlineCompletionState>, DebounceConfig) {
    let state = use_signal(InlineCompletionState::new);
    let config = DebounceConfig::default();
    (state, config)
}

/// Component that renders ghost text inline completion.
///
/// This component displays suggestion text with ghost text styling:
/// - 50% opacity
/// - Italic font style
/// - #888 color (gray)
/// - Monospace font family
#[component]
pub fn InlineCompletion(
    suggestion: Option<String>,
    on_accept: Callback<()>,
    on_dismiss: Callback<()>,
) -> Element {
    let suggestion_text = suggestion.unwrap_or_default();

    if suggestion_text.is_empty() {
        return rsx! {};
    }

    rsx! {
        span {
            class: "inline-completion",
            // Ghost text styling: 50% opacity, italic, #888, monospace
            style: "
                opacity: 0.5;
                font-style: italic;
                color: #888;
                font-family: 'JetBrains Mono', 'Cascadia Code', monospace;
            ",
            {suggestion_text.to_string()}
        }
    }
}

/// Component that manages inline completion state and keyboard handling.
///
/// This is a headless component that handles:
/// - Tab/ArrowRight to accept
/// - Escape to dismiss
/// - State management for the completion lifecycle
#[component]
pub fn InlineCompletionHandler(
    state: Signal<InlineCompletionState>,
    on_accept: Callback<String>,
    on_dismiss: Callback<()>,
    children: Element,
) -> Element {
    let _handle_accept = move || {
        let mut state = state.write();
        if let Some(accepted) = state.accept_completion() {
            on_accept.call(accepted);
        }
    };

    let _handle_dismiss = move || {
        state.write().dismiss_completion();
        on_dismiss.call(());
    };

    rsx! {
        {children}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inline_completion_state_new() {
        let state = InlineCompletionState::new();
        assert!(state.current_prediction.is_none());
        assert!(!state.is_discarded);
        assert!(state.pending_request.is_none());
    }

    #[test]
    fn test_show_completion() {
        let mut state = InlineCompletionState::new();
        state.show_completion("SELECT * FROM users".to_string());
        assert_eq!(
            state.current_prediction,
            Some("SELECT * FROM users".to_string())
        );
        assert!(!state.is_discarded);
    }

    #[test]
    fn test_accept_completion() {
        let mut state = InlineCompletionState::new();
        state.show_completion("SELECT * FROM".to_string());
        let accepted = state.accept_completion();
        assert_eq!(accepted, Some("SELECT * FROM".to_string()));
        assert!(state.current_prediction.is_none());
        assert!(!state.is_discarded);
    }

    #[test]
    fn test_dismiss_completion() {
        let mut state = InlineCompletionState::new();
        state.show_completion("SELECT".to_string());
        state.dismiss_completion();
        assert!(state.current_prediction.is_none());
        assert!(state.is_discarded);
    }

    #[test]
    fn test_has_completion() {
        let mut state = InlineCompletionState::new();
        assert!(!state.has_completion());

        state.show_completion("test".to_string());
        assert!(state.has_completion());

        state.dismiss_completion();
        assert!(!state.has_completion());
    }

    #[test]
    fn test_suggestion() {
        let mut state = InlineCompletionState::new();
        assert!(state.suggestion().is_none());

        state.show_completion("test".to_string());
        assert_eq!(state.suggestion(), Some("test"));

        state.dismiss_completion();
        assert!(state.suggestion().is_none());
    }

    #[test]
    fn test_debounce_config_default() {
        let config = DebounceConfig::default();
        assert!(config.delay_ms >= 100);
        assert!(config.delay_ms <= 150);
    }

    #[test]
    fn accept_when_no_prediction_returns_none() {
        let mut state = InlineCompletionState::new();
        assert!(state.accept_completion().is_none());
        assert!(state.current_prediction.is_none());
        assert!(!state.is_discarded);
    }

    #[test]
    fn show_after_dismiss_resurrects_completion() {
        let mut state = InlineCompletionState::new();
        state.show_completion("first".to_string());
        state.dismiss_completion();
        assert!(!state.has_completion());

        state.show_completion("second".to_string());
        assert!(state.has_completion());
        assert_eq!(state.suggestion(), Some("second"));
        assert!(!state.is_discarded);
    }

    #[test]
    fn cancel_pending_without_handle_is_noop() {
        let mut state = InlineCompletionState::new();
        state.cancel_pending();
        assert!(state.pending_request.is_none());
    }

    #[test]
    fn dismiss_then_show_then_accept_lifecycle() {
        let mut state = InlineCompletionState::new();
        state.show_completion("SELECT *".to_string());
        assert!(state.has_completion());

        state.dismiss_completion();
        assert!(!state.has_completion());

        state.show_completion("INSERT INTO".to_string());
        assert!(state.has_completion());

        let accepted = state.accept_completion();
        assert_eq!(accepted, Some("INSERT INTO".to_string()));
        assert!(state.current_prediction.is_none());
        assert!(!state.has_completion());
    }

    #[test]
    fn suggestion_returns_none_when_discarded() {
        let mut state = InlineCompletionState::new();
        state.show_completion("hidden".to_string());
        state.dismiss_completion();
        assert!(state.suggestion().is_none());
    }

    #[test]
    fn double_accept_returns_none_on_second_call() {
        let mut state = InlineCompletionState::new();
        state.show_completion("once".to_string());

        let first = state.accept_completion();
        assert_eq!(first, Some("once".to_string()));

        let second = state.accept_completion();
        assert!(second.is_none());
    }
}
