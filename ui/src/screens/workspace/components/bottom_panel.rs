//! Bottom dock (Output / Messages / Query Log / Transactions / Problems).
//!
//! Mirrors the inspector/sidebar dock system visually but lives below the
//! main editor/results area. A horizontal tab strip drives which view is
//! rendered; the whole dock is hideable, resizable, and its
//! visibility/height are persisted via [`models::AppUiSettings`].

use crate::{
    app_state::{APP_LAST_QUERY, APP_TOAST, ToastKind, set_show_bottom_panel},
    screens::workspace::components::{ActionIcon, IconButton, IconGlyph},
};
use dioxus::prelude::*;
use models::QueryHistoryItem;

/// The five tabs the bottom dock can show. Ordering in [`BottomPanelTab::ALL`]
/// determines the tab strip order. Each variant maps to one section in
/// [`BottomPanelContent`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BottomPanelTab {
    Output,
    Messages,
    QueryLog,
    Transactions,
    Problems,
}

impl BottomPanelTab {
    /// Tab strip order. `Output` lands first because that is the highest
    /// signal-to-noise view in the dock and the one the user is most
    /// likely to want open by default.
    pub const ALL: [Self; 5] = [
        Self::Output,
        Self::Messages,
        Self::QueryLog,
        Self::Transactions,
        Self::Problems,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Output => "Output",
            Self::Messages => "Messages",
            Self::QueryLog => "Query Log",
            Self::Transactions => "Transactions",
            Self::Problems => "Problems",
        }
    }

    pub fn icon(self) -> ActionIcon {
        match self {
            Self::Output => ActionIcon::Output,
            Self::Messages => ActionIcon::Messages,
            Self::QueryLog => ActionIcon::QueryLog,
            Self::Transactions => ActionIcon::Transactions,
            Self::Problems => ActionIcon::Problems,
        }
    }
}

/// The dock itself. The parent (`WorkspaceBody`) is responsible for
/// driving visibility, resize, and the height CSS variable; this
/// component only owns which tab is currently active and renders the
/// tab strip + body for the visible dock.
#[component]
pub fn BottomPanelDock(
    history: Signal<Vec<QueryHistoryItem>>,
    active_tab: Signal<BottomPanelTab>,
) -> Element {
    let active = active_tab();
    let close_label = "Hide bottom dock".to_string();

    rsx! {
        section {
            class: "bottom-panel",
            aria_label: "Bottom dock",
            div {
                class: "bottom-panel__handle-row",
                div {
                    class: "bottom-panel__tabs",
                    role: "tablist",
                    for tab in BottomPanelTab::ALL {
                        {
                            let is_active = active == tab;
                            let class_name = if is_active {
                                "bottom-panel__tab bottom-panel__tab--active".to_string()
                            } else {
                                "bottom-panel__tab".to_string()
                            };
                            let tab_kind = tab;
                            let label = tab.label().to_string();
                            let tab_icon = tab.icon();
                            rsx! {
                                button {
                                    key: "{tab.label()}",
                                    class: class_name,
                                    role: "tab",
                                    aria_selected: is_active,
                                    onclick: move |_| {
                                        if active_tab() != tab_kind {
                                            active_tab.set(tab_kind);
                                        }
                                    },
                                    IconGlyph { icon: tab_icon }
                                    span { class: "bottom-panel__tab-label", "{label}" }
                                }
                            }
                        }
                    }
                }
                IconButton {
                    icon: ActionIcon::Close,
                    label: close_label,
                    small: true,
                    onclick: move |_| set_show_bottom_panel(false),
                }
            }
            div {
                class: "bottom-panel__body",
                role: "tabpanel",
                BottomPanelContent {
                    active_tab,
                    history,
                }
            }
        }
    }
}

/// Body of the active tab. Kept as its own component so a tab switch
/// only re-renders the body, not the tab strip.
#[component]
fn BottomPanelContent(
    active_tab: Signal<BottomPanelTab>,
    history: Signal<Vec<QueryHistoryItem>>,
) -> Element {
    match active_tab() {
        BottomPanelTab::Output => rsx! { OutputView {} },
        BottomPanelTab::Messages => rsx! { MessagesView {} },
        BottomPanelTab::QueryLog => rsx! { QueryLogView { history } },
        BottomPanelTab::Transactions => rsx! { TransactionsView {} },
        BottomPanelTab::Problems => rsx! { ProblemsView {} },
    }
}

/// Most recent query summary mirrored from the status bar. Falls back to
/// a friendly placeholder until the first query is executed in the
/// session.
#[component]
fn OutputView() -> Element {
    let last = APP_LAST_QUERY();
    rsx! {
        div {
            class: "bottom-panel__view bottom-panel__view--output",
            if let Some(summary) = last.as_ref() {
                div {
                    class: if summary.failed {
                        "bottom-panel__line bottom-panel__line--error"
                    } else {
                        "bottom-panel__line bottom-panel__line--ok"
                    },
                    span { class: "bottom-panel__line-label", "{summary.label}" }
                    if let Some(ms) = summary.duration_ms {
                        span { class: "bottom-panel__line-meta", " ({ms} ms)" }
                    }
                }
                p {
                    class: "bottom-panel__hint",
                    "Live output of the last executed query. Detailed rows remain in the active tab."
                }
            } else {
                p {
                    class: "bottom-panel__hint",
                    "No query has been executed in this session yet."
                }
            }
        }
    }
}

/// Mirrors the global toast log. The toast queue is capped inside
/// `app_state`; we render every entry newest-first. Clicking the
/// dismiss (×) on a single row is a no-op for now — toasts auto-clear
/// in the surface that produced them.
#[component]
fn MessagesView() -> Element {
    let toasts = APP_TOAST();
    let rows = toasts
        .iter()
        .rev()
        .map(|toast| {
            let kind_class = match toast.kind {
                ToastKind::Info => "bottom-panel__msg--info",
                ToastKind::Success => "bottom-panel__msg--success",
                ToastKind::Warning => "bottom-panel__msg--warning",
                ToastKind::Error => "bottom-panel__msg--error",
            };
            let message = toast.message.clone();
            rsx! {
                div {
                    key: "{toast.id}",
                    class: "bottom-panel__msg {kind_class}",
                    span { class: "bottom-panel__msg-text", "{message}" }
                }
            }
        })
        .collect::<Vec<_>>();

    rsx! {
        div {
            class: "bottom-panel__view bottom-panel__view--messages",
            if rows.is_empty() {
                p {
                    class: "bottom-panel__hint",
                    "No messages yet. Errors and warnings raised by the app appear here."
                }
            } else {
                for row in rows {
                    {row}
                }
            }
        }
    }
}

/// Recent queries for the active session. Backed by the same
/// [`QueryHistoryItem`] signal the dedicated History panel uses, so a
/// user toggling between the side panel and the bottom dock sees the
/// same data.
#[component]
fn QueryLogView(history: Signal<Vec<QueryHistoryItem>>) -> Element {
    let items = history
        .read()
        .iter()
        .rev()
        .take(40)
        .cloned()
        .collect::<Vec<_>>();

    rsx! {
        div {
            class: "bottom-panel__view bottom-panel__view--querylog",
            if items.is_empty() {
                p {
                    class: "bottom-panel__hint",
                    "Query Log is empty. Run a query to populate it."
                }
            } else {
                div {
                    class: "bottom-panel__log",
                    for item in items {
                        div {
                            key: "{item.id}",
                            class: "bottom-panel__log-row",
                            div {
                                class: "bottom-panel__log-meta",
                                span { class: "bottom-panel__log-title", "{item.tab_title}" }
                                span { class: "bottom-panel__log-outcome", "{item.outcome}" }
                            }
                            pre { class: "bottom-panel__log-sql", "{item.sql}" }
                        }
                    }
                }
            }
        }
    }
}

/// Placeholder for the Transactions tab. Live transaction state lives
/// inside per-tab `BatchRunState` and is surfaced from the active tab's
/// batch UI; surfacing it here would require a dedicated bridge that
/// is out of scope for the dock itself.
#[component]
fn TransactionsView() -> Element {
    rsx! {
        div {
            class: "bottom-panel__view bottom-panel__view--transactions",
            p {
                class: "bottom-panel__hint",
                "No active transaction. Pending changes from a multi-statement script are shown in the active tab's batch panel."
            }
        }
    }
}

/// Placeholder for the Problems tab. Problems are produced by per-tab
/// validators; aggregating them into a workspace-level stream is a
/// follow-up that the dock leaves room for.
#[component]
fn ProblemsView() -> Element {
    rsx! {
        div {
            class: "bottom-panel__view bottom-panel__view--problems",
            p {
                class: "bottom-panel__hint",
                "No problems. Validation errors and lint warnings from the active editor appear here once enabled."
            }
        }
    }
}
