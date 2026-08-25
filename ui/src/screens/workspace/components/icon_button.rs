use crate::app_state::{hide_tooltip, show_tooltip};
use dioxus::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ActionIcon {
    SavedQueries,
    Connections,
    Explorer,
    History,
    SqlEditor,
    Agent,
    Refresh,
    NewConnection,
    Run,
    Clear,
    Format,
    Generate,
    Explain,
    CreateTable,
    Structure,
    ExportCsv,
    ExportJson,
    ExportXlsx,
    ExportXml,
    ExportHtml,
    ExportSql,
    ImportCsv,
    InsertRow,
    Apply,
    Undo,
    Delete,
    Truncate,
    Duplicate,
    Details,
    AddRule,
    Filter,
    FilterApply,
    FilterClear,
    Previous,
    Next,
    Close,
    Output,
    Messages,
    Split,
    QueryLog,
    Transactions,
    Problems,
    /// Toolbar overflow entry point — opens the consolidated "View"
    /// menu that lists the panel-visibility toggles.
    ViewMenu,
    /// Toolbar overflow entry point — opens a dropdown of secondary
    /// actions for the current toolbar (result grid / SQL editor).
    More,
}

#[component]
pub fn IconButton(
    icon: ActionIcon,
    label: String,
    onclick: EventHandler<MouseEvent>,
    #[props(default = false)] active: bool,
    #[props(default = false)] disabled: bool,
    #[props(default = false)] primary: bool,
    #[props(default = false)] small: bool,
) -> Element {
    let mut class_name = String::from("button button--icon");
    if primary {
        class_name.push_str(" button--primary");
    } else {
        class_name.push_str(" button--ghost");
    }
    if small {
        class_name.push_str(" button--small");
    }
    if active {
        class_name.push_str(" button--active");
    }

    rsx! {
        button {
            class: class_name,
            disabled,
            "aria-label": label.clone(),
            onmouseenter: {
                let label = label.clone();
                move |event| {
                    let position = event.client_coordinates();
                    show_tooltip(label.clone(), position.x, position.y);
                }
            },
            onmousemove: {
                let label = label.clone();
                move |event| {
                    let position = event.client_coordinates();
                    show_tooltip(label.clone(), position.x, position.y);
                }
            },
            onmouseleave: move |_| hide_tooltip(),
            onmousedown: move |_| hide_tooltip(),
            onblur: move |_| hide_tooltip(),
            onclick: move |event| onclick.call(event),
            IconGlyph { icon }
            span { class: "button__sr-label", {label.to_string()} }
        }
    }
}

#[component]
pub fn IconGlyph(icon: ActionIcon) -> Element {
    let icon_class = match icon {
        ActionIcon::Close => "button__icon button__icon--close",
        _ => "button__icon",
    };
    let stroke_width = match icon {
        ActionIcon::Close => "2.35",
        _ => "1.85",
    };

    rsx! {
        svg {
            class: icon_class,
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width,
            stroke_linecap: "round",
            stroke_linejoin: "round",
            match icon {
                ActionIcon::SavedQueries => rsx! {
                    path { d: "M6 5h12a1 1 0 0 1 1 1v13l-4-2.5L12 19l-3-2.5L5 19V6a1 1 0 0 1 1-1z" }
                    path { d: "M8 9h8" }
                    path { d: "M8 12h6" }
                },
                ActionIcon::Connections => rsx! {
                    rect { x: "4", y: "4", width: "16", height: "6", rx: "2" }
                    rect { x: "4", y: "14", width: "16", height: "6", rx: "2" }
                    circle { cx: "8", cy: "7", r: "0.9", fill: "currentColor", stroke: "none" }
                    circle { cx: "8", cy: "17", r: "0.9", fill: "currentColor", stroke: "none" }
                },
                ActionIcon::Explorer => rsx! {
                    path { d: "M4 6.5h6l2 2H20v9.5a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2z" }
                    path { d: "M9 12v6" }
                    path { d: "M9 15h4" }
                    path { d: "M13 15h2v3" }
                },
                ActionIcon::History => rsx! {
                    circle { cx: "12", cy: "12", r: "8" }
                    path { d: "M12 8v4l3 2" }
                    path { d: "M8 4H5v3" }
                },
                ActionIcon::SqlEditor => rsx! {
                    path { d: "M8 3h7l4 4v13a1 1 0 0 1-1 1H8a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z" }
                    path { d: "M15 3v5h4" }
                    path { d: "m11 15-2-2 2-2" }
                    path { d: "m13 11 2 2-2 2" }
                },
                ActionIcon::Agent => rsx! {
                    rect { x: "5", y: "7", width: "14", height: "10", rx: "3" }
                    path { d: "M9 7V5a3 3 0 0 1 6 0v2" }
                    circle { cx: "10", cy: "12", r: "1", fill: "currentColor", stroke: "none" }
                    circle { cx: "14", cy: "12", r: "1", fill: "currentColor", stroke: "none" }
                    path { d: "M10 15h4" }
                },
                ActionIcon::Refresh => rsx! {
                    path { d: "M19 11a7 7 0 1 1-2.1-5" }
                    path { d: "M19 6v5h-5" }
                },
                ActionIcon::NewConnection => rsx! {
                    path { d: "M9 8V5h6v3" }
                    path { d: "M7 12v-2a2 2 0 0 1 2-2h6a2 2 0 0 1 2 2v2" }
                    path { d: "M12 12v7" }
                    path { d: "M8.5 15.5h7" }
                },
                ActionIcon::Run => rsx! {
                    path { d: "M8 6v12l10-6z", fill: "currentColor", stroke: "none" }
                },
                ActionIcon::Clear => rsx! {
                    path { d: "m7 7 10 10" }
                    path { d: "m17 7-10 10" }
                },
                ActionIcon::Format => rsx! {
                    path { d: "M5 7h14" }
                    path { d: "M5 11h10" }
                    path { d: "M5 15h14" }
                    path { d: "M5 19h10" }
                },
                ActionIcon::Generate => rsx! {
                    path { d: "M12 4v4" }
                    path { d: "M12 16v4" }
                    path { d: "M4 12h4" }
                    path { d: "M16 12h4" }
                    path { d: "m6.5 6.5 2.8 2.8" }
                    path { d: "m14.7 14.7 2.8 2.8" }
                    path { d: "m17.5 6.5-2.8 2.8" }
                    path { d: "m9.3 14.7-2.8 2.8" }
                    circle { cx: "12", cy: "12", r: "2.2" }
                },
                ActionIcon::Explain => rsx! {
                    path { d: "M4 4h6v6H4z" }
                    path { d: "M14 4h6v6h-6z" }
                    path { d: "M7 10v2" }
                    path { d: "M17 10v2" }
                    path { d: "M7 12h10" }
                    path { d: "M12 12v4" }
                    path { d: "M10 16h4" }
                    path { d: "M4 16h6v4H4z" }
                    path { d: "M14 16h6v4h-6z" }
                },
                ActionIcon::CreateTable => rsx! {
                    rect { x: "4", y: "5", width: "12", height: "14", rx: "2" }
                    path { d: "M4 10h12" }
                    path { d: "M10 5v14" }
                    path { d: "M19 9v8" }
                    path { d: "M15 13h8" }
                },
                ActionIcon::Structure => rsx! {
                    rect { x: "4", y: "5", width: "16", height: "14", rx: "2" }
                    path { d: "M4 10h16" }
                    path { d: "M10 10v9" }
                },
                ActionIcon::ExportCsv => rsx! {
                    path { d: "M7 4h7l3 3v6" }
                    path { d: "M14 4v3h3" }
                    path { d: "M8 14v5" }
                    path { d: "m5.5 16.5 2.5 2.5 2.5-2.5" }
                    path { d: "M12.5 15.5h6" }
                    path { d: "M12.5 18.5h6" }
                },
                ActionIcon::ExportJson => rsx! {
                    path { d: "M10 5c-1.5 0-2 1-2 2.5v2c0 1-.5 1.5-1.5 2 .9.4 1.5 1 1.5 2v2c0 1.5.5 2.5 2 2.5" }
                    path { d: "M14 5c1.5 0 2 1 2 2.5v2c0 1 .5 1.5 1.5 2-.9.4-1.5 1-1.5 2v2c0 1.5-.5 2.5-2 2.5" }
                    path { d: "M12 7v10" }
                    path { d: "m9.5 14.5 2.5 2.5 2.5-2.5" }
                },
                ActionIcon::ExportXlsx => rsx! {
                    path { d: "M7 4h7l3 3v13H7z" }
                    path { d: "M14 4v3h3" }
                    path { d: "m9 12 4 5" }
                    path { d: "m13 12-4 5" }
                },
                ActionIcon::ExportXml => rsx! {
                    path { d: "M7 4h7l3 3v13H7z" }
                    path { d: "M14 4v3h3" }
                    path { d: "M9 13l3-3 3 3" }
                    path { d: "M12 16v3" }
                },
                ActionIcon::ExportHtml => rsx! {
                    path { d: "M7 4h7l3 3v13H7z" }
                    path { d: "M14 4v3h3" }
                    path { d: "M9 12h6" }
                    path { d: "M9 15h6" }
                    path { d: "M9 18h4" }
                },
                ActionIcon::ExportSql => rsx! {
                    path { d: "M7 4h7l3 3v13H7z" }
                    path { d: "M14 4v3h3" }
                    path { d: "M9 11l3 3-3 3" }
                    path { d: "M15 17h4" }
                },
                ActionIcon::ImportCsv => rsx! {
                    path { d: "M7 20h10" }
                    path { d: "M12 5v11" }
                    path { d: "m8.5 8.5 3.5-3.5 3.5 3.5" }
                    path { d: "M5 18h14" }
                },
                ActionIcon::InsertRow => rsx! {
                    rect { x: "4", y: "7", width: "16", height: "10", rx: "2" }
                    path { d: "M12 4v6" }
                    path { d: "M9 7h6" }
                    path { d: "M8 13h8" }
                },
                ActionIcon::Apply => rsx! {
                    path { d: "m5 13 4 4L19 7" }
                },
                ActionIcon::Undo => rsx! {
                    path { d: "M9 8H5v4" }
                    path { d: "M5 12c1.8-4.2 8.7-5.8 12.7-2.2 2.6 2.3 2.8 5.6 1.6 8.2" }
                },
                ActionIcon::Delete => rsx! {
                    path { d: "M4 7h16" }
                    path { d: "M9 7V5h6v2" }
                    path { d: "M8 7l.8 12h6.4L16 7" }
                    path { d: "M10 11v5" }
                    path { d: "M14 11v5" }
                },
                ActionIcon::Truncate => rsx! {
                    rect { x: "4", y: "5", width: "16", height: "9", rx: "2" }
                    path { d: "M4 9.5h16" }
                    path { d: "M8 18h8" }
                    path { d: "M10 14v4" }
                    path { d: "M14 14v4" }
                },
                ActionIcon::Duplicate => rsx! {
                    rect { x: "8", y: "8", width: "10", height: "10", rx: "2" }
                    path { d: "M6 15H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v1" }
                },
                ActionIcon::Details => rsx! {
                    rect { x: "4", y: "5", width: "16", height: "14", rx: "2" }
                    path { d: "M10 5v14" }
                    path { d: "M13 9h4" }
                    path { d: "M13 12h4" }
                    path { d: "M13 15h3" }
                },
                ActionIcon::AddRule => rsx! {
                    path { d: "M4 6h16" }
                    path { d: "M7 12h10" }
                    path { d: "M10 18h4" }
                    path { d: "M18 15v6" }
                    path { d: "M15 18h6" }
                },
                ActionIcon::Filter => rsx! {
                    path { d: "M4 6h16" }
                    path { d: "M7 6l5 6v5l-2 1v-6z" }
                    path { d: "M12 12l5-6" }
                },
                ActionIcon::FilterApply => rsx! {
                    path { d: "M4 6h16" }
                    path { d: "M7 12h10" }
                    path { d: "M10 18h4" }
                    path { d: "m16.5 18 1.8 1.8L21 17" }
                },
                ActionIcon::FilterClear => rsx! {
                    path { d: "M4 6h16" }
                    path { d: "M7 12h10" }
                    path { d: "M10 18h4" }
                    path { d: "m17 16 4 4" }
                    path { d: "m21 16-4 4" }
                },
                ActionIcon::Previous => rsx! {
                    path { d: "m15 6-6 6 6 6" }
                },
                ActionIcon::Next => rsx! {
                    path { d: "m9 6 6 6-6 6" }
                },
                ActionIcon::Close => rsx! {
                    path { d: "m4 4 16 16" }
                    path { d: "m20 4-16 16" }
                },
                ActionIcon::Output => rsx! {
                    rect { x: "4", y: "5", width: "16", height: "14", rx: "2" }
                    path { d: "M8 9h8" }
                    path { d: "M8 12h8" }
                    path { d: "M8 15h5" }
                },
                ActionIcon::Split => rsx! {
                    rect { x: "4", y: "5", width: "6.5", height: "14", rx: "1.6" }
                    rect { x: "13.5", y: "5", width: "6.5", height: "14", rx: "1.6" }
                    path { d: "M12 9v6" }
                },
                ActionIcon::Messages => rsx! {
                    path { d: "M4 6.5a2 2 0 0 1 2-2h12a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H10l-4 3.5v-3.5H6a2 2 0 0 1-2-2z" }
                    path { d: "M8 9.5h8" }
                    path { d: "M8 12.5h5" }
                },
                ActionIcon::QueryLog => rsx! {
                    rect { x: "4", y: "4", width: "16", height: "16", rx: "2" }
                    path { d: "M8 8h8" }
                    path { d: "M8 12h8" }
                    path { d: "M8 16h5" }
                    path { d: "M5 8h.01" }
                    path { d: "M5 12h.01" }
                    path { d: "M5 16h.01" }
                },
                ActionIcon::Transactions => rsx! {
                    path { d: "M4 7h16" }
                    path { d: "M4 12h16" }
                    path { d: "M4 17h16" }
                    path { d: "M8 5l2 2-2 2" }
                    path { d: "M16 15l-2 2 2 2" }
                },
                ActionIcon::Problems => rsx! {
                    path { d: "M12 4 2 20h20z" }
                    path { d: "M12 10v4" }
                    path { d: "M12 17h.01" }
                },
                ActionIcon::ViewMenu => rsx! {
                    // Three stacked panes representing the consolidated
                    // "View" overflow menu — visually echoes the
                    // sidebar / inspector / editor arrangement so the
                    // affordance reads as a layout control.
                    rect { x: "4", y: "5", width: "16", height: "4", rx: "1" }
                    rect { x: "4", y: "11", width: "10", height: "8", rx: "1" }
                    rect { x: "16", y: "11", width: "4", height: "8", rx: "1" }
                },
                ActionIcon::More => rsx! {
                    // Horizontal ellipsis — the standard "more actions"
                    // affordance for a toolbar overflow menu.
                    circle { cx: "5", cy: "12", r: "1.6", fill: "currentColor", stroke: "none" }
                    circle { cx: "12", cy: "12", r: "1.6", fill: "currentColor", stroke: "none" }
                    circle { cx: "19", cy: "12", r: "1.6", fill: "currentColor", stroke: "none" }
                },
            }
        }
    }
}
