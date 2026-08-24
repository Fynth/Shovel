use dioxus::prelude::*;
use models::QueryTabState;

use super::{
    ResultTable,
    table_structure::{DdlPanel, IndexesPanel, RelationsPanel, StructurePanel},
};

/// Coarse index for the sub-tab strip inside the table editor.
///
/// Numeric rather than a richer enum so we can store it in a
/// `use_signal<u8>` cheaply and the sub-tab bar can iterate over an
/// `ALL` slice in declaration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TableEditorTab {
    Data = 0,
    Structure = 1,
    Ddl = 2,
    Indexes = 3,
    Relations = 4,
}

impl TableEditorTab {
    pub const ALL: [TableEditorTab; 5] = [
        TableEditorTab::Data,
        TableEditorTab::Structure,
        TableEditorTab::Ddl,
        TableEditorTab::Indexes,
        TableEditorTab::Relations,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Data => "Data",
            Self::Structure => "Structure",
            Self::Ddl => "DDL",
            Self::Indexes => "Indexes",
            Self::Relations => "Relations",
        }
    }

    pub fn css_class(self) -> &'static str {
        match self {
            Self::Data => "data",
            Self::Structure => "structure",
            Self::Ddl => "ddl",
            Self::Indexes => "indexes",
            Self::Relations => "relations",
        }
    }
}

/// Shell for a previewed table: sub-tab strip (Data | Structure | DDL
/// | Indexes | Relations) over a body that hosts the matching panel.
///
/// `Data` reuses the existing virtualized `ResultTable` (read/edit
/// grid). The other panels are lazy — their data sources load on
/// first selection and are cached for subsequent selections inside
/// the component's lifetime.
#[component]
pub fn TableEditor(tabs: Signal<Vec<QueryTabState>>, active_tab_id: Signal<u64>) -> Element {
    let mut active_subtab = use_signal(|| TableEditorTab::Data);

    let current_tab = tabs
        .read()
        .iter()
        .find(|tab| tab.id == active_tab_id())
        .cloned();

    let Some(tab) = current_tab else {
        return rsx! {
            div { class: "table-editor",
                p { class: "empty-state", "No active tab." }
            }
        };
    };

    // The Structure and DDL/Indexes/Relations panels all read the
    // preview_source. Guard the whole editor behind it so we never
    // render an empty shell for a non-table query tab.
    let Some(source) = tab.preview_source.clone() else {
        return rsx! {
            div { class: "table-editor",
                p { class: "empty-state", "Open a table from the explorer to use the table editor." }
            }
        };
    };

    let session_id = tab.session_id;
    let table_name = source.table_name.clone();
    let schema = source.schema.clone();
    let result = tab.result.clone();
    let existing_result = tab.result.clone();

    rsx! {
        div { class: "table-editor",
            div {
                class: "table-editor__tabs",
                role: "tablist",
                "aria-label": "Table editor sections",
                for sub in TableEditorTab::ALL {
                    {
                        let class_name = if sub == active_subtab() {
                            format!("table-editor__tab table-editor__tab--active table-editor__tab--{}", sub.css_class())
                        } else {
                            format!("table-editor__tab table-editor__tab--{}", sub.css_class())
                        };
                        let label = sub.label();
                        let selected = sub == active_subtab();
                        rsx! {
                            button {
                                key: "{label}",
                                class: "{class_name}",
                                role: "tab",
                                "aria-selected": if selected { "true" } else { "false" },
                                onclick: move |_| active_subtab.set(sub),
                                "{label}"
                            }
                        }
                    }
                }
            }
            div { class: "table-editor__body",
                match active_subtab() {
                    TableEditorTab::Data => rsx! {
                        ResultTable {
                            result,
                            tabs,
                            active_tab_id,
                        }
                    },
                    TableEditorTab::Structure => rsx! {
                        StructurePanel {
                            tabs,
                            active_tab_id,
                            source: source.clone(),
                            session_id,
                            existing_result,
                        }
                    },
                    TableEditorTab::Ddl => rsx! {
                        DdlPanel {
                            tabs,
                            active_tab_id,
                            source: source.clone(),
                            session_id,
                        }
                    },
                    TableEditorTab::Indexes => rsx! {
                        IndexesPanel {
                            source: source.clone(),
                        }
                    },
                    TableEditorTab::Relations => rsx! {
                        RelationsPanel {
                            tabs,
                            active_tab_id,
                            source: source.clone(),
                            session_id,
                        }
                    },
                }
            }
            div { class: "table-editor__meta",
                span { class: "table-editor__meta-name", "{table_name}" }
                if let Some(schema_name) = schema.as_deref() {
                    span { class: "table-editor__meta-schema", "{schema_name}" }
                }
            }
        }
    }
}
