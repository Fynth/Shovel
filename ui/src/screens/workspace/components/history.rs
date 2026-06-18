use crate::app_state::{
    APP_STATE,
    activate_session,
    context_menu::{ContextMenuItem, open_context_menu},
};
use dioxus::prelude::*;
use models::{QueryHistoryItem, QueryTabState};

use crate::screens::workspace::actions::set_active_tab_sql;

const PAGE_SIZE: usize = 50;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DateFilter {
    All,
    Today,
    Week,
    Month,
}

impl DateFilter {
    fn label(self) -> &'static str {
        match self {
            Self::All => "All time",
            Self::Today => "Today",
            Self::Week => "This week",
            Self::Month => "This month",
        }
    }

    fn all() -> [Self; 4] {
        [Self::All, Self::Today, Self::Week, Self::Month]
    }

    fn cutoff(self) -> Option<i64> {
        let now = unix_timestamp_now();
        match self {
            Self::All => None,
            Self::Today => Some(now - 86_400),
            Self::Week => Some(now - 604_800),
            Self::Month => Some(now - 2_592_000),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OutcomeFilter {
    All,
    Success,
    Error,
}

impl OutcomeFilter {
    fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Success => "Success",
            Self::Error => "Error",
        }
    }

    fn all() -> [Self; 3] {
        [Self::All, Self::Success, Self::Error]
    }
}

fn unix_timestamp_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn duration_class(ms: u64) -> &'static str {
    if ms < 100 {
        "history__duration history__duration--fast"
    } else if ms < 1000 {
        "history__duration history__duration--medium"
    } else {
        "history__duration history__duration--slow"
    }
}

fn format_duration(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else {
        format!("{:.1}s", ms as f64 / 1000.0)
    }
}

fn format_rows(rows: Option<usize>) -> String {
    match rows {
        Some(n) => format!("{n} rows"),
        None => String::new(),
    }
}

fn format_timestamp(epoch: i64) -> String {
    let secs = epoch as u64;
    let days = secs / 86_400;
    let time_of_day = secs % 86_400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let now_days = unix_timestamp_now() as u64 / 86_400;
    let time_str = format!("{hours:02}:{minutes:02}");

    if days == now_days {
        format!("Today {time_str}")
    } else if days + 1 == now_days {
        format!("Yesterday {time_str}")
    } else {
        let days_ago = now_days.saturating_sub(days);
        if days_ago < 7 {
            format!("{days_ago}d ago {time_str}")
        } else {
            format!("{days_ago}d ago")
        }
    }
}

fn apply_filters(
    items: Vec<QueryHistoryItem>,
    date: DateFilter,
    connection: &str,
    outcome: OutcomeFilter,
) -> Vec<QueryHistoryItem> {
    let cutoff = date.cutoff();
    items
        .into_iter()
        .filter(|item| {
            if let Some(cutoff) = cutoff
                && item.executed_at < cutoff
            {
                return false;
            }
            if !connection.is_empty() && item.connection_name != connection {
                return false;
            }
            let is_error = item.outcome.starts_with("Error");
            match outcome {
                OutcomeFilter::All => true,
                OutcomeFilter::Success => !is_error,
                OutcomeFilter::Error => is_error,
            }
        })
        .collect()
}

fn redact_sql_display(sql: &str) -> String {
    let lower = sql.to_lowercase();
    if lower.contains("password") || lower.contains("secret") || lower.contains("token") {
        let mut result = sql.to_string();
        for sensitive in ["password", "secret", "token"] {
            if lower.contains(sensitive) {
                result = result
                    .lines()
                    .map(|line| {
                        let line_lower = line.to_lowercase();
                        if line_lower.contains(sensitive) {
                            if let Some(eq_pos) = line.find('=') {
                                let (before, _) = line.split_at(eq_pos + 1);
                                format!("{} [REDACTED]", before.trim_end())
                            } else {
                                line.to_string()
                            }
                        } else {
                            line.to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
            }
        }
        result
    } else {
        sql.to_string()
    }
}

fn build_csv(items: &[QueryHistoryItem]) -> String {
    let mut csv = String::from(
        "id,sql,outcome,duration_ms,rows_returned,executed_at,connection_name,connection_type,error_message\n",
    );
    for item in items {
        let sql_escaped = format!("\"{}\"", item.sql.replace('"', "\"\""));
        let rows_str = item
            .rows_returned
            .map(|r| r.to_string())
            .unwrap_or_default();
        let error_msg = item
            .error_message
            .as_deref()
            .map(|m| format!("\"{}\"", m.replace('"', "\"\"")))
            .unwrap_or_default();
        csv.push_str(&item.id.to_string());
        csv.push(',');
        csv.push_str(&sql_escaped);
        csv.push(',');
        csv.push_str(&format!("\"{}\"", item.outcome));
        csv.push(',');
        csv.push_str(&item.duration_ms.to_string());
        csv.push(',');
        csv.push_str(&rows_str);
        csv.push(',');
        csv.push_str(&item.executed_at.to_string());
        csv.push(',');
        csv.push_str(&format!("\"{}\"", item.connection_name));
        csv.push(',');
        csv.push_str(&format!("\"{}\"", item.connection_type));
        csv.push(',');
        csv.push_str(&error_msg);
        csv.push('\n');
    }
    csv
}

#[component]
pub fn QueryHistoryPanel(
    history: Signal<Vec<QueryHistoryItem>>,
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: Signal<u64>,
) -> Element {
    let mut search_query = use_signal(String::new);
    let mut date_filter = use_signal(|| DateFilter::All);
    let mut connection_filter = use_signal(String::new);
    let mut outcome_filter = use_signal(|| OutcomeFilter::All);
    let mut current_page = use_signal(|| 0usize);

    let search_results = use_resource(move || {
        let query = search_query();
        async move {
            if query.trim().is_empty() {
                return Vec::new();
            }
            let _ = services::QueryHistoryStore::init().await;
            services::QueryHistoryStore::search(&query)
                .await
                .unwrap_or_default()
        }
    });

    let searching = !search_query().trim().is_empty();
    let base_items: Vec<QueryHistoryItem> = if searching {
        search_results().unwrap_or_default()
    } else {
        history()
    };

    let connection_names: Vec<String> = {
        let mut set = std::collections::HashSet::new();
        for item in history() {
            if !item.connection_name.is_empty() {
                set.insert(item.connection_name.clone());
            }
        }
        let mut names: Vec<String> = set.into_iter().collect();
        names.sort();
        names
    };

    let filtered = apply_filters(
        base_items,
        date_filter(),
        &connection_filter(),
        outcome_filter(),
    );

    let total_items = filtered.len();
    let total_pages = if total_items == 0 {
        1
    } else {
        total_items.div_ceil(PAGE_SIZE)
    };
    let page = current_page().min(total_pages.saturating_sub(1));
    let page_start = page * PAGE_SIZE;
    let page_items: Vec<QueryHistoryItem> = filtered
        .into_iter()
        .skip(page_start)
        .take(PAGE_SIZE)
        .collect();

    let session_ids_by_name = APP_STATE
        .read()
        .sessions
        .iter()
        .map(|session| (session.name.clone(), session.id))
        .collect::<std::collections::HashMap<_, _>>();

    let export_items = page_items.clone();

    rsx! {
        section {
            class: "history",
            div {
                class: "history__header",
                div {
                    class: "history__header-row",
                    h2 { class: "workspace__section-title", "History" }
                    button {
                        class: "button button--ghost button--small history__export",
                        onclick: move |_| {
                            let items = export_items.clone();
                            spawn(async move {
                                let Some(file) = rfd::AsyncFileDialog::new()
                                    .set_file_name("query_history.csv")
                                    .add_filter("CSV", &["csv"])
                                    .save_file()
                                    .await
                                else {
                                    return;
                                };
                                let csv = build_csv(&items);
                                let _ = std::fs::write(file.path(), csv);
                            });
                        },
                        "Export history"
                    }
                }
            }

            div {
                class: "history__search",
                input {
                    class: "input history__search-input",
                    r#type: "text",
                    placeholder: "Search SQL text…",
                    value: "{search_query}",
                    oninput: move |e| {
                        search_query.set(e.value());
                        current_page.set(0);
                    },
                }
                if searching {
                    button {
                        class: "button button--ghost button--small history__search-clear",
                        onclick: move |_| search_query.set(String::new()),
                        "Clear"
                    }
                }
            }

            div {
                class: "history__filters",
                select {
                    class: "input history__filter-select",
                    onchange: move |e| {
                        date_filter.set(match e.value().as_str() {
                            "today" => DateFilter::Today,
                            "week" => DateFilter::Week,
                            "month" => DateFilter::Month,
                            _ => DateFilter::All,
                        });
                        current_page.set(0);
                    },
                    for variant in DateFilter::all() {
                        option {
                            value: match variant {
                                DateFilter::All => "all",
                                DateFilter::Today => "today",
                                DateFilter::Week => "week",
                                DateFilter::Month => "month",
                            },
                            selected: date_filter() == variant,
                            "{variant.label()}"
                        }
                    }
                }
                select {
                    class: "input history__filter-select",
                    onchange: move |e| {
                        let val = e.value();
                        connection_filter.set(if val == "all" { String::new() } else { val });
                        current_page.set(0);
                    },
                    option {
                        value: "all",
                        selected: connection_filter().is_empty(),
                        "All connections"
                    }
                    for name in &connection_names {
                        option {
                            value: "{name}",
                            selected: connection_filter() == *name,
                            "{name}"
                        }
                    }
                }
                select {
                    class: "input history__filter-select",
                    onchange: move |e| {
                        outcome_filter.set(match e.value().as_str() {
                            "success" => OutcomeFilter::Success,
                            "error" => OutcomeFilter::Error,
                            _ => OutcomeFilter::All,
                        });
                        current_page.set(0);
                    },
                    for variant in OutcomeFilter::all() {
                        option {
                            value: match variant {
                                OutcomeFilter::All => "all",
                                OutcomeFilter::Success => "success",
                                OutcomeFilter::Error => "error",
                            },
                            selected: outcome_filter() == variant,
                            "{variant.label()}"
                        }
                    }
                }
            }

            div {
                class: "history__list",
                if searching && search_results().is_none() {
                    p { class: "empty-state", "Searching…" }
                } else if page_items.is_empty() {
                    p { class: "empty-state",
                        if searching { "No matching queries found." }
                        else { "No executed queries yet." }
                    }
                } else {
                    for item in page_items {
                        {
                            let source_session_id = session_ids_by_name.get(&item.connection_name).copied();
                            let (connection_kind, connection_target) = item
                                .connection_name
                                .split_once(" · ")
                                .map(|(kind, target)| (kind.to_string(), target.to_string()))
                                .unwrap_or_else(|| (String::new(), item.connection_name.clone()));
                            let is_error = item.outcome.starts_with("Error");
                            let outcome_class = if is_error {
                                "history__outcome history__outcome--error"
                            } else {
                                "history__outcome history__outcome--success"
                            };
                            let outcome_label = if is_error { "Error" } else { "Success" };
                            let display_sql = redact_sql_display(&item.sql);
                            let dur_class = duration_class(item.duration_ms);
                            let dur_text = format_duration(item.duration_ms);
                            let rows_text = format_rows(item.rows_returned);
                            let time_text = if item.executed_at > 0 {
                                format_timestamp(item.executed_at)
                            } else {
                                String::new()
                            };

                            let context_items = build_history_context_menu(
                                item.clone(),
                                source_session_id,
                                tabs,
                                active_tab_id,
                                history,
                            );

                            rsx! {
                                div {
                                    class: "history__item",
                                    oncontextmenu: move |event| {
                                        event.prevent_default();
                                        let coords = event.client_coordinates();
                                        open_context_menu(coords.x, coords.y, context_items.clone());
                                    },
                                    div {
                                        class: "history__meta",
                                        div {
                                            class: "history__topline",
                                            if !item.tab_title.is_empty() {
                                                p { class: "history__title", "{item.tab_title}" }
                                            }
                                            div {
                                                class: "history__metrics",
                                                span {
                                                    class: dur_class,
                                                    title: "Duration",
                                                    "{dur_text}"
                                                }
                                                if !rows_text.is_empty() {
                                                    span {
                                                        class: "history__rows",
                                                        title: "Rows returned",
                                                        "{rows_text}"
                                                    }
                                                }
                                                if !time_text.is_empty() {
                                                    span {
                                                        class: "history__time",
                                                        "{time_text}"
                                                    }
                                                }
                                            }
                                            p {
                                                class: outcome_class,
                                                title: "{item.outcome}",
                                                "{outcome_label}"
                                            }
                                        }
                                        if !connection_target.is_empty() {
                                            div {
                                                class: "history__connection",
                                                if !connection_kind.is_empty() {
                                                    span { class: "history__connection-kind", "{connection_kind}" }
                                                }
                                                span {
                                                    class: "history__connection-target",
                                                    title: "{item.connection_name}",
                                                    "{connection_target}"
                                                }
                                            }
                                        }
                                        if is_error {
                                            if let Some(err) = &item.error_message {
                                                p {
                                                    class: "history__error-message",
                                                    title: "{err}",
                                                    "{err}"
                                                }
                                            }
                                        }
                                    }
                                    pre {
                                        class: "history__sql",
                                        title: "{display_sql}",
                                        "{display_sql}"
                                    }
                                    div {
                                        class: "history__actions",
                                        if let Some(session_id) = source_session_id {
                                            button {
                                                class: "button button--ghost button--small",
                                                onclick: move |_| activate_session(session_id),
                                                "Activate"
                                            }
                                        },
                                        button {
                                            class: "button button--ghost button--small",
                                            onclick: {
                                                let sql = item.sql.clone();
                                                move |_| {
                                                    set_active_tab_sql(
                                                        tabs,
                                                        active_tab_id(),
                                                        sql.clone(),
                                                        "Loaded query from history".to_string(),
                                                    );
                                                }
                                            },
                                            "Load in tab"
                                        }
                                        button {
                                            class: "button button--ghost button--small",
                                            onclick: {
                                                let sql = item.sql.clone();
                                                move |_| {
                                                    set_active_tab_sql(
                                                        tabs,
                                                        active_tab_id(),
                                                        sql.clone(),
                                                        "Copied query to editor".to_string(),
                                                    );
                                                }
                                            },
                                            "Copy to editor"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if total_items > PAGE_SIZE {
                div {
                    class: "history__pagination",
                    button {
                        class: "button button--ghost button--small",
                        disabled: page == 0,
                        onclick: move |_| current_page.set(page.saturating_sub(1)),
                        "Prev"
                    }
                    span {
                        class: "history__pagination-info",
                        "Page {page + 1} of {total_pages} · {total_items} items"
                    }
                    button {
                        class: "button button--ghost button--small",
                        disabled: page + 1 >= total_pages,
                        onclick: move |_| current_page.set(page + 1),
                        "Next"
                    }
                }
            }
        }
    }
}

fn build_history_context_menu(
    item: QueryHistoryItem,
    source_session_id: Option<u64>,
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: Signal<u64>,
    mut history_signal: Signal<Vec<QueryHistoryItem>>,
) -> Vec<ContextMenuItem> {
    use crate::{app_state::context_menu::copy_to_clipboard, screens::workspace::ActionIcon};

    let mut items: Vec<ContextMenuItem> = Vec::new();

    // 1. Load in tab — replaces the SQL in the active tab.
    {
        let sql = item.sql.clone();
        items.push(
            ContextMenuItem::new("Load in tab", move || {
                set_active_tab_sql(
                    tabs,
                    active_tab_id(),
                    sql.clone(),
                    "Loaded query from history".to_string(),
                );
            })
            .with_icon(ActionIcon::Run),
        );
    }

    // 2. Copy SQL (verbatim, including any passwords / secrets).
    {
        let sql = item.sql.clone();
        items.push(
            ContextMenuItem::new("Copy SQL", move || {
                let _ = copy_to_clipboard(sql.clone());
            })
            .with_icon(ActionIcon::Duplicate),
        );
    }

    // 3. Copy redacted SQL — uses the same `redact_sql_display`
    //    helper that the inline history view uses to mask lines
    //    containing `password` / `secret` / `token`.
    {
        let sql = item.sql.clone();
        items.push(
            ContextMenuItem::new("Copy redacted SQL", move || {
                let redacted = redact_sql_display(&sql);
                let _ = copy_to_clipboard(redacted);
            })
            .with_icon(ActionIcon::Duplicate)
            .separator(),
        );
    }

    // 4. Activate the originating session — only when we can
    //    resolve the saved connection name back to a live session.
    if let Some(session_id) = source_session_id {
        items.push(
            ContextMenuItem::new("Activate connection", move || {
                activate_session(session_id);
            })
            .with_icon(ActionIcon::Refresh),
        );
    }

    // 5. Delete this single entry.
    //
    //    The row is removed from the in-memory signal first so the
    //    list updates instantly. A background `QueryHistoryStore::delete`
    //    call then removes the FTS5 row + the SQLite row. If the
    //    background call fails the toast surfaces the error and the
    //    row is restored on the next reload.
    items.push(
        ContextMenuItem::new("Delete entry", move || {
            let id = item.id;
            history_signal.with_mut(|list| list.retain(|i| i.id != id));
            spawn(async move {
                if let Err(err) = services::QueryHistoryStore::delete(id).await {
                    crate::app_state::toast_error(format!("Failed to delete history entry: {err}"));
                }
            });
        })
        .with_icon(ActionIcon::Delete)
        .separator(),
    );

    // 6. Clear every history entry — destructive, lives at the end
    //    of the menu and behind its own danger visual.
    items.push(
        ContextMenuItem::new("Clear all history…", move || {
            history_signal.with_mut(|list| list.clear());
            spawn(async move {
                match services::QueryHistoryStore::clear().await {
                    Ok(n) => crate::app_state::show_toast(
                        format!("Cleared {n} history entries"),
                        crate::app_state::ToastKind::Info,
                    ),
                    Err(err) =>
                        crate::app_state::toast_error(format!("Failed to clear history: {err}")),
                }
            });
        })
        .with_icon(ActionIcon::Delete),
    );

    items
}
