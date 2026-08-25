use crate::screens::workspace::{
    components::ResultTable,
    helpers::format_duration,
    tab_store::TabStore,
};
use dioxus::prelude::*;
use models::{BatchOutcome, BatchRunState};

/// Цвет/подпись исхода оператора для индикатора во вкладке.
fn outcome_label(outcome: BatchOutcome) -> &'static str {
    match outcome {
        BatchOutcome::Ok => "OK",
        BatchOutcome::Error => "Error",
        BatchOutcome::Skipped => "Skipped",
        BatchOutcome::Running => "Running",
    }
}

fn outcome_class(outcome: BatchOutcome) -> &'static str {
    match outcome {
        BatchOutcome::Ok => "batch-results__chip--ok",
        BatchOutcome::Error => "batch-results__chip--error",
        BatchOutcome::Skipped => "batch-results__chip--skipped",
        BatchOutcome::Running => "batch-results__chip--running",
    }
}

/// Переключает активную вкладку пакетного результата и синхронизирует
/// `tab.result` с выходом выбранного оператора, чтобы экспорт и
/// статус-бар работали как для обычного однооператорного запроса.
fn select_batch_index(mut store: TabStore, index: usize) {
    let active_tab_id = store.active_tab_id();
    store.result.with_mut(|m| {
        if let Some(res) = m.get_mut(&active_tab_id) {
            if let Some(batch) = res.batch_results.as_mut() {
                batch.active_index = index;
            }
            res.result = if index < res.batch_outputs.len() {
                res.batch_outputs[index].clone()
            } else {
                None
            };
        }
    });
}

/// Панель результатов пакетного выполнения: горизонтальная лента
/// пооператорных вкладок + вкладка «Status» с итоговой сводкой.
/// Выбранная вкладка оператора рендерит свой `QueryOutput` через
/// `ResultTable`; вкладка Status показывает сводку по всем операторам.
#[component]
pub fn BatchResultsView(store: TabStore) -> Element {
    let active_tab_id = store.active_tab_id();
    let current_tab = store.result.read().get(&active_tab_id).cloned();
    let Some(current_tab) = current_tab else {
        return rsx! {};
    };
    let Some(batch) = current_tab.batch_results.clone() else {
        return rsx! {};
    };

    let statement_count = batch.results.len();
    let status_index = statement_count; // вкладка Status идёт после операторов
    let active_index = batch.active_index.min(status_index);
    let is_status_tab = active_index >= statement_count;
    let selected_output = current_tab
        .batch_outputs
        .get(active_index)
        .cloned()
        .flatten();

    rsx! {
        div { class: "batch-results",
            div { class: "batch-results__strip",
                for (pos, result) in batch.results.iter().enumerate() {
                    {
                        let active = pos == active_index;
                        let class = format!(
                            "batch-results__tab{}",
                            if active { " batch-results__tab--active" } else { "" }
                        );
                        let label = format!("{}. {}", pos + 1, result.preview);
                        let chip_class = outcome_class(result.outcome);
                        let chip_label = outcome_label(result.outcome);
                        let meta = match (result.duration_ms, result.rows) {
                            (Some(ms), Some(rows)) => format!("{} rows · {}", rows, format_duration(ms)),
                            (Some(ms), None) => format_duration(ms),
                            (None, Some(rows)) => format!("{} rows", rows),
                            (None, None) => String::new(),
                        };
                        rsx! {
                            button {
                                class,
                                title: {label.to_string()},
                                onclick: move |_| select_batch_index(store, pos),
                                span { class: "batch-results__tab-label", {label.to_string()} }
                                span { class: "batch-results__tab-meta",
                                    span { class: "batch-results__chip {chip_class}", {chip_label.to_string()} }
                                    if !meta.is_empty() {
                                        span { class: "batch-results__tab-duration", {meta.to_string()} }
                                    }
                                }
                            }
                        }
                    }
                }
                {
                    let active = is_status_tab;
                    let class = format!(
                        "batch-results__tab batch-results__tab--status{}",
                        if active { " batch-results__tab--active" } else { "" }
                    );
                    rsx! {
                        button {
                            class,
                            onclick: move |_| select_batch_index(store, status_index),
                            "Status"
                        }
                    }
                }
            }

            if is_status_tab {
                { render_status_summary(&batch) }
            } else if let Some(output) = selected_output {
                div { class: "batch-results__grid",
                    ResultTable {
                        result: Some(output),
                        store,
                    }
                }
            } else {
                // Оператор без сохранённого выхода (ошибка/пропуск) —
                // показываем сводку по этому оператору.
                { render_statement_summary(&batch, active_index) }
            }
        }
    }
}

fn render_status_summary(batch: &BatchRunState) -> Element {
    let ok_count = batch
        .results
        .iter()
        .filter(|r| r.outcome == BatchOutcome::Ok)
        .count();
    let error_count = batch
        .results
        .iter()
        .filter(|r| r.outcome == BatchOutcome::Error)
        .count();
    let skipped_count = batch
        .results
        .iter()
        .filter(|r| r.outcome == BatchOutcome::Skipped)
        .count();
    let total_rows: usize = batch.results.iter().filter_map(|r| r.rows).sum();

    let tx_label = match batch.tx_state {
        models::BatchTransactionState::None => "Auto-commit (per statement)",
        models::BatchTransactionState::InProgress => "Transaction in progress",
        models::BatchTransactionState::Committed => "Committed",
        models::BatchTransactionState::RolledBack => "Rolled back",
    };

    rsx! {
        div { class: "batch-results__summary",
            div { class: "batch-results__summary-header",
                h3 { class: "batch-results__summary-title", "Batch summary" }
                span { class: "batch-results__summary-total",
                    "{batch.results.len()} statements · {format_duration(batch.total_duration_ms)}"
                }
            }
            div { class: "batch-results__summary-stats",
                span { class: "batch-results__chip batch-results__chip--ok", "OK: {ok_count}" }
                if error_count > 0 {
                    span { class: "batch-results__chip batch-results__chip--error", "Errors: {error_count}" }
                }
                if skipped_count > 0 {
                    span { class: "batch-results__chip batch-results__chip--skipped", "Skipped: {skipped_count}" }
                }
                span { class: "batch-results__summary-rows", "Rows: {total_rows}" }
                span { class: "batch-results__summary-tx", {tx_label.to_string()} }
            }
            div { class: "batch-results__summary-list",
                for (pos, result) in batch.results.iter().enumerate() {
                    {
                        let chip_class = outcome_class(result.outcome);
                        let chip_label = outcome_label(result.outcome);
                        let meta = match (result.duration_ms, result.rows) {
                            (Some(ms), Some(rows)) => format!("{rows} rows · {}", format_duration(ms)),
                            (Some(ms), None) => format_duration(ms),
                            (None, Some(rows)) => format!("{rows} rows"),
                            (None, None) => String::new(),
                        };
                        rsx! {
                            div { class: "batch-results__summary-row",
                                span { class: "batch-results__summary-index", "#{pos + 1}" }
                                span { class: "batch-results__summary-preview", "{result.preview}" }
                                span { class: "batch-results__chip {chip_class}", {chip_label.to_string()} }
                                if !meta.is_empty() {
                                    span { class: "batch-results__summary-meta", {meta.to_string()} }
                                }
                                if let Some(msg) = result.error_message.as_ref() {
                                    span { class: "batch-results__summary-error", {msg.to_string()} }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn render_statement_summary(batch: &BatchRunState, index: usize) -> Element {
    let Some(result) = batch.results.get(index) else {
        return rsx! {};
    };
    let chip_class = outcome_class(result.outcome);
    let chip_label = outcome_label(result.outcome);
    let rows_label = result
        .rows
        .map(|r| format!("{r} rows affected"))
        .unwrap_or_else(|| "No rows returned".to_string());

    rsx! {
        div { class: "batch-results__summary batch-results__summary--single",
            div { class: "batch-results__summary-header",
                h3 { class: "batch-results__summary-title", "Statement #{index + 1}" }
                span { class: "batch-results__chip {chip_class}", {chip_label.to_string()} }
            }
            p { class: "batch-results__summary-preview", "{result.preview}" }
            p { class: "batch-results__summary-rows", {rows_label.to_string()} }
            if let Some(msg) = result.error_message.as_ref() {
                p { class: "batch-results__summary-error", {msg.to_string()} }
            }
        }
    }
}
