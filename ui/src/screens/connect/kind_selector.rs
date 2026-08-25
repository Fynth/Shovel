use dioxus::prelude::*;
use models::DatabaseKind;

#[component]
pub fn KindSelector(mut selected_kind: Signal<DatabaseKind>) -> Element {
    let current_value = match selected_kind() {
        DatabaseKind::Sqlite => "sqlite",
        DatabaseKind::Postgres => "postgres",
        DatabaseKind::MySql => "mysql",
        DatabaseKind::ClickHouse => "clickhouse",
    };

    rsx! {
        div { class: "field",
            label {
                class: "field__label",
                r#for: "db-kind",
                "Connection type"
            }
            select {
                class: "input",
                id: "db-kind",
                value: current_value.to_string(),
                onchange: move |event| {
                    let next_kind = match event.value().as_str() {
                        "sqlite" => DatabaseKind::Sqlite,
                        "postgres" => DatabaseKind::Postgres,
                        "mysql" => DatabaseKind::MySql,
                        "clickhouse" => DatabaseKind::ClickHouse,
                        _ => DatabaseKind::Sqlite,
                    };
                    selected_kind.set(next_kind);
                },
                option { value: "sqlite", "SQLite" }
                option { value: "postgres", "PostgreSQL" }
                option { value: "mysql", "MySQL" }
                option { value: "clickhouse", "ClickHouse" }
            }
        }
    }
}
