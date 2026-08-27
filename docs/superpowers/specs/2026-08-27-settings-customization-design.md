# Settings chrome, widgets, and shortcut editor — design

**Date:** 2026-08-27
**Status:** Draft (awaiting user review)
**Path:** Architectural (settings window restyle + persist new fields on `AppUiSettings` + live wiring)

## Problem

The native settings window is created at 760×640. The stylesheet switches to a
narrow layout at `max-width: 760px`, so the window always collapses: the left
category list becomes a wrapping two-column card grid, and Dark/Light/density
become stacked full-width pills. Inner cards still use 18px glass radii while
the rest of the app is being densified to Zed-like chrome (`$radius-sm` /
`$radius-md`, hairline borders, flat fills).

Three categories (Database, Grid, Navigation) render `CategoryEmptyState`.
Explorer, page size, and panel visibility already exist, but they sit in
Advanced. `config.toml` already defines `ThemeOverrides`, `EditorBehavior`,
`AppBehavior`, and `KeybindingMap`. Those load into globals at startup.
`ThemeOverrides` and custom keybindings are applied. `EditorBehavior` and
`AppBehavior` are never read by the SQL editor, result grid, or explorer.
Ollama settings exist on `AppUiSettings` and have no GUI.

## Goal

One settings window that matches the rest of the app, has no empty categories,
exposes key-capture / color / font / slider widgets, and lets the user edit
keyboard shortcuts. Every control persists on `AppUiSettings` and changes the
live app. `config.toml` stays an overlay, not the GUI source of truth.

Out of scope: a settings page per workspace surface (chart, ER, blob, value
editor), a VS Code-style JSON keybinding studio, a token studio for every CSS
variable, writing `config.toml` back from the GUI.

## Approach

Chosen: **in-theme widgets + `AppUiSettings` as the GUI store**.

Rejected:

- Browser-native-only controls (`input type=color` with no hex, text-only
  shortcut fields). Too easy to mistype, looks foreign next to the toolbar.
- Full theme/keybinding studio. Separate product.

## Persistence

All GUI-editable customization lives on `AppUiSettings` (`app_ui_settings.json`).
`SettingsSnapshot { ui, sql }` is unchanged. `replace_ui_settings` /
`sync_runtime_ui_settings` also copy the new nested structs onto the existing
globals (`APP_THEME_OVERRIDES`, `APP_KEYBINDINGS`, `APP_EDITOR_BEHAVIOR`,
`APP_APP_BEHAVIOR`) so the settings window bridge keeps working.

`config.toml` remains an overlay applied at startup and on Config → Reload.
If a toml field is set, it wins over JSON for that launch. The GUI does not
rewrite `config.toml`.

Legacy JSON without the new fields deserializes via `#[serde(default)]` to the
defaults below. API keys stay in the keyring. Reset UI restores `AppUiSettings`
defaults but keeps DeepSeek / CodeStral / Ollama API keys and does **not**
clear keybinding overrides (Keyboard has its own Reset all).

### New / promoted fields on `AppUiSettings`

```rust
pub struct EditorSettings {
    pub font_size: u32,          // default 13, clamp 10..=22
    pub tab_size: u32,           // default 2, clamp 1..=8
    pub auto_format_on_run: bool, // default false
    pub word_wrap: bool,          // default false
    pub show_line_numbers: bool,  // default true
}

pub enum NullDisplay {
    Literal,  // the text "NULL"
    Empty,
    EmDash,   // "—"
}

pub struct GridSettings {
    pub row_height: u32,       // default 28, clamp 18..=48
    pub zebra: bool,           // default false (current grid has hover, not stripes)
    pub null_display: NullDisplay, // default Literal
    pub wrap_cells: bool,      // default false
}

pub struct AppBehaviorSettings {
    pub confirm_before_drop: bool,      // default true (today always confirms)
    pub confirm_before_truncate: bool,  // default true
}

// Already exist; promoted onto AppUiSettings so the GUI can write them:
// theme_overrides: ThemeOverrides
// keybindings: KeybindingMap  // HashMap<String, String>, empty = all defaults
// editor: EditorSettings      // concrete, not Option-fields EditorBehavior
// grid: GridSettings
// behavior: AppBehaviorSettings
```

`models::EditorBehavior` (Option fields) stays as the toml-shaped overlay.
`ShovelConfig::apply_to` copies `Some` toml values onto the concrete
`EditorSettings` / `GridSettings` / `AppBehaviorSettings` / `ThemeOverrides` /
`KeybindingMap` on `AppUiSettings`.

`ThemeOverrides::to_css` must emit the variable names the live stylesheet
actually consumes, not the unused names it emits today:

| Field | CSS variable |
|---|---|
| `primary` | `--color-primary` |
| derived if hover/active unset | `--color-primary-hover`, `--color-primary-active` |
| `font_family` | `--font-sans` (not `--font-family-sans`) |
| `font_family_mono` | `--font-mono` |
| `font_size` | `--ui-font-size` |
| `font_size_small` | `--ui-font-size-sm` |
| `radius_small` / `medium` / `large` | `--radius-sm` / `--radius-md` / `--radius-lg` |

`.app` (and `.settings-window-shell`) define fallbacks for `--radius-*` and
`--font-sans` / `--font-mono` from the SCSS tokens. Buttons, inputs, settings
modal, command palette, and workspace panels use `var(--radius-sm, #{$radius-sm})`
and friends. Not every `border-radius` in the repo.

The injected `<style>{theme_css}</style>` stays after the compiled stylesheet
so overrides win over density.

## Window and chrome

Native settings window: inner size **960×720**, min **720×520**, still
resizable. Title stays `Shovel Settings`.

Narrow breakpoint: **max-width: 560px** (not 760). Below that, nav stacks
above content as a wrapping row of compact label-only chips. Segmented
controls stay a single row until 560px, then wrap.

Native window shell (`.settings-window-shell`):

- Fill the webview. No dimmed backdrop padding.
- `.settings-modal` is 100% width/height, `border-radius: 0`, no card shadow.
- Apply `theme-dark`/`theme-light` **and** the density class.

Settings currently open only as this native window (`open_settings_window` from
the toolbar). Overlay styles on `.settings-modal__backdrop` stay for that
shell; they must not add inset padding or a second card inside the OS window.

Left nav: 168px, label only, no two-line descriptions. Active item uses the
same fill as `.button--active` (`--color-active`, `$radius-sm`). Hover uses
`--color-hover`.

Right pane: scrollable sections. Section cards use `$radius-md`, hairline
`--color-border`, `var(--color-surface-elevated)`, no glass/18px radii.

Segmented controls: one pill row, equal columns, height 24px like `.button`.
Density is three columns, not a 2-column grid that wraps Comfortable.

No in-window title row. The OS window title is `Shovel Settings`; Wayland
already draws compositor chrome. A second heading would duplicate it.

## Categories

Order (nav + `SettingsCategory::ALL`):

1. Appearance
2. Database
3. Editor
4. Grid
5. Navigation
6. Keyboard
7. Advanced
8. Config

### Appearance

- Theme: Dark | Light
- Density: Compact | Normal | Comfortable
- Accent: color widget → `theme_overrides.primary` (hover/active derived)
- UI font: short list (System UI, IBM Plex Sans, Segoe UI) → `font_family`
- Editor font: short list (JetBrains Mono, SF Mono, Cascadia Code, ui-monospace)
  → `font_family_mono`
- UI font size slider 10–16 → `font_size` (also sets `font_size_small` to size-1)
- Radius slider 0–12 → writes `radius_small = n`, `radius_medium = n+2`,
  `radius_large = n+4`

Invalid hex is ignored; the previous value stays.

### Database

- Default page size (10–1000, existing field)
- Restore session on launch
- Read-only mode
- Confirm before drop
- Confirm before truncate

### Editor

- Existing SQL format section + Reset SQL
- Font size, tab size, word wrap, line numbers, auto-format on run
- Existing CodeStral section

### Grid

- Row height slider 18–48
- Zebra stripes
- Null display segmented: `NULL` | empty | `—`
- Wrap cells

### Navigation

- Explorer view toggles (moved from Advanced)
- Visible panels by default (moved from Advanced)
- Editor/result split mode (moved from Advanced)
- Show bottom dock

### Keyboard

Table of bindable actions only. Bindable means `action_from_id` already maps
the id to a `ShortcutAction` the workspace dispatcher handles:

| id | default combo |
|---|---|
| `focus_editor` | Ctrl+E |
| `format_sql` | Ctrl+Shift+F |
| `new_tab` | Ctrl+T |
| `close_tab` | Ctrl+W |
| `next_tab` | Ctrl+Tab |
| `refresh_explorer` | F5 |
| `focus_filter_panel` | Ctrl+F |
| `save_query` | Ctrl+Shift+S |
| `close_overlay` | Escape |
| `command_palette` | Ctrl+Shift+P |
| `global_search` | Ctrl+K |
| `rename_selected` | F2 |
| `delete_selected` | Delete |
| `focus_agent_composer` | Ctrl+Shift+M |
| `new_connection` | Ctrl+Shift+N |
| `open_settings` | Ctrl+, |

One combo per action. Built-in `Ctrl+N` as an alias of New Tab is not a second
row; customizing `new_tab` replaces `Ctrl+T` only. `Ctrl+N` remains the
hard-coded alias unless we later add a dedicated id.

Effective map = defaults with `settings.keybindings` overrides applied.

Key capture widget: click the combo cell → listening. Next `keydown` that is
not a lone modifier is parsed with existing `parse_combo` (after building
`Ctrl+Shift+F` from the event). Escape while listening cancels. Backspace
while listening removes the override (row returns to default).

Conflict: if the new combo equals another action's **effective** combo, do not
write. The row shows an error: `already used by {label}`. No silent steal.

Filter input matches action labels. Reset row / Reset all shortcuts only
clears `keybindings`.

Custom bindings already win in `match_custom_keybinding` before
`match_key_combination`. GUI only writes the map.

### Advanced

- DeepSeek (existing)
- Ollama: enabled, API key (password field, same keyring pattern as DeepSeek),
  base URL, model
- AI features enable, response language, auto-apply completions
- Reset UI

### Config

Path to `config.toml`, Open config folder, Reload config. No second shortcut
table.

## Widgets

Shared controls live in `ui/src/layout/settings_modal/widgets.rs`:

- `ColorField` — swatch (`input type=color`) + hex text, both write the same
  string. Hex must match `#RGB` or `#RRGGBB` (case-insensitive) or the edit is
  dropped.
- `FontSelect` — `<select>` of the lists above, selected option rendered in
  that font.
- `SliderField` — range + numeric input, shared min/max/step, clamp on both.
- `KeyCapture` — button showing the current combo or `Press keys…` while
  listening. Uses `onkeydown` on the button; `prevent_default` while listening
  so Tab/arrows do not move focus.

Section helpers stay prop-driven: `(settings, sql_settings, on_change)`. No
new globals inside the modal. Listening state is local to `KeyCapture`.

## Live wiring

| Setting | Consumer |
|---|---|
| Theme / density | existing `APP_THEME` / `APP_UI_DENSITY` |
| ThemeOverrides CSS | existing `<style>` in `App`; also inject in `SettingsWindowRoot` so the dialog previews accent/fonts |
| Editor font size / wrap / tab size | SQL editor textarea + highlight overlay: `font-size`, `white-space`, `tab-size` |
| Line numbers | a numeric gutter beside the textarea, hidden when off. No change to highlight tokenizer |
| Auto-format on run | `run_query_for_tab` (and batch) formats with current `SqlFormatSettings` before execute when the flag is on |
| Grid row height | replace the hard-coded `virtual_row_height: f64 = 28.0` in `result_table.rs` |
| Zebra / wrap / null | result grid cell/row classes and null renderer |
| Confirm drop/truncate | `confirm_and_drop_table` / `confirm_and_truncate_table` skip the Yes/No dialog when the matching flag is false, then run the mutation |
| Keybindings | existing `match_custom_keybinding` |

`sync_runtime_ui_settings` copies nested structs to globals with equality
guards (same pattern as density/split_mode) so unrelated toggles do not
invalidate the editor/grid.

## File split

`ui/src/layout/settings_modal.rs` is already ~1300 lines. Move to:

- `ui/src/layout/settings_modal/mod.rs` — shell, nav, category routing, tests
  for category order
- `ui/src/layout/settings_modal/widgets.rs` — ColorField, FontSelect,
  SliderField, KeyCapture
- `ui/src/layout/settings_modal/keyboard.rs` — Keyboard section + catalog
  table
- `ui/src/layout/settings_modal/sections.rs` — Appearance, Database, Editor,
  Grid, Navigation, Advanced, Config

Public `SettingsModal` export path stays `crate::layout::SettingsModal`.

Pure helpers (hex parse, combo conflict, default catalog, clamp) get unit
tests next to them. Conflict helper is `fn combo_conflict(action_id, combo,
effective: &HashMap<String, String>) -> Option<String>`.

## Error handling

- Invalid number/hex: keep previous persisted value, do not toast.
- Shortcut conflict: inline error on the row, no write, no toast.
- Keyring save failures: existing toast path in `app.rs`.
- Reload config parse error: toast the existing `ShovelConfig::load` error
  string.
- Settings window close: last bridged snapshot is already on main-window
  globals; nothing extra to flush.

## Testing

- `AppUiSettings` serde: missing new fields default as specified; round-trip
  preserves them; API keys still skipped.
- `ThemeOverrides::to_css` emits `--font-sans`, `--ui-font-size`,
  `--radius-sm`, derived hover/active when only primary is set.
- `combo_conflict` detects clash against effective map, allows replacing the
  same action's own combo.
- `parse_combo` already tested; add `Ctrl+,` and `F2`.
- `SettingsCategory::ALL` order includes Keyboard between Navigation and
  Advanced.
- `confirm_and_*` skip dialog when flags are false: extract a pure
  `fn should_prompt_table_mutation(kind, behavior) -> bool` and test it.
  Do not try to drive `rfd` in unit tests.

CI still: `cargo fmt --all -- --check`, `clippy -D warnings`,
`cargo test --workspace`.

## Non-goals (explicit)

- Per-surface settings pages (chart, ER, blob, value editor).
- Recording chords that are not representable by `parse_combo`.
- Editing explorer-only context actions that have no `ShortcutAction`.
- Writing `config.toml` from the GUI.
- Migrating every `border-radius: $radius-*` in the repo.

## Rollout

Single change set. No feature flag. Defaults match current behaviour except
line numbers default on (visible gutter) and confirm-before-drop/truncate
default on (same as today's always-confirm dialogs).
