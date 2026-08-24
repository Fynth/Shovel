use dioxus::prelude::*;
use models::ExplorerNodeKind;

/// Renders a small (~14px) stroke-currentColor SVG glyph that represents
/// a single object kind in the explorer tree. Replaces the legacy
/// letter-badge (`tree_badge()`) so the tree reads like a real IDE
/// navigator instead of a spreadsheet of single characters.
///
/// Each glyph is hand-tuned for a 24-unit viewBox so the stroke weight
/// stays consistent with [`crate::screens::workspace::components::IconGlyph`]
/// but at a smaller physical size — the explorer renders at `width: 14px`,
/// `height: 14px` via `.tree__object-icon`. The `aria-hidden` attribute
/// hides the SVG from the accessibility tree because the visible row
/// label and `display_label()` already name the object kind.
#[component]
pub fn ObjectIcon(kind: ExplorerNodeKind) -> Element {
    rsx! {
        svg {
            class: "tree__object-icon",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "1.85",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            "aria-hidden": "true",
            match kind {
                ExplorerNodeKind::Schema => rsx! {
                    path { d: "M4 6.5h6l2 2H20a1 1 0 0 1 1 1V19a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2z" }
                    path { d: "M4 11h17" }
                },
                ExplorerNodeKind::Table => rsx! {
                    rect { x: "4", y: "5", width: "16", height: "14", rx: "2" }
                    path { d: "M4 10h16" }
                    path { d: "M10 5v14" }
                },
                ExplorerNodeKind::View => rsx! {
                    path { d: "M2.5 12s3.5-6 9.5-6 9.5 6 9.5 6-3.5 6-9.5 6-9.5-6-9.5-6z" }
                    circle { cx: "12", cy: "12", r: "2.5" }
                },
                ExplorerNodeKind::MaterializedView => rsx! {
                    path { d: "M3 6.5C5 8 8.5 9 12 9s7-1 9-2.5" }
                    path { d: "M3 12c2 1.5 5.5 2.5 9 2.5s7-1 9-2.5" }
                    path { d: "M3 17.5c2 1.5 5.5 2.5 9 2.5s7-1 9-2.5" }
                },
                ExplorerNodeKind::Sequence => rsx! {
                    rect { x: "4", y: "9", width: "16", height: "6", rx: "1.5" }
                    path { d: "M8 6.5 6 9l2 2.5" }
                    path { d: "M16 17.5l2-2.5-2-2.5" }
                },
                ExplorerNodeKind::Function => rsx! {
                    path { d: "M9 4h7a4 4 0 0 1 4 4v0a4 4 0 0 1-4 4h-5l-2 4v-4H7a4 4 0 0 1-4-4v0a4 4 0 0 1 4-4z" }
                    path { d: "M16 16v4" }
                    path { d: "M14 18h4" }
                },
                ExplorerNodeKind::Procedure => rsx! {
                    path { d: "M5 5h10l4 4v10a1 1 0 0 1-1 1H5z" }
                    path { d: "M15 5v4h4" }
                    path { d: "M9 14h6" }
                    path { d: "M12 11v6" }
                },
                ExplorerNodeKind::Trigger => rsx! {
                    path { d: "M12 3v4" }
                    path { d: "M12 17v4" }
                    path { d: "M3 12h4" }
                    path { d: "M17 12h4" }
                    path { d: "m5.5 5.5 2.8 2.8" }
                    path { d: "m15.7 15.7 2.8 2.8" }
                    circle { cx: "12", cy: "12", r: "3" }
                },
                ExplorerNodeKind::Column => rsx! {
                    rect { x: "5", y: "4", width: "14", height: "16", rx: "2" }
                    path { d: "M9 8h6" }
                    path { d: "M9 12h6" }
                    path { d: "M9 16h4" }
                },
            }
        }
    }
}
