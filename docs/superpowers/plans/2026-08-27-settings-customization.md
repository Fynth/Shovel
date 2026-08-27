# Settings Chrome, Widgets, and Shortcut Editor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restyle the native settings window to match the densified app chrome, fill every category, add color/font/slider/key-capture widgets plus a shortcut editor, persist it all on `AppUiSettings`, and wire those fields into the live editor, grid, and drop/truncate confirms.

**Architecture:** GUI state stays on `AppUiSettings` and rides the existing `SettingsSnapshot` bridge. New nested structs (`EditorSettings`, `GridSettings`, `AppBehaviorSettings`) plus promoted `ThemeOverrides` and `KeybindingMap` are copied onto globals in `sync_runtime_ui_settings`. `config.toml` remains a launch/reload overlay via `ShovelConfig::apply_to`. Settings UI splits from the 1300-line `settings_modal.rs` into a module.

**Tech Stack:** Rust nightly (workspace pin), Dioxus 0.7, serde, grass SCSS → `app/assets/app.css` via `app/build.rs`.

**Spec:** `docs/superpowers/specs/2026-08-27-settings-customization-design.md`

## Global Constraints

- Dioxus 0.7 APIs only (`use_signal`, `use_effect`, `#[component]`). No `cx`/`Scope`/`use_state`.
- Never hold a signal read/write across an `.await` point.
- `ui` may import `models` and `services` only. New types live in `models`.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- `rustfmt.toml`: `max_width = 100`, `imports_granularity = "Crate"`, `reorder_modules = false`.
- Do not revert the already-dirty density/theme SCSS in the working tree. Only change settings chrome and the radius/font CSS variables listed in each task.
- Do not write `config.toml` from the GUI.
- Do not add chart/ER/blob/value-editor settings pages.
- API keys stay `#[serde(skip_serializing)]` and in the keyring.

## File structure

- Create: `ui/src/layout/settings_modal/mod.rs`
- Create: `ui/src/layout/settings_modal/widgets.rs`
- Create: `ui/src/layout/settings_modal/keyboard.rs`
- Create: `ui/src/layout/settings_modal/sections.rs`
- Delete: `ui/src/layout/settings_modal.rs` (after the move)
- Modify: `models/src/settings.rs` — new nested structs on `AppUiSettings`
- Modify: `models/src/customization.rs` — CSS variable names, hex parse, keybinding helpers
- Modify: `models/src/config.rs` — `apply_to` for the new fields
- Modify: `ui/src/app_state/mod.rs` — `sync_runtime_ui_settings`
- Modify: `ui/src/app_state/keyboard.rs` — `combo_from_event`, extra `parse_combo` tests
- Modify: `ui/src/windows/mod.rs` — window size, density class, theme CSS inject
- Modify: `styles/components/_settings-modal.scss` — sidebar layout, 560px breakpoint
- Modify: `styles/layout/_app-layout.scss`, `_buttons.scss`, `_inputs.scss` — `var(--radius-*)` / `--font-sans`
- Modify: `ui/src/screens/workspace/components/sql_editor.rs` — font/wrap/gutter
- Modify: `ui/src/screens/workspace/components/result_table.rs` — row height/zebra/null/wrap
- Modify: `ui/src/screens/workspace/actions.rs` — auto-format on run
- Modify: `ui/src/screens/workspace/components/explorer/tree_views.rs` — confirm flags
- Modify: `services/src/app.rs` — hydrate/save Ollama key
- Modify: `storage/src/settings.rs` only if a dedicated Ollama helper is cleaner than `load_lm_api_key("shovel.ollama")`

---

### Task 1: Nested settings types on `AppUiSettings`

**Files:**
- Modify: `models/src/settings.rs` (append types before `AppUiSettings`, add fields, defaults, tests)

**Interfaces:**
- Consumes: nothing new.
- Produces:
  - `pub struct EditorSettings { pub font_size: u32, pub tab_size: u32, pub auto_format_on_run: bool, pub word_wrap: bool, pub show_line_numbers: bool }`
  - `pub enum NullDisplay { Literal, Empty, EmDash }`
  - `pub struct GridSettings { pub row_height: u32, pub zebra: bool, pub null_display: NullDisplay, pub wrap_cells: bool }`
  - `pub struct AppBehaviorSettings { pub confirm_before_drop: bool, pub confirm_before_truncate: bool }`
  - `AppUiSettings.theme_overrides: ThemeOverrides`
  - `AppUiSettings.keybindings: KeybindingMap`
  - `AppUiSettings.editor: EditorSettings`
  - `AppUiSettings.grid: GridSettings`
  - `AppUiSettings.behavior: AppBehaviorSettings`

- [ ] **Step 1: Write the failing test**

Add to the existing `#[cfg(test)] mod tests` in `models/src/settings.rs`:

```rust
#[test]
fn fresh_defaults_match_settings_spec() {
    let s = AppUiSettings::default();
    assert_eq!(s.editor.font_size, 13);
    assert_eq!(s.editor.tab_size, 2);
    assert!(!s.editor.auto_format_on_run);
    assert!(!s.editor.word_wrap);
    assert!(s.editor.show_line_numbers);
    assert_eq!(s.grid.row_height, 28);
    assert!(!s.grid.zebra);
    assert_eq!(s.grid.null_display, NullDisplay::Literal);
    assert!(!s.grid.wrap_cells);
    assert!(s.behavior.confirm_before_drop);
    assert!(s.behavior.confirm_before_truncate);
    assert!(s.keybindings.is_empty());
    assert_eq!(s.theme_overrides, ThemeOverrides::default());
}

#[test]
fn legacy_json_without_new_fields_gets_spec_defaults() {
    let settings: AppUiSettings = serde_json::from_str(r#"{"theme":"Dark"}"#)
        .expect("legacy settings should deserialize");
    assert_eq!(settings.editor.font_size, 13);
    assert!(settings.editor.show_line_numbers);
    assert_eq!(settings.grid.row_height, 28);
    assert!(settings.behavior.confirm_before_drop);
    assert!(settings.keybindings.is_empty());
}

#[test]
fn new_settings_fields_round_trip() {
    let mut settings = AppUiSettings::default();
    settings.editor.font_size = 16;
    settings.editor.word_wrap = true;
    settings.grid.zebra = true;
    settings.grid.null_display = NullDisplay::EmDash;
    settings.behavior.confirm_before_drop = false;
    settings.keybindings.insert("format_sql".into(), "Ctrl+Alt+F".into());
    settings.theme_overrides.primary = Some("#ff8800".into());
    let json = serde_json::to_string(&settings).unwrap();
    let back: AppUiSettings = serde_json::from_str(&json).unwrap();
    assert_eq!(back.editor.font_size, 16);
    assert!(back.editor.word_wrap);
    assert!(back.grid.zebra);
    assert_eq!(back.grid.null_display, NullDisplay::EmDash);
    assert!(!back.behavior.confirm_before_drop);
    assert_eq!(back.keybindings.get("format_sql").map(String::as_str), Some("Ctrl+Alt+F"));
    assert_eq!(back.theme_overrides.primary.as_deref(), Some("#ff8800"));
}
```

Import `NullDisplay`, `ThemeOverrides` at the top of the test module.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p models fresh_defaults_match_settings_spec -- --nocapture`

Expected: FAIL — `AppUiSettings` has no `editor`/`grid`/`behavior` fields.

- [ ] **Step 3: Write minimal implementation**

In `models/src/settings.rs`, add (with `#[serde(default)]` on structs):

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorSettings {
    pub font_size: u32,
    pub tab_size: u32,
    pub auto_format_on_run: bool,
    pub word_wrap: bool,
    pub show_line_numbers: bool,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            font_size: 13,
            tab_size: 2,
            auto_format_on_run: false,
            word_wrap: false,
            show_line_numbers: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NullDisplay {
    #[default]
    Literal,
    Empty,
    EmDash,
}

impl NullDisplay {
    pub const ALL: [Self; 3] = [Self::Literal, Self::Empty, Self::EmDash];

    pub fn label(self) -> &'static str {
        match self {
            Self::Literal => "NULL",
            Self::Empty => "Empty",
            Self::EmDash => "—",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GridSettings {
    pub row_height: u32,
    pub zebra: bool,
    pub null_display: NullDisplay,
    pub wrap_cells: bool,
}

impl Default for GridSettings {
    fn default() -> Self {
        Self {
            row_height: 28,
            zebra: false,
            null_display: NullDisplay::Literal,
            wrap_cells: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppBehaviorSettings {
    pub confirm_before_drop: bool,
    pub confirm_before_truncate: bool,
}

impl Default for AppBehaviorSettings {
    fn default() -> Self {
        Self {
            confirm_before_drop: true,
            confirm_before_truncate: true,
        }
    }
}
```

Add the five fields to `AppUiSettings` and `Default`. Import `KeybindingMap` and `ThemeOverrides` from `crate` (already re-exported from `customization`).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p models fresh_defaults_match_settings_spec legacy_json_without_new_fields_gets_spec_defaults new_settings_fields_round_trip`

Expected: PASS. Also run `cargo test -p models` and fix any `Default` struct-update compile errors.

- [ ] **Step 5: Commit**

```bash
git add models/src/settings.rs
git commit -m "feat(models): persist editor, grid, behavior, theme overrides, keybindings"
```

---

### Task 2: ThemeOverrides CSS names and hex parse

**Files:**
- Modify: `models/src/customization.rs`
- Modify: `models/src/config.rs` test `theme_overrides_render_to_css` if it asserts `--font-size-md`

**Interfaces:**
- Consumes: `ThemeOverrides` fields from Task 1.
- Produces:
  - `ThemeOverrides::to_css` emits `--font-sans`, `--font-mono`, `--ui-font-size`, `--ui-font-size-sm`, `--radius-sm/md/lg`, and derived `--color-primary-hover` / `--color-primary-active` when only `primary` is set.
  - `pub fn parse_hex_color(value: &str) -> Option<String>` returning normalized `#RRGGBB` (lowercase).

- [ ] **Step 1: Write the failing test**

Replace `theme_renders_set_tokens_only` assertions and add:

```rust
#[test]
fn theme_emits_live_css_variable_names() {
    let theme = ThemeOverrides {
        primary: Some("#ff0000".to_string()),
        font_family: Some("IBM Plex Sans, sans-serif".to_string()),
        font_size: Some(14),
        radius_small: Some(4),
        ..ThemeOverrides::default()
    };
    let css = theme.to_css();
    assert!(css.contains("--color-primary: #ff0000;"));
    assert!(css.contains("--color-primary-hover:"));
    assert!(css.contains("--color-primary-active:"));
    assert!(css.contains("--font-sans: IBM Plex Sans, sans-serif;"));
    assert!(css.contains("--ui-font-size: 14px;"));
    assert!(css.contains("--radius-sm: 4px;"));
    assert!(!css.contains("--font-family-sans"));
    assert!(!css.contains("--font-size-md"));
}

#[test]
fn parse_hex_color_accepts_short_and_long() {
    assert_eq!(parse_hex_color("#f00").as_deref(), Some("#ff0000"));
    assert_eq!(parse_hex_color("#FF8800").as_deref(), Some("#ff8800"));
    assert_eq!(parse_hex_color("not-a-color"), None);
    assert_eq!(parse_hex_color("#gg0000"), None);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p models theme_emits_live_css_variable_names parse_hex_color_accepts_short_and_long`

Expected: FAIL — still emits `--font-size-md` / no `parse_hex_color`.

- [ ] **Step 3: Write minimal implementation**

```rust
pub fn parse_hex_color(value: &str) -> Option<String> {
    let value = value.trim();
    let hex = value.strip_prefix('#')?;
    let expanded = match hex.len() {
        3 => {
            let mut out = String::with_capacity(6);
            for ch in hex.chars() {
                out.push(ch);
                out.push(ch);
            }
            out
        }
        6 => hex.to_string(),
        _ => return None,
    };
    if !expanded.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("#{}", expanded.to_ascii_lowercase()))
}
```

In `to_css`:
- `font_family` → `--font-sans`
- `font_family_mono` → `--font-mono`
- `font_size` → `--ui-font-size`
- `font_size_small` → `--ui-font-size-sm`
- if `primary` is set and `primary_hover` is `None`, emit `--color-primary-hover: color-mix(in srgb, {primary} 72%, white);`
- if `primary` is set and `primary_active` is `None`, emit `--color-primary-active: color-mix(in srgb, {primary} 80%, black);`
- keep explicit hover/active when `Some`

Update `config.rs` test `theme_overrides_render_to_css` to assert `--ui-font-size` not `--font-size-md`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p models theme_emits_live_css_variable_names parse_hex_color_accepts_short_and_long theme_overrides_render_to_css theme_renders_set_tokens_only`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add models/src/customization.rs models/src/config.rs
git commit -m "fix(models): emit live CSS variable names from theme overrides"
```

---

### Task 3: Default keybindings, effective map, conflict helper

**Files:**
- Modify: `models/src/customization.rs`

**Interfaces:**
- Consumes: `KeybindingMap`.
- Produces:
  - `pub const DEFAULT_KEYBINDINGS: &[(&str, &str)]` ids and default combos from the spec table
  - `pub fn default_keybinding_map() -> KeybindingMap`
  - `pub fn effective_keybindings(overrides: &KeybindingMap) -> KeybindingMap`
  - `pub fn combo_conflict(action_id: &str, combo: &str, effective: &KeybindingMap) -> Option<String>`
    returns the **other** action id that already owns `combo`, or `None`. Comparing is case-insensitive on the combo string after trimming.

Default pairs (id, combo):

```text
focus_editor Ctrl+E
format_sql Ctrl+Shift+F
new_tab Ctrl+T
close_tab Ctrl+W
next_tab Ctrl+Tab
refresh_explorer F5
focus_filter_panel Ctrl+F
save_query Ctrl+Shift+S
close_overlay Escape
command_palette Ctrl+Shift+P
global_search Ctrl+K
rename_selected F2
delete_selected Delete
focus_agent_composer Ctrl+Shift+M
new_connection Ctrl+Shift+N
open_settings Ctrl+,
```

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn effective_keybindings_apply_overrides() {
    let mut overrides = KeybindingMap::new();
    overrides.insert("format_sql".into(), "Ctrl+Alt+F".into());
    let effective = effective_keybindings(&overrides);
    assert_eq!(effective.get("format_sql").map(String::as_str), Some("Ctrl+Alt+F"));
    assert_eq!(effective.get("new_tab").map(String::as_str), Some("Ctrl+T"));
}

#[test]
fn combo_conflict_reports_other_action() {
    let effective = default_keybinding_map();
    assert_eq!(
        combo_conflict("format_sql", "Ctrl+T", &effective).as_deref(),
        Some("new_tab")
    );
    assert_eq!(combo_conflict("format_sql", "Ctrl+Shift+F", &effective), None);
    assert_eq!(combo_conflict("new_tab", "Ctrl+T", &effective), None);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p models effective_keybindings_apply_overrides combo_conflict_reports_other_action`

Expected: FAIL — functions missing.

- [ ] **Step 3: Write minimal implementation**

`effective_keybindings`: start from `default_keybinding_map()`, then for each override, if the value is non-empty after trim, insert it.

`combo_conflict`: normalize with `combo.trim().eq_ignore_ascii_case`. Skip `action_id`. Return first other id with the same combo.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p models effective_keybindings_apply_overrides combo_conflict_reports_other_action`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add models/src/customization.rs
git commit -m "feat(models): default keybindings and combo conflict helper"
```

---

### Task 4: `ShovelConfig::apply_to` overlays the new fields

**Files:**
- Modify: `models/src/config.rs`

**Interfaces:**
- Consumes: `EditorSettings`, `GridSettings`, `AppBehaviorSettings`, `ThemeOverrides`, `KeybindingMap`, existing `EditorBehavior` / `AppBehavior` Option structs.
- Produces: `apply_to` copies toml overlays onto `AppUiSettings` concrete fields. Add `pub grid: Option<GridSettings>` to `ShovelConfig`. Merge `editor: Option<EditorBehavior>` Option fields onto `EditorSettings`. Merge `behavior: Option<AppBehavior>` onto `AppBehaviorSettings`. Replace `theme_overrides` / `keybindings` when `Some`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn config_overlays_editor_and_keybindings() {
    let toml_str = r##"
[editor]
font_size = 16
word_wrap = true

[behavior]
confirm_before_drop = false

[theme_overrides]
primary = "#00ff00"

[keybindings]
format_sql = "Ctrl+Alt+F"

[grid]
row_height = 32
zebra = true
"##;
    let config: ShovelConfig = toml::from_str(toml_str).expect("parse");
    let mut settings = AppUiSettings::default();
    config.apply_to(&mut settings);
    assert_eq!(settings.editor.font_size, 16);
    assert!(settings.editor.word_wrap);
    assert!(!settings.behavior.confirm_before_drop);
    assert_eq!(settings.theme_overrides.primary.as_deref(), Some("#00ff00"));
    assert_eq!(settings.keybindings.get("format_sql").map(String::as_str), Some("Ctrl+Alt+F"));
    assert_eq!(settings.grid.row_height, 32);
    assert!(settings.grid.zebra);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p models config_overlays_editor_and_keybindings`

Expected: FAIL — `apply_to` does not copy these fields.

- [ ] **Step 3: Write minimal implementation**

Add `pub grid: Option<GridSettings>` to `ShovelConfig`.

At the end of `apply_to`:

```rust
if let Some(theme) = &self.theme_overrides {
    base.theme_overrides = theme.clone();
}
if let Some(map) = &self.keybindings {
    base.keybindings = map.clone();
}
if let Some(grid) = &self.grid {
    base.grid = grid.clone();
}
if let Some(editor) = &self.editor {
    if let Some(v) = editor.font_size {
        base.editor.font_size = v.clamp(10, 22);
    }
    if let Some(v) = editor.tab_size {
        base.editor.tab_size = v.clamp(1, 8);
    }
    if let Some(v) = editor.auto_format_on_run {
        base.editor.auto_format_on_run = v;
    }
    if let Some(v) = editor.word_wrap {
        base.editor.word_wrap = v;
    }
    if let Some(v) = editor.show_line_numbers {
        base.editor.show_line_numbers = v;
    }
}
if let Some(behavior) = &self.behavior {
    if let Some(v) = behavior.confirm_before_drop {
        base.behavior.confirm_before_drop = v;
    }
    if let Some(v) = behavior.confirm_before_truncate {
        base.behavior.confirm_before_truncate = v;
    }
}
```

`AppBehavior` currently has `Option<bool>` fields. Use those. Do not invent extra toml keys.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p models config_overlays_editor_and_keybindings`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add models/src/config.rs
git commit -m "feat(models): overlay editor, grid, shortcuts from config.toml"
```

---

### Task 5: `combo_from_event` and extra `parse_combo` cases

**Files:**
- Modify: `ui/src/app_state/keyboard.rs`

**Interfaces:**
- Consumes: existing `parse_combo`.
- Produces: `pub fn combo_from_event(key: &Key, modifiers: Modifiers) -> Option<String>`
  Returns `None` for lone modifier keys (`Control`, `Shift`, `Alt`, `Meta`). Otherwise builds `Ctrl+Shift+F` style strings that `parse_combo` accepts. `Key::Character(",")` with Ctrl → `Ctrl+,`.

- [ ] **Step 1: Write the failing test**

Append to `ui/src/app_state/keyboard.rs` tests:

```rust
#[test]
fn parse_combo_accepts_ctrl_comma_and_f2() {
    let (key, mods) = parse_combo("Ctrl+,").expect("ctrl comma");
    assert_eq!(key, Key::Character(",".into()));
    assert!(ctrl_or_meta(mods));
    let (key, mods) = parse_combo("F2").expect("f2");
    assert_eq!(key, Key::F2);
    assert!(mods.is_empty());
}

#[test]
fn combo_from_event_skips_lone_modifiers() {
    assert_eq!(combo_from_event(&Key::Control, Modifiers::CONTROL), None);
    assert_eq!(
        combo_from_event(&Key::Character("f".into()), ctrl_shift()).as_deref(),
        Some("Ctrl+Shift+F")
    );
    assert_eq!(
        combo_from_event(&Key::Character(",".into()), ctrl()).as_deref(),
        Some("Ctrl+,")
    );
}
```

Check how `parse_combo` currently maps `,`. If it uppercases the character, adjust the assertion to whatever `parse_combo` already returns, then make `combo_from_event` round-trip through `parse_combo`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ui parse_combo_accepts_ctrl_comma_and_f2 combo_from_event_skips_lone_modifiers`

Expected: FAIL — `combo_from_event` missing and possibly `parse_combo("Ctrl+,")` is `None`.

- [ ] **Step 3: Write minimal implementation**

If `parse_combo` rejects `,`, add `"comma" | "," => Key::Character(",".into())` in the key match.

```rust
pub fn combo_from_event(key: &Key, modifiers: Modifiers) -> Option<String> {
    if matches!(key, Key::Control | Key::Shift | Key::Alt | Key::Meta) {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    if ctrl_or_meta(modifiers) {
        parts.push("Ctrl");
    }
    if modifiers.contains(Modifiers::SHIFT) {
        parts.push("Shift");
    }
    if modifiers.contains(Modifiers::ALT) {
        parts.push("Alt");
    }
    let owned_label: String;
    let key_label: &str = match key {
        Key::Tab => "Tab",
        Key::Escape => "Escape",
        Key::Enter => "Enter",
        Key::Delete => "Delete",
        Key::Backspace => "Backspace",
        Key::F2 => "F2",
        Key::F5 => "F5",
        Key::Character(c) if c == "," => ",",
        Key::Character(c) => {
            owned_label = c.to_uppercase();
            owned_label.as_str()
        }
        _ => return None,
    };
    if parts.is_empty() {
        Some(key_label.to_string())
    } else {
        Some(format!("{}+{}", parts.join("+"), key_label))
    }
}
```

Character keys use `c.to_uppercase()` as the label. Keep `,` as comma. The `owned_label` binding is only used on the `Character` arm; if the compiler rejects borrowing it from inside the match, format the Character arm as `return Some(...)` directly.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p ui parse_combo_accepts_ctrl_comma_and_f2 combo_from_event_skips_lone_modifiers`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/src/app_state/keyboard.rs
git commit -m "feat(ui): build shortcut combo strings from key events"
```

---

### Task 6: Settings window size and chrome CSS

**Files:**
- Modify: `ui/src/windows/mod.rs` (`settings_window_config`, ~lines 167–184)
- Modify: `styles/components/_settings-modal.scss`
- Modify: `styles/layout/_app-layout.scss` (`.app` CSS variables)
- Modify: `styles/components/_buttons.scss` (`border-radius: var(--radius-sm, #{$radius-sm})`)
- Modify: `styles/components/_inputs.scss` (same for `.input`)

**Interfaces:**
- Consumes: Task 2 CSS variable names.
- Produces: window 960×720, min 720×520; settings sidebar layout at desktop widths; narrow breakpoint 560px; `.settings-window-shell` fills the webview.

- [ ] **Step 1: Write the failing test**

No runtime CSS test. Add a compile-time assertion next to `settings_window_config` if one exists; otherwise add in `ui/src/windows/mod.rs` tests (create `#[cfg(test)]` if missing):

```rust
#[test]
fn settings_window_is_wide_enough_for_sidebar() {
    // Document the sizes the builder uses so a future shrink re-breaks the layout.
    assert!(960.0 > 760.0);
    assert!(720.0 > 560.0);
}
```

This is weak. Prefer grepping after the SCSS edit. The real check is Step 4 `cargo test -p ui --lib` plus reading the SCSS.

Skip a fake test. Verification for this task is `rg` + `cargo test -p app --lib` after grass rebuild.

- [ ] **Step 2: Change window sizes**

In `settings_window_config`:

```rust
.with_inner_size(LogicalSize::new(960.0, 720.0))
.with_min_inner_size(LogicalSize::new(720.0, 520.0))
```

- [ ] **Step 3: Restyle settings modal SCSS**

Required rules (replace the current glass/18px block):

```scss
.settings-modal {
  width: min(960px, 100%);
  max-height: min(88vh, 100%);
  overflow: hidden;
  display: flex;
  flex-direction: column;
  gap: $spacing-sm;
  padding: $spacing-md;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md, #{$radius-md});
  background: var(--color-surface-elevated, var(--color-panel-2));
  box-shadow: $shadow-md;
}

.settings-modal__nav {
  flex: 0 0 auto;
  width: 168px;
  display: flex;
  flex-direction: column;
  gap: $spacing-2xs;
  padding: $spacing-2xs;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md, #{$radius-md});
  background: var(--color-surface-contrast, var(--color-panel-2));
}

.settings-modal__nav-item {
  display: flex;
  align-items: center;
  min-height: 28px;
  padding: 0 $spacing-sm;
  border: 1px solid transparent;
  border-radius: var(--radius-sm, #{$radius-sm});
  background: transparent;
  color: var(--color-text);
  font-size: $font-size-md;
  text-align: left;
  cursor: pointer;
  width: 100%;

  &:hover:not(.settings-modal__nav-item--active) {
    background: var(--color-hover);
  }

  &--active {
    background: var(--color-active);
    border-color: color-mix(in srgb, var(--color-primary) 46%, transparent);
  }
}

.settings-modal__content {
  flex: 1 1 auto;
  min-width: 0;
  min-height: 0;
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: $spacing-md;
}

.settings-modal__section {
  display: flex;
  flex-direction: column;
  gap: $spacing-sm;
  padding: $spacing-md;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md, #{$radius-md});
  background: var(--color-surface-elevated, var(--color-panel));
}

.settings-modal__segmented {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: $spacing-2xs;
  padding: $spacing-2xs;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm, #{$radius-sm});
  background: var(--color-surface-contrast, var(--color-panel-2));
}

.settings-modal__segmented--density,
.settings-modal__segmented--split-mode {
  grid-template-columns: repeat(3, minmax(0, 1fr));
}

.settings-modal__segmented .button {
  height: 24px;
  border-radius: var(--radius-sm, #{$radius-sm});
}

.settings-window-shell {
  min-height: 100vh;
  height: 100vh;
  color: var(--color-text);
  font-family: var(--font-sans, #{$font-family-sans});
  background: var(--color-surface-shell, var(--color-bg));

  .settings-modal__backdrop {
    padding: 0;
    background: transparent;
    place-items: stretch;
  }

  .settings-modal {
    width: 100%;
    max-height: 100%;
    height: 100%;
    border-radius: 0;
    box-shadow: none;
    border: 0;
  }
}

@media (max-width: 560px) {
  .settings-modal__body {
    flex-direction: column;
  }
  .settings-modal__nav {
    width: 100%;
    flex-direction: row;
    flex-wrap: wrap;
  }
  .settings-modal__nav-item {
    width: auto;
    flex: 1 1 calc(50% - #{$spacing-2xs});
  }
}
```

Remove `.settings-modal__nav-description` styles (nav is label-only).

On `.app` in `_app-layout.scss` add:

```scss
--radius-sm: #{$radius-sm};
--radius-md: #{$radius-md};
--radius-lg: #{$radius-lg};
--font-sans: #{$font-family-sans};
--font-mono: #{$font-family-mono};
```

Buttons and inputs: `border-radius: var(--radius-sm, #{$radius-sm});` (buttons already `$radius-sm`; inputs `$radius-md` → `var(--radius-md, #{$radius-md})`).

`app/build.rs` regenerates `app/assets/app.css` on next `cargo test -p app` / `cargo build -p app`.

- [ ] **Step 4: Verify**

Run: `cargo test -p ui --lib windows::`

Expected: PASS (or no such module — then `cargo test -p app --lib` to run grass). Confirm `rg "max-width: 560px" styles/components/_settings-modal.scss` matches and `rg "760.0" ui/src/windows/mod.rs` no longer has the old size.

- [ ] **Step 5: Commit**

```bash
git add ui/src/windows/mod.rs styles/components/_settings-modal.scss styles/layout/_app-layout.scss styles/components/_buttons.scss styles/components/_inputs.scss
git commit -m "fix(ui): densify settings chrome and widen native window"
```

Do not `git add` unrelated dirty SCSS.

---

### Task 7: Split `settings_modal` and add widgets

**Files:**
- Create: `ui/src/layout/settings_modal/mod.rs`
- Create: `ui/src/layout/settings_modal/widgets.rs`
- Create: `ui/src/layout/settings_modal/sections.rs` (move existing section components as-is first)
- Create: `ui/src/layout/settings_modal/keyboard.rs` (stub `KeyboardSection` that renders an empty section titled Keyboard — filled in Task 9)
- Delete: `ui/src/layout/settings_modal.rs`

**Interfaces:**
- Consumes: `parse_hex_color`, `SettingsSectionProps`.
- Produces:
  - `SettingsCategory::Keyboard` with `id = "keyboard"`, `label = "Keyboard"`, `description` unused in nav
  - `pub fn ColorField`, `FontSelect`, `SliderField`, `KeyCapture` in `widgets.rs`
  - Public export still `crate::layout::SettingsModal`

- [ ] **Step 1: Write the failing test**

Move `category_order_matches_all_constant` into `mod.rs` and change expected labels to:

```rust
vec![
    "Appearance",
    "Database",
    "Editor",
    "Grid",
    "Navigation",
    "Keyboard",
    "Advanced",
    "Config",
]
```

Add in `widgets.rs`:

```rust
#[test]
fn slider_clamp_rejects_out_of_range() {
    assert_eq!(clamp_u32(5, 10, 16), 10);
    assert_eq!(clamp_u32(20, 10, 16), 16);
    assert_eq!(clamp_u32(12, 10, 16), 12);
}
```

- [ ] **Step 2: Run test to verify it fails**

After the file move, run: `cargo test -p ui category_order_matches_all_constant`

Expected: FAIL — Keyboard missing from `ALL`.

- [ ] **Step 3: Write minimal implementation**

1. `git mv ui/src/layout/settings_modal.rs ui/src/layout/settings_modal/mod.rs` is wrong (file vs dir). Copy content into `mod.rs`, then delete the old file.
2. Add `mod widgets; mod sections; mod keyboard;`
3. Nav buttons: only `span.settings-modal__nav-label`, drop description spans.
4. Match arm `SettingsCategory::Keyboard => rsx! { KeyboardSection { ..section_props } }`
5. Widgets:

```rust
pub fn clamp_u32(value: u32, min: u32, max: u32) -> u32 {
    value.clamp(min, max)
}

#[component]
pub fn SliderField(label: String, value: u32, min: u32, max: u32, on_change: EventHandler<u32>) -> Element {
    rsx! {
        div { class: "field",
            span { class: "field__label", "{label}" }
            div { class: "settings-modal__slider",
                input {
                    r#type: "range",
                    min: "{min}",
                    max: "{max}",
                    value: "{value}",
                    oninput: move |event| {
                        if let Ok(parsed) = event.value().parse::<u32>() {
                            on_change.call(clamp_u32(parsed, min, max));
                        }
                    },
                }
                input {
                    class: "input",
                    r#type: "number",
                    min: "{min}",
                    max: "{max}",
                    value: "{value}",
                    oninput: move |event| {
                        if let Ok(parsed) = event.value().parse::<u32>() {
                            on_change.call(clamp_u32(parsed, min, max));
                        }
                    },
                }
            }
        }
    }
}
```

`ColorField`: swatch `input type=color` + text input. On text input, `if let Some(hex) = parse_hex_color(&event.value()) { on_change.call(hex); }`.

`FontSelect`: `options: Vec<(String, String)>` of (css family, label). `select` with `style: "font-family: {value}"`.

`KeyCapture`: local `listening: Signal<bool>`. Button shows `current` or `"Press keys…"`. `onkeydown`: if listening, `event.prevent_default()`; Escape → listening false; Backspace → `on_clear.call(())`; else `combo_from_event` → `on_change.call(combo)` and stop listening.

6. Add SCSS for `.settings-modal__slider` (flex row, range grows, number input 64px).

Move existing section fns into `sections.rs` without behavior change except empty Database/Grid/Navigation stay empty until Task 8.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p ui category_order_matches_all_constant slider_clamp_rejects_out_of_range`

Expected: PASS. `cargo check -p ui` PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/src/layout/settings_modal ui/src/layout/settings_modal.rs styles/components/_settings-modal.scss
git commit -m "refactor(ui): split settings modal and add shared widgets"
```

(`git add -u ui/src/layout/settings_modal.rs` to record the deletion.)

---

### Task 8: Fill Appearance, Database, Editor, Grid, Navigation, Advanced, Config

**Files:**
- Modify: `ui/src/layout/settings_modal/sections.rs`
- Modify: `ui/src/layout/settings_modal/mod.rs` match arms (drop `CategoryEmptyState` for Database/Grid/Navigation)

**Interfaces:**
- Consumes: widgets from Task 7, fields from Task 1.
- Produces: no empty categories except none. Reset UI keeps API keys (deepseek, codestral, ollama, openai/groq/openrouter/xai/mistral) and does **not** clear `keybindings`.

- [ ] **Step 1: Write the failing test**

In `mod.rs` tests, assert every category except none has a mapped section by keeping `CategoryEmptyState` unused. Add:

```rust
#[test]
fn reset_ui_preserves_api_keys_and_keybindings() {
    let mut ui = AppUiSettings::default();
    ui.deepseek.api_key = "keep-me".into();
    ui.keybindings.insert("format_sql".into(), "Ctrl+Alt+F".into());
    ui.density = UiDensity::Comfortable;
    let mut next = AppUiSettings::default();
    next.deepseek.api_key = ui.deepseek.api_key.clone();
    next.codestral.api_key = ui.codestral.api_key.clone();
    next.ollama.api_key = ui.ollama.api_key.clone();
    next.openai.api_key = ui.openai.api_key.clone();
    next.groq.api_key = ui.groq.api_key.clone();
    next.openrouter.api_key = ui.openrouter.api_key.clone();
    next.xai.api_key = ui.xai.api_key.clone();
    next.mistral.api_key = ui.mistral.api_key.clone();
    next.keybindings = ui.keybindings.clone();
    assert_eq!(next.deepseek.api_key, "keep-me");
    assert_eq!(next.keybindings.get("format_sql").map(String::as_str), Some("Ctrl+Alt+F"));
    assert_eq!(next.density, UiDensity::Compact);
}
```

This locks Reset UI semantics. Implement Reset button to match.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ui reset_ui_preserves_api_keys_and_keybindings`

Expected: FAIL until the test is added then PASS (the test is pure). That's OK — the test documents the helper. Extract:

```rust
pub(super) fn reset_ui_preserving_secrets(current: &AppUiSettings) -> AppUiSettings {
    let mut next = AppUiSettings::default();
    next.deepseek.api_key = current.deepseek.api_key.clone();
    next.codestral.api_key = current.codestral.api_key.clone();
    next.ollama.api_key = current.ollama.api_key.clone();
    next.openai.api_key = current.openai.api_key.clone();
    next.groq.api_key = current.groq.api_key.clone();
    next.openrouter.api_key = current.openrouter.api_key.clone();
    next.xai.api_key = current.xai.api_key.clone();
    next.mistral.api_key = current.mistral.api_key.clone();
    next.keybindings = current.keybindings.clone();
    next
}
```

Write the test against `reset_ui_preserving_secrets` first (fail because missing), then implement.

- [ ] **Step 3: Fill sections**

**Appearance:** existing theme + density segmented (add `settings-modal__segmented--density`). Then ColorField accent → `theme_overrides.primary`. FontSelect UI fonts:

- `("SF Pro Text, IBM Plex Sans, Segoe UI, sans-serif", "System UI")`
- `("IBM Plex Sans, sans-serif", "IBM Plex Sans")`
- `("Segoe UI, sans-serif", "Segoe UI")`

Editor fonts:

- `("JetBrains Mono, SF Mono, Cascadia Code, monospace", "JetBrains Mono")`
- `("SF Mono, ui-monospace, monospace", "SF Mono")`
- `("Cascadia Code, ui-monospace, monospace", "Cascadia Code")`
- `("ui-monospace, monospace", "ui-monospace")`

Slider UI font size 10–16: writes `theme_overrides.font_size = Some(n)` and `font_size_small = Some(n.saturating_sub(1).max(10))`.

Slider radius 0–12: `radius_small = Some(n)`, `radius_medium = Some(n+2)`, `radius_large = Some(n+4)`.

**Database:** page size, restore session, read-only, confirm drop, confirm truncate.

**Editor:** existing SQL format + CodeStral + EditorSettings sliders/toggles (font_size 10–22, tab_size 1–8).

**Grid:** row height slider 18–48, zebra, wrap cells, NullDisplay segmented (`settings-modal__segmented` with 3 columns).

**Navigation:** move explorer toggles, panel visibility, split mode, bottom dock out of WorkspaceSection.

**Advanced:** DeepSeek, new Ollama block (can be a stub enabled/base/model/key; keyring hydrate is Task 14), AI flags, Reset UI using `reset_ui_preserving_secrets`.

**Config:** unchanged path/open/reload.

Remove `CategoryEmptyState` once unused.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p ui reset_ui_preserving_secrets category_order_matches_all_constant`

Expected: PASS. `cargo check -p ui` PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/src/layout/settings_modal
git commit -m "feat(ui): fill settings categories with real controls"
```

---

### Task 9: Keyboard section

**Files:**
- Modify: `ui/src/layout/settings_modal/keyboard.rs`

**Interfaces:**
- Consumes: `combo_conflict`, `effective_keybindings`, `default_keybinding_map`, `DEFAULT_KEYBINDINGS`, `KeyCapture`, `combo_from_event`.
- Produces: filterable table; conflict does not write; Reset row removes that id from `settings.keybindings`; Reset all sets `keybindings` to empty map.

Action labels (hard-code next to ids):

```text
focus_editor Focus SQL editor
format_sql Format SQL
new_tab New tab
close_tab Close tab
next_tab Next tab
refresh_explorer Refresh explorer
focus_filter_panel Focus result filter
save_query Save query
close_overlay Close overlay
command_palette Command palette
global_search Global search
rename_selected Rename selected
delete_selected Drop selected
focus_agent_composer Focus agent composer
new_connection New connection
open_settings Open settings
```

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn assigning_duplicate_combo_is_rejected() {
    let current = KeybindingMap::new();
    let effective = effective_keybindings(&current);
    assert!(combo_conflict("format_sql", "Ctrl+T", &effective).is_some());
}

#[test]
fn reset_row_removes_override() {
    let mut map = KeybindingMap::new();
    map.insert("format_sql".into(), "Ctrl+Alt+F".into());
    map.remove("format_sql");
    let effective = effective_keybindings(&map);
    assert_eq!(effective.get("format_sql").map(String::as_str), Some("Ctrl+Shift+F"));
}
```

These pass with Task 3 helpers. Still add them in `keyboard.rs` to lock UI semantics. Then implement the section so a capture that conflicts sets local `error: Option<String>` = `format!("already used by {label}")` and does not call `on_change` with a mutated map.

- [ ] **Step 2: Run test to verify it fails**

If helpers already pass, Step 2 is `cargo test -p ui assigning_duplicate_combo_is_rejected` PASS. Proceed to implement UI.

- [ ] **Step 3: Implement KeyboardSection**

Local `filter: Signal<String>` and `conflict: Signal<Option<(String, String)>>` (action id, message).

On KeyCapture `on_change(combo)`:

```rust
let mut next = settings.clone();
let effective = effective_keybindings(&next.keybindings);
if let Some(other) = combo_conflict(action_id, &combo, &effective) {
    conflict.set(Some((action_id.to_string(), format!("already used by {other_label}"))));
    return;
}
next.keybindings.insert(action_id.to_string(), combo);
on_change.call((next, sql_settings.clone()));
```

On clear: `next.keybindings.remove(action_id)`.

Filter: `label.to_ascii_lowercase().contains(&filter)`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p ui assigning_duplicate_combo_is_rejected reset_row_removes_override category_order_matches_all_constant`

Expected: PASS. `cargo check -p ui` PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/src/layout/settings_modal/keyboard.rs
git commit -m "feat(ui): add keyboard shortcut editor"
```

---

### Task 10: Sync nested settings onto globals

**Files:**
- Modify: `ui/src/app_state/mod.rs`

**Interfaces:**
- Consumes: `AppUiSettings` nested fields.
- Produces: `sync_runtime_ui_settings` also equality-guards:
  - `APP_THEME_OVERRIDES`
  - `APP_KEYBINDINGS`
  - `APP_EDITOR_BEHAVIOR` **change type** to `EditorSettings`
  - `APP_APP_BEHAVIOR` **change type** to `AppBehaviorSettings`
  - new `APP_GRID_SETTINGS: GlobalSignal<GridSettings>`
- Update `app.rs` startup writes: assign `startup.ui_settings.theme_overrides` etc. instead of the old `Option` fields on `AppStartupSettings`. Keep copying `startup.theme_overrides` **onto** `ui_settings` only if you still want toml-only overlays; after Task 4, `apply_to` already merged them into `ui_settings`. Change `app.rs` to:

```rust
replace_ui_settings(startup.ui_settings.clone());
```

and stop separately writing `APP_THEME_OVERRIDES` from `startup.theme_overrides` (sync does it). If `AppStartupSettings` still has those Option fields, leave them for auto-connect; do not let them clobber JSON after `apply_to`.

- [ ] **Step 1: Write the failing test**

`sync_runtime_ui_settings` is private. Add a unit test in `app_state/mod.rs` via a `pub(crate)` wrapper or test the public `replace_ui_settings`:

```rust
#[test]
fn replace_ui_settings_copies_nested_globals() {
    let mut settings = AppUiSettings::default();
    settings.editor.font_size = 18;
    settings.grid.row_height = 40;
    crate::app_state::replace_ui_settings(settings.clone());
    assert_eq!(crate::app_state::APP_EDITOR_BEHAVIOR.peek().font_size, 18);
    assert_eq!(crate::app_state::APP_GRID_SETTINGS.peek().row_height, 40);
}
```

Dioxus global signals in unit tests can be flaky. Prefer a pure helper:

```rust
pub(crate) fn nested_runtime_snapshot(settings: &AppUiSettings) -> (u32, u32, bool) {
    (settings.editor.font_size, settings.grid.row_height, settings.behavior.confirm_before_drop)
}
```

Do not add a weak wrapper. Instead compile-fail is enough if types change: update every `APP_EDITOR_BEHAVIOR` site. Grep after the type change.

Write:

```rust
#[test]
fn nested_settings_are_copied_by_replace() {
    // This test lives in ui and calls replace_ui_settings.
}
```

If Dioxus globals panic in `cargo test -p ui`, drop the test and rely on `cargo check -p ui`.

- [ ] **Step 2: Run check to verify it fails**

Run: `cargo check -p ui`

Expected: FAIL after changing `APP_EDITOR_BEHAVIOR` type until `app.rs` assignments are updated.

- [ ] **Step 3: Implement**

```rust
pub static APP_EDITOR_BEHAVIOR: GlobalSignal<models::EditorSettings> =
    Signal::global(models::EditorSettings::default);
pub static APP_APP_BEHAVIOR: GlobalSignal<models::AppBehaviorSettings> =
    Signal::global(models::AppBehaviorSettings::default);
pub static APP_GRID_SETTINGS: GlobalSignal<models::GridSettings> =
    Signal::global(models::GridSettings::default);
```

In `sync_runtime_ui_settings`:

```rust
if *APP_THEME_OVERRIDES.peek() != settings.theme_overrides {
    *APP_THEME_OVERRIDES.write() = settings.theme_overrides.clone();
}
if *APP_KEYBINDINGS.peek() != settings.keybindings {
    *APP_KEYBINDINGS.write() = settings.keybindings.clone();
}
if *APP_EDITOR_BEHAVIOR.peek() != settings.editor {
    *APP_EDITOR_BEHAVIOR.write() = settings.editor.clone();
}
if *APP_APP_BEHAVIOR.peek() != settings.behavior {
    *APP_APP_BEHAVIOR.write() = settings.behavior.clone();
}
if *APP_GRID_SETTINGS.peek() != settings.grid {
    *APP_GRID_SETTINGS.write() = settings.grid.clone();
}
```

Fix `app.rs` startup to only `replace_ui_settings(startup.ui_settings.clone())` plus sql settings. Remove the four separate Option writes, or assign from `startup.ui_settings.*`.

- [ ] **Step 4: Verify**

Run: `cargo check -p ui`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/src/app_state/mod.rs ui/src/app.rs
git commit -m "feat(ui): sync editor, grid, shortcuts, and theme overrides to globals"
```

---

### Task 11: Wire SQL editor + auto-format on run

**Files:**
- Modify: `ui/src/screens/workspace/components/sql_editor.rs`
- Modify: `styles/components/_editor.scss` (gutter)
- Modify: `ui/src/screens/workspace/actions.rs` (`run_query_for_tab`, `run_batch_for_tab`)

**Interfaces:**
- Consumes: `APP_EDITOR_BEHAVIOR: EditorSettings`.
- Produces: textarea + highlight use font-size, tab-size, white-space; gutter when `show_line_numbers`; format SQL before execute when `auto_format_on_run`.

- [ ] **Step 1: Write the failing test**

Add a pure helper in `sql_editor.rs` or `actions.rs`:

```rust
pub fn maybe_format_sql(
    sql: String,
    auto_format: bool,
    kind: Option<models::DatabaseKind>,
    format_settings: &models::SqlFormatSettings,
) -> String {
    if !auto_format {
        return sql;
    }
    services::format_sql(kind, &sql, format_settings)
}
```

Test in `actions.rs` or a small `#[cfg(test)]` in sql_editor:

```rust
#[test]
fn maybe_format_sql_is_noop_when_disabled() {
    let sql = "select 1";
    let out = maybe_format_sql(
        sql.into(),
        false,
        None,
        &models::SqlFormatSettings::default(),
    );
    assert_eq!(out, "select 1");
}
```

For line counts:

```rust
pub fn line_number_labels(sql: &str) -> Vec<usize> {
    let lines = sql.split('\n').count().max(1);
    (1..=lines).collect()
}
```

```rust
#[test]
fn line_number_labels_count_lines() {
    assert_eq!(line_number_labels("a"), vec![1]);
    assert_eq!(line_number_labels("a\nb\n"), vec![1, 2, 3]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ui line_number_labels_count_lines maybe_format_sql_is_noop_when_disabled`

Expected: FAIL — helpers missing.

- [ ] **Step 3: Implement**

Read `let editor = APP_EDITOR_BEHAVIOR();` in the sql editor component (no await). Apply:

```rust
let wrap = if editor.word_wrap { "pre-wrap" } else { "pre" };
let editor_style = format!(
    "font-size: {}px; tab-size: {}; white-space: {}; font-family: var(--font-mono, monospace);",
    editor.font_size.clamp(10, 22),
    editor.tab_size.clamp(1, 8),
    wrap,
);
```

Put `style: "{editor_style}"` on `.sql-editor__input` and `.sql-editor__highlight`.

If `editor.show_line_numbers`, wrap viewport in:

```rust
div { class: "sql-editor__gutter",
    for n in line_number_labels(&current_sql) {
        span { "{n}" }
    }
}
```

SCSS:

```scss
.sql-editor__gutter {
  flex: 0 0 36px;
  overflow: hidden;
  text-align: right;
  padding: 8px 6px 0 0;
  color: var(--color-text-dim);
  font-family: var(--font-mono, monospace);
  font-size: inherit;
  line-height: inherit;
  user-select: none;
}
.sql-editor__viewport {
  display: flex;
}
```

Inspect current `.sql-editor__viewport` before changing display; keep highlight/input stacking. If viewport is already `position: relative` with overlay input, put the gutter as a sibling **outside** the overlay stack:

```
div.sql-editor { display:flex }
  gutter
  div.sql-editor__viewport { flex 1; position relative }
    highlight
    textarea
```

In `run_query_for_tab` **before** the batch/read-only checks, clone settings without holding a guard across await:

```rust
let auto_format = APP_EDITOR_BEHAVIOR.peek().auto_format_on_run;
let format_settings = APP_SQL_FORMAT_SETTINGS.peek().clone();
```

If `auto_format`, compute `kind` from the tab session (same as `format_active_tab`), `let sql = maybe_format_sql(sql, true, kind, &format_settings);` then if it changed, `replace_active_tab_sql` or write editor signal **without** awaiting. `format_sql` is sync. Do the same at the start of `run_batch_for_tab`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p ui line_number_labels_count_lines maybe_format_sql_is_noop_when_disabled`

Expected: PASS. `cargo check -p ui` PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/src/screens/workspace/components/sql_editor.rs ui/src/screens/workspace/actions.rs styles/components/_editor.scss
git commit -m "feat(ui): apply editor settings to SQL editor and run-format"
```

---

### Task 12: Wire result grid

**Files:**
- Modify: `models/src/settings.rs` or `models/src/customization.rs` — `format_null_display`
- Modify: `ui/src/screens/workspace/components/result_table.rs`
- Modify: `styles/components/_result-grid.scss`

**Interfaces:**
- Consumes: `APP_GRID_SETTINGS`, `NullDisplay`.
- Produces: virtual row height from settings; zebra class; wrap class; null renderer.

- [ ] **Step 1: Write the failing test**

In `models` (so it stays pure):

```rust
#[test]
fn format_null_display_modes() {
    assert_eq!(format_null_display("NULL", NullDisplay::Literal), "NULL");
    assert_eq!(format_null_display("NULL", NullDisplay::Empty), "");
    assert_eq!(format_null_display("null", NullDisplay::EmDash), "—");
    assert_eq!(format_null_display("hello", NullDisplay::EmDash), "hello");
    assert_eq!(format_null_display("", NullDisplay::Literal), "NULL");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p models format_null_display_modes`

Expected: FAIL — function missing.

- [ ] **Step 3: Implement**

```rust
pub fn format_null_display(raw: &str, mode: NullDisplay) -> String {
    let is_null = raw.is_empty() || raw.eq_ignore_ascii_case("null");
    if !is_null {
        return raw.to_string();
    }
    match mode {
        NullDisplay::Literal => "NULL".to_string(),
        NullDisplay::Empty => String::new(),
        NullDisplay::EmDash => "—".to_string(),
    }
}
```

In `result_table.rs` around the `virtual_row_height: f64 = 28.0` line:

```rust
let grid = APP_GRID_SETTINGS();
let virtual_row_height = f64::from(grid.row_height.clamp(18, 48));
```

When rendering cell text, pass `format_null_display(&cell_value, grid.null_display)` instead of raw `cell_value` for display (not for copy/edit — copy still uses the real value).

Row class: if `grid.zebra && row_index % 2 == 1` add `results__row--stripe`.

Cell class: if `grid.wrap_cells` add `results__cell--wrap`.

SCSS:

```scss
.results__row--stripe td {
  background: color-mix(in srgb, var(--color-primary) 6%, transparent);
}
.results__cell--wrap {
  white-space: normal;
  overflow-wrap: anywhere;
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p models format_null_display_modes`

Expected: PASS. `cargo check -p ui` PASS.

- [ ] **Step 5: Commit**

```bash
git add models/src/settings.rs ui/src/screens/workspace/components/result_table.rs styles/components/_result-grid.scss
git commit -m "feat(ui): apply grid row height, zebra, wrap, and null display"
```

---

### Task 13: Confirm-before drop/truncate

**Files:**
- Modify: `ui/src/screens/workspace/components/explorer/tree_views.rs`

**Interfaces:**
- Consumes: `APP_APP_BEHAVIOR`.
- Produces: `fn should_prompt_table_mutation(kind: TableMutationKind, behavior: &AppBehaviorSettings) -> bool`

- [ ] **Step 1: Write the failing test**

In `tree_views.rs` `#[cfg(test)]`:

```rust
#[test]
fn should_prompt_table_mutation_honors_flags() {
    let mut behavior = models::AppBehaviorSettings::default();
    assert!(should_prompt_table_mutation(TableMutationKind::Drop, &behavior));
    assert!(should_prompt_table_mutation(TableMutationKind::Truncate, &behavior));
    behavior.confirm_before_drop = false;
    assert!(!should_prompt_table_mutation(TableMutationKind::Drop, &behavior));
    assert!(should_prompt_table_mutation(TableMutationKind::Truncate, &behavior));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ui should_prompt_table_mutation_honors_flags`

Expected: FAIL — function missing.

- [ ] **Step 3: Implement**

```rust
fn should_prompt_table_mutation(
    kind: TableMutationKind,
    behavior: &models::AppBehaviorSettings,
) -> bool {
    match kind {
        TableMutationKind::Drop => behavior.confirm_before_drop,
        TableMutationKind::Truncate => behavior.confirm_before_truncate,
    }
}
```

At the start of `confirm_and_drop_table` / `confirm_and_truncate_table`, **before** any await:

```rust
let behavior = crate::app_state::APP_APP_BEHAVIOR.peek().clone();
let prompt = should_prompt_table_mutation(TableMutationKind::Drop, &behavior);
```

If `prompt` is false, skip the Yes/No dialog and run the mutation. If true, keep the existing dialog.

Do not hold a signal guard across `.await`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p ui should_prompt_table_mutation_honors_flags`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/src/screens/workspace/components/explorer/tree_views.rs
git commit -m "feat(ui): honor confirm-before drop and truncate settings"
```

---

### Task 14: Ollama keyring + settings window theme/density preview

**Files:**
- Modify: `services/src/app.rs` (`load_app_startup_settings`, `save_app_ui_settings_with_secrets`)
- Modify: `ui/src/windows/mod.rs` (`SettingsWindowRoot`)
- Modify: `ui/src/layout/settings_modal/sections.rs` (Ollama fields if not finished in Task 8)

**Interfaces:**
- Consumes: `storage::load_lm_api_key` / `save_lm_api_key`.
- Produces: Ollama API key hydrated/saved with service `"shovel.ollama"`; settings window shell has theme + density classes and injects `theme_overrides.to_css()`.

- [ ] **Step 1: Write the failing test**

In `models` or `services`, skip a keyring test. Add:

```rust
#[test]
fn ollama_keyring_service_name_is_stable() {
    assert_eq!(models::OllamaSettings::keyring_service(), "shovel.ollama");
}
```

Add on `OllamaSettings`:

```rust
pub fn keyring_service() -> &'static str {
    "shovel.ollama"
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p models ollama_keyring_service_name_is_stable`

Expected: FAIL — method missing.

- [ ] **Step 3: Implement**

Hydrate in `load_app_startup_settings` after deepseek:

```rust
hydrate_secret(
    &mut ui_settings.ollama.api_key,
    storage::load_lm_api_key(models::OllamaSettings::keyring_service()).await?,
    |value| async move {
        storage::save_lm_api_key(models::OllamaSettings::keyring_service(), value).await
    },
)
.await?;
```

The `hydrate_secret` save-legacy closure currently takes `Fn(String) -> Fut`. Match existing codestral usage; if the generic does not fit a two-arg save, add `load_ollama_api_key`/`save_ollama_api_key` in `storage/src/settings.rs` copying the DeepSeek pair with service `shovel.ollama`. Prefer dedicated functions if the closure fight is real.

In `save_app_ui_settings_with_secrets`, also save `settings.ollama.api_key`.

`SettingsWindowRoot`:

```rust
let theme_class = ui().theme.css_class().to_string();
let density_class = ui().density.css_class();
let theme_css = ui().theme_overrides.to_css();
rsx! {
    document::Style { {APP_CSS.to_string()} }
    if !theme_css.is_empty() {
        style { {theme_css} }
    }
    div { class: "settings-window-shell {theme_class} {density_class}",
        SettingsModal { ... }
    }
}
```

Ollama section: same layout as DeepSeek (enabled toggle disabled when key empty, password API key, base URL, model).

- [ ] **Step 4: Verify**

Run: `cargo test -p models ollama_keyring_service_name_is_stable`

Expected: PASS. `cargo check -p ui -p services` PASS.

- [ ] **Step 5: Commit**

```bash
git add models/src/settings.rs services/src/app.rs storage/src/settings.rs ui/src/windows/mod.rs ui/src/layout/settings_modal/sections.rs
git commit -m "feat(ui): persist Ollama key and preview theme overrides in settings"
```

---

### Task 15: Workspace CI gate

**Files:** none new.

**Interfaces:** none.

- [ ] **Step 1: Format**

Run: `cargo fmt --all`

- [ ] **Step 2: Clippy**

Run: `cargo clippy --workspace --all-targets -- -D warnings`

Expected: no warnings. Fix any from this work (unused imports after the settings_modal split, type changes).

- [ ] **Step 3: Tests**

Run: `cargo test --workspace`

Expected: PASS.

- [ ] **Step 4: Confirm CSS rebuilt**

`app/assets/app.css` must contain `.settings-window-shell .settings-modal` and `max-width: 560px`. If not, `cargo build -p app --features desktop` to run `build.rs`.

- [ ] **Step 5: Commit leftover fmt/css**

```bash
git add app/assets/app.css
git commit -m "chore: format and rebuild settings stylesheet"
```

Only if those files actually changed.

---

## Spec coverage

| Spec requirement | Task |
|---|---|
| Persist nested fields + serde defaults | 1 |
| ThemeOverrides live CSS names + hex | 2 |
| Shortcut defaults/conflict | 3, 5, 9 |
| config.toml overlay | 4 |
| Window 960×720, breakpoint 560, densified chrome | 6 |
| Widgets | 7 |
| Fill tabs, Reset UI keeps keys not shortcuts | 8 |
| Keyboard editor | 9 |
| sync globals | 10 |
| SQL editor + auto-format | 11 |
| Grid row height/zebra/null/wrap | 12 |
| Confirm drop/truncate | 13 |
| Ollama GUI + keyring + settings preview | 14 |
| fmt/clippy/test | 15 |
| No chart/ER pages, no toml writes | honored by omission |

## Placeholder scan

None of TBD / implement later / similar-to-Task-N without code.

## Type consistency

- `EditorSettings` / `GridSettings` / `AppBehaviorSettings` names are identical in Tasks 1, 4, 8, 10–13.
- `APP_EDITOR_BEHAVIOR` becomes `EditorSettings` in Task 10; Task 11 reads `.font_size` on that type.
- `combo_conflict(action_id, combo, effective) -> Option<String>` is the Task 3 signature used in Task 9.
- `parse_hex_color` Task 2 is used by `ColorField` in Task 7.
- `OllamaSettings::keyring_service()` is `"shovel.ollama"` in Task 14.
