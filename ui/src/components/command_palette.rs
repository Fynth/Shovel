//! Renders the global command-palette overlay.
//!
//! Mounted once at the top of [`crate::app::App`] next to
//! [`crate::components::context_menu::ContextMenu`]. The palette
//! reads its visibility from
//! [`crate::app_state::APP_COMMAND_PALETTE`] and the catalog from
//! [`crate::app_state::commands::command_list`]. When the user
//! presses Enter on a result, the palette invokes the matching
//! runner via
//! [`crate::app_state::commands::dispatch`] and closes itself.
//!
//! Behaviour:
//! - Search input is auto-focused when the palette opens; the
//!   underlying `use_signal`s for the query and the selection index
//!   are reset so the palette always opens in a fresh state.
//! - Up/Down arrows navigate; Home/End jump to the first/last
//!   result; Enter runs; Escape closes.
//! - Filtering is a case-insensitive substring match over the title
//!   and the keywords. When the query is empty the catalog is
//!   grouped by category (File / View / Query / Help) for a clean
//!   "browse" feel.
//! - The palette calls `prevent_default` on its own key events so
//!   the workspace's root onkeydown matcher does not double-fire
//!   when the user is typing inside the search field.

use crate::{
    app_state::{
        APP_COMMAND_PALETTE,
        close_command_palette,
        commands::{self, Command, CommandId, command_list},
    },
    screens::workspace::components::IconGlyph,
};
use dioxus::prelude::*;

const MAX_VISIBLE_RESULTS: usize = 50;

/// Render a list of filter hits, preserving the original command
/// order. The query is matched case-insensitively against title +
/// keywords. Returns `None` to mean "include all", `Some(vec)` to
/// mean "use these".
fn filter_commands(query: &str, list: &[Command]) -> Vec<usize> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return (0..list.len()).collect();
    }
    let needle = needle.as_str();
    list.iter()
        .enumerate()
        .filter(|(_, cmd)| {
            if cmd.title.to_lowercase().contains(needle) {
                return true;
            }
            cmd.keywords
                .iter()
                .any(|kw| kw.to_lowercase().contains(needle))
        })
        .map(|(idx, _)| idx)
        .collect()
}

/// Highlight the matching substring inside `title`. Returns a
/// sequence of `(text, matched)` pairs so the renderer can wrap
/// the matched fragment in a styled span.
fn highlight_segments(title: &str, needle: &str) -> Vec<(String, bool)> {
    if needle.is_empty() {
        return vec![(title.to_string(), false)];
    }
    let lower_title = title.to_lowercase();
    let lower_needle = needle.to_lowercase();
    let mut segments: Vec<(String, bool)> = Vec::new();
    let mut cursor = 0usize;
    while let Some(rel) = lower_title[cursor..].find(&lower_needle) {
        let start = cursor + rel;
        let end = start + lower_needle.len();
        if start > cursor {
            segments.push((title[cursor..start].to_string(), false));
        }
        segments.push((title[start..end].to_string(), true));
        cursor = end;
    }
    if cursor < title.len() {
        segments.push((title[cursor..].to_string(), false));
    }
    if segments.is_empty() {
        segments.push((title.to_string(), false));
    }
    segments
}

#[component]
pub fn CommandPalette() -> Element {
    let open = APP_COMMAND_PALETTE();
    if !open {
        return rsx! {};
    }

    let mut query = use_signal(String::new);
    let mut selection: Signal<usize> = use_signal(|| 0usize);
    let mut filtered_indices: Signal<Vec<usize>> = use_signal(Vec::new);

    // Touch the visibility signal so this effect re-runs when the
    // palette is (re-)opened from outside — we use that to reset
    // the query, the selection index, and the filtered list.
    // Without this, opening the palette via Ctrl+Shift+P after a
    // previous query would keep the stale query and selection.
    use_effect(move || {
        let _ = APP_COMMAND_PALETTE();
        query.set(String::new());
        selection.set(0);
        filtered_indices.set(Vec::new());
    });

    // Recompute the filtered list whenever the query changes. We
    // store the indices as a Signal so the onkeydown closure can
    // borrow them across moves without each render rebuilding the
    // Vec (and without triggering the "borrow of moved value"
    // error that bit the inline-Vec variant).
    use_effect(move || {
        let catalog = command_list();
        let next: Vec<usize> = filter_commands(&query(), catalog)
            .into_iter()
            .take(MAX_VISIBLE_RESULTS)
            .collect();
        filtered_indices.set(next);
    });

    // Clamp the selection when the filter list shrinks so a stale
    // index never points past the end.
    {
        let len = filtered_indices().len();
        let cur = selection();
        if len == 0 {
            if cur != 0 {
                selection.set(0);
            }
        } else if cur >= len {
            selection.set(len - 1);
        }
    }

    let run_and_close = move |id: CommandId| {
        commands::dispatch(id);
        close_command_palette();
    };

    let onkeydown = move |event: KeyboardEvent| {
        use dioxus::prelude::Modifiers;
        let key = event.key();
        let mods = event.modifiers();
        let ctrl = mods.contains(Modifiers::CONTROL) || mods.contains(Modifiers::META);
        let filtered_len = filtered_indices().len();
        match key {
            Key::Escape => {
                event.prevent_default();
                close_command_palette();
            }
            Key::ArrowDown => {
                event.prevent_default();
                if filtered_len > 0 {
                    let next = (selection() + 1) % filtered_len;
                    selection.set(next);
                }
            }
            Key::ArrowUp => {
                event.prevent_default();
                if filtered_len > 0 {
                    let prev = if selection() == 0 {
                        filtered_len - 1
                    } else {
                        selection() - 1
                    };
                    selection.set(prev);
                }
            }
            Key::Home => {
                event.prevent_default();
                if filtered_len > 0 {
                    selection.set(0);
                }
            }
            Key::End => {
                event.prevent_default();
                if filtered_len > 0 {
                    selection.set(filtered_len - 1);
                }
            }
            Key::Enter => {
                event.prevent_default();
                let indices = filtered_indices();
                if let Some(&idx) = indices.get(selection()) {
                    let id = command_list()[idx].id;
                    run_and_close(id);
                }
            }
            Key::PageDown => {
                event.prevent_default();
                if filtered_len > 0 {
                    let step = 8usize;
                    let next = (selection() + step).min(filtered_len - 1);
                    selection.set(next);
                }
            }
            Key::PageUp => {
                event.prevent_default();
                if filtered_len > 0 {
                    let step = 8usize;
                    let next = selection().saturating_sub(step);
                    selection.set(next);
                }
            }
            _ => {
                let _ = ctrl;
            }
        }
    };

    rsx! {
        div {
            class: "command-palette__backdrop",
            onclick: move |_| close_command_palette(),
            div {
                class: "command-palette",
                onclick: move |event| event.stop_propagation(),
                onkeydown: onkeydown,
                tabindex: "0",
                div {
                    class: "command-palette__input-row",
                    span { class: "command-palette__icon",
                        svg {
                            view_box: "0 0 24 24",
                            fill: "none",
                            stroke: "currentColor",
                            stroke_width: "1.85",
                            stroke_linecap: "round",
                            stroke_linejoin: "round",
                            width: "16",
                            height: "16",
                            circle { cx: "11", cy: "11", r: "6" }
                            path { d: "m20 20-3.5-3.5" }
                        }
                    }
                    input {
                        class: "command-palette__input",
                        r#type: "text",
                        placeholder: "Type a command or search...",
                        value: "{query}",
                        autofocus: true,
                        autocomplete: "off",
                        spellcheck: "false",
                        oninput: move |event| {
                            query.set(event.value());
                            selection.set(0);
                        },
                    }
                    span { class: "command-palette__shortcut-hint", "Esc" }
                }
                div {
                    class: "command-palette__results",
                    {
                        let indices = filtered_indices();
                        let catalog = command_list();
                        let query_trimmed = query().trim().to_string();
                        if indices.is_empty() {
                            rsx! {
                                div {
                                    class: "command-palette__empty",
                                    "No matching commands"
                                }
                            }
                        } else if query_trimmed.is_empty() {
                            // Empty query: browse by category.
                            rsx! {
                                for category in ["File", "View", "Query", "Help"] {
                                    {
                                        let in_cat: Vec<usize> = indices
                                            .iter()
                                            .copied()
                                            .filter(|&i| catalog[i].category == category)
                                            .collect();
                                        if in_cat.is_empty() {
                                            rsx! {}
                                        } else {
                                            rsx! {
                                                div {
                                                    class: "command-palette__group",
                                                    div { class: "command-palette__group-title", {category} }
                                                    for orig_idx in in_cat.iter().copied() {
                                                        {
                                                            let cmd = &catalog[orig_idx];
                                                            let id = cmd.id;
                                                            let title = cmd.title;
                                                            let is_selected = selection() == orig_idx;
                                                            let mut class_name = String::from("command-palette__item");
                                                            if is_selected {
                                                                class_name.push_str(" command-palette__item--selected");
                                                            }
                                                            rsx! {
                                                                button {
                                                                    class: class_name.clone(),
                                                                    r#type: "button",
                                                                    onclick: move |_| run_and_close(id),
                                                                    onmouseenter: move |_| selection.set(orig_idx),
                                                                    if let Some(icon) = cmd.icon {
                                                                        span { class: "command-palette__item-icon",
                                                                            svg {
                                                                                view_box: "0 0 24 24",
                                                                                fill: "none",
                                                                                stroke: "currentColor",
                                                                                stroke_width: "1.85",
                                                                                stroke_linecap: "round",
                                                                                stroke_linejoin: "round",
                                                                                width: "14",
                                                                                height: "14",
                                                                                IconGlyph { icon }
                                                                            }
                                                                        }
                                                                    } else {
                                                                        span { class: "command-palette__item-icon command-palette__item-icon--placeholder" }
                                                                    }
                                                                    span { class: "command-palette__item-title", {title} }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            rsx! {
                                div {
                                    class: "command-palette__list",
                                    for (display_idx, orig_idx) in indices.iter().copied().enumerate() {
                                        {
                                            let cmd = &catalog[orig_idx];
                                            let id = cmd.id;
                                            let title = cmd.title;
                                            let category = cmd.category;
                                            let is_selected = selection() == display_idx;
                                            let needle = query_trimmed.clone();
                                            let mut class_name = String::from("command-palette__item");
                                            if is_selected {
                                                class_name.push_str(" command-palette__item--selected");
                                            }
                                            let segments = highlight_segments(title, &needle);
                                            rsx! {
                                                button {
                                                    class: class_name.clone(),
                                                    r#type: "button",
                                                    onclick: move |_| run_and_close(id),
                                                    onmouseenter: move |_| selection.set(display_idx),
                                                    if let Some(icon) = cmd.icon {
                                                        span { class: "command-palette__item-icon",
                                                            svg {
                                                                view_box: "0 0 24 24",
                                                                fill: "none",
                                                                stroke: "currentColor",
                                                                stroke_width: "1.85",
                                                                stroke_linecap: "round",
                                                                stroke_linejoin: "round",
                                                                width: "14",
                                                                height: "14",
                                                                IconGlyph { icon }
                                                            }
                                                        }
                                                    } else {
                                                        span { class: "command-palette__item-icon command-palette__item-icon--placeholder" }
                                                    }
                                                    span { class: "command-palette__item-title",
                                                        for segment in segments.iter() {
                                                            if segment.1 {
                                                                span { class: "command-palette__match", {segment.0.clone()} }
                                                            } else {
                                                                span { {segment.0.clone()} }
                                                            }
                                                        }
                                                    }
                                                    span { class: "command-palette__item-category", {category} }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                div {
                    class: "command-palette__footer",
                    span { class: "command-palette__hint",
                        span { class: "command-palette__kbd", "↑↓" }
                        " navigate"
                        span { class: "command-palette__kbd-sep", " · " }
                        span { class: "command-palette__kbd", "Enter" }
                        " run"
                        span { class: "command-palette__kbd-sep", " · " }
                        span { class: "command-palette__kbd", "Esc" }
                        " close"
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cmd(id: u64, title: &'static str, keywords: &'static [&'static str]) -> Command {
        Command {
            id: CommandId(id),
            title,
            keywords,
            category: "Test",
            icon: None,
        }
    }

    #[test]
    fn filter_empty_query_returns_all_in_order() {
        let list = vec![
            make_cmd(1, "Run Query", &["run"]),
            make_cmd(2, "Open Settings", &["config"]),
        ];
        let out = filter_commands("", &list);
        assert_eq!(out, vec![0, 1]);
    }

    #[test]
    fn filter_matches_title_case_insensitively() {
        let list = vec![
            make_cmd(1, "Run Query", &[]),
            make_cmd(2, "Open Settings", &[]),
        ];
        let out = filter_commands("run", &list);
        assert_eq!(out, vec![0]);
        let out = filter_commands("QUERY", &list);
        assert_eq!(out, vec![0]);
    }

    #[test]
    fn filter_matches_keywords() {
        let list = vec![
            make_cmd(1, "Refresh Explorer", &["reload", "tree"]),
            make_cmd(2, "New Connection", &["add"]),
        ];
        let out = filter_commands("reload", &list);
        assert_eq!(out, vec![0]);
        let out = filter_commands("add", &list);
        assert_eq!(out, vec![1]);
    }

    #[test]
    fn filter_returns_empty_when_no_match() {
        let list = vec![make_cmd(1, "Run Query", &[])];
        let out = filter_commands("xyz", &list);
        assert!(out.is_empty());
    }

    #[test]
    fn highlight_segments_preserves_text() {
        let segs = highlight_segments("Run Query", "run");
        let joined: String = segs.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(joined, "Run Query");
        // The matched span is the first segment.
        assert!(segs[0].1);
        assert_eq!(segs[0].0, "Run");
    }

    #[test]
    fn highlight_segments_empty_needle_returns_full_text() {
        let segs = highlight_segments("Run Query", "");
        assert_eq!(segs.len(), 1);
        assert!(!segs[0].1);
        assert_eq!(segs[0].0, "Run Query");
    }

    #[test]
    fn highlight_segments_no_match_returns_plain_text() {
        let segs = highlight_segments("Run Query", "zzz");
        let joined: String = segs.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(joined, "Run Query");
        assert!(segs.iter().all(|(_, m)| !*m));
    }
}
