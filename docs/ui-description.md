# Shovel UI — Полное описание интерфейса

> **Для агентов:** этот документ — единый источник правды о том, как устроен
> пользовательский интерфейс Shovel. Читайте его перед любой работой в `ui/`,
> чтобы понимать, какие экраны, панели, сигналы и потоки данных существуют и
> как они связаны. Он описывает **текущее состояние дерева** (не целевую
> архитектуру из планов рефакторинга).

Дата актуализации: 2026-08-25.

---

## 1. Обзор

Shovel — нативный десктопный клиент баз данных (SQLite, PostgreSQL, MySQL,
ClickHouse), написанный на Rust (nightly, edition 2024) с Dioxus Desktop 0.7.3.
UI живёт в крейте `ui`, доменные модели — в `models`, персистентность — в
`storage`, операции с БД — в `connection`/`query`/`explorer`/`acp`.

**Ключевые факты о стеке UI:**

- Только Dioxus 0.7 API: `use_signal`, `use_resource`, `use_effect`,
  `use_memo`, `#[component]`. Никаких `cx`, `Scope`, `use_state`.
- Глобальное состояние живёт в `GlobalSignal` в `ui/src/app_state.rs`.
  Локальное состояние — в `use_signal` внутри компонентов.
- `ui` может импортировать только `models` и `services`. Операционные вызовы
  идут через фасад `services` (хотя местами `ui` всё ещё тянет нижние крейты
  напрямую — это известный долг, не считайте рефакторинг завершённым).
- Никогда не держите чтение/запись сигнала через `.await` (clippy
  `await-holding-invalid-types`). Снимите снапшот в owned-локальную переменную
  до `await`, затем заново возьмите блокировку после.

---

## 2. Структура экранов

Приложение имеет два основных экрана, переключаемых через `APP_STATE`:

| Экран | Компонент | Когда показывается |
|-------|-----------|--------------------|
| **Connect** | `DbConnect` (`ui/src/screens/connect/mod.rs`) | Нет сессий, или `show_connection_screen == true` |
| **Workspace** | `Workspace` (`ui/src/screens/workspace/mod.rs`) | Есть хотя бы одна сессия |

Корневой компонент — `App` (`ui/src/app.rs`). Он:

1. Загружает стартовые настройки (`services::load_app_startup_settings`) через
   `use_resource`.
2. Применяет их к глобальным сигналам (`replace_ui_settings`,
   `APP_SQL_FORMAT_SETTINGS`).
3. Если `restore_session_on_launch` — восстанавливает сохранённые сессии и
   таб-драфты.
4. Персистит изменения настроек обратно на диск из `use_effect`.
5. Рендерит: `Toolbar`, `main` (Connect или Workspace), `StatusBar`,
   `ToastContainer`, `ContextMenu`, `CommandPalette`, `GlobalSearch`,
   `TextInputMenuInit`.

### 2.1 Обёртка `App`

```text
div.app.{theme}.{density}
├── Toolbar (layout/toolbar.rs)
├── main.app__body
│   ├── Workspace (или DbConnect)
│   └── app__tooltip-layer (если есть тултип)
├── ToastContainer (layout/toast.rs)
├── StatusBar (layout/status_bar.rs)
├── TextInputMenuInit
├── ContextMenu (components/context_menu.rs)
├── CommandPalette (components/command_palette.rs)
└── GlobalSearch (components/global_search.rs)
```

`Workspace` обёрнут в `ErrorBoundary` — при панике рендера показывается
заглушка «Something went wrong».

---

## 3. Глобальное состояние (`ui/src/app_state.rs`)

Правило: **любое состояние, которое наблюдают несколько экранов, живёт в
глобальном сигнале.** Всё чисто локальное — в `use_signal` внутри компонента.

### 3.1 Основные глобальные сигналы

| Сигнал | Тип | Назначение |
|--------|-----|-----------|
| `APP_STATE` | `AppState` | Сессии, активная сессия, флаг показа connect-экрана |
| `APP_THEME` | `String` | CSS-класс темы (`dark`/`light`) |
| `APP_UI_DENSITY` | `UiDensity` | Плотность интерфейса |
| `APP_UI_SETTINGS` | `AppUiSettings` | Все персистентные UI-настройки |
| `APP_SQL_FORMAT_SETTINGS` | `SqlFormatSettings` | Настройки форматирования SQL |
| `APP_AI_FEATURES_ENABLED` | `bool` | Включены ли AI-фичи |
| `APP_AI_AUTO_APPLY_COMPLETIONS` | `bool` | Авто-вставка inline-завершений |
| `APP_READ_ONLY_MODE` | `bool` | Read-only режим |
| `APP_SHOW_SAVED_QUERIES` | `bool` | Видимость панели Saved Queries |
| `APP_SHOW_CONNECTIONS` | `bool` | Видимость панели Connections |
| `APP_SHOW_EXPLORER` | `bool` | Видимость панели Explorer |
| `APP_SHOW_HISTORY` | `bool` | Видимость панели History |
| `APP_SHOW_SQL_EDITOR` | `bool` | Видимость SQL-редактора |
| `APP_SHOW_AGENT_PANEL` | `bool` | Видимость панели Agent |
| `APP_SHOW_BOTTOM_PANEL` | `bool` | Видимость нижнего дока |
| `APP_BOTTOM_PANEL_HEIGHT` | `f64` | Высота нижнего дока (px) |
| `APP_SPLIT_MODE` | `WorkspaceSplitMode` | Режим сплита редактора/результата |
| `APP_TOOLTIP` | `Option<AppTooltip>` | Текущий тултип |
| `APP_TOAST` | `Vec<AppToast>` | Очередь тостов |
| `APP_TAB_DRAFTS` | `Vec<TabDraft>` | Сохранённые SQL-драфты вкладок |
| `APP_COLLAPSED_PANELS` | `Vec<WorkspaceToolPanel>` | Свёрнутые панели (in-memory) |
| `APP_RECENTLY_CLOSED_TABS` | `Vec<QueryTabState>` | Стек «Reopen Closed Tab» (до 8) |
| `APP_LAST_QUERY` | `Option<LastQuerySummary>` | Сводка последнего запроса для статус-бара |
| `APP_FOCUS_EDITOR_REQUEST` | `u64` | Счётчик запроса фокуса редактора |
| `APP_FOCUS_FILTER_PANEL_REQUEST` | `u64` | Счётчик запроса фокуса фильтров |
| `APP_FOCUS_AGENT_COMPOSER_REQUEST` | `u64` | Счётчик запроса фокуса композера агента |
| `APP_EXPLORER_SELECTED_NODE` | `String` | Выбранный узел дерева (qualified name) |
| `APP_COMMAND_PALETTE` | `bool` | Видимость палитры команд |
| `APP_COMMAND_REQUEST` / `APP_COMMAND_REQUEST_KIND` | `u64` | Канал команд из палитры в workspace |
| `APP_GLOBAL_SEARCH_*` | — | Канал глобального поиска (Ctrl+K) |

### 3.2 Синхронизация настроек

Все `set_*`-переключатели проходят через `update_ui_settings`, который пишет
`APP_UI_SETTINGS` и затем `sync_runtime_ui_settings` — зеркалит значения в
отдельные сигналы (`APP_THEME`, `APP_SHOW_*`, `APP_SPLIT_MODE` и т.д.).
**Важно:** зеркальные записи equality-guarded (`sync_bool` и др.), чтобы
переключение одной панели не перерисовывало всё `.app`-поддерево.

### 3.3 Сессии

- `APP_STATE` — единственный источник правды для живых сессий.
- Добавление/удаление сессии всегда через `add_session`/`remove_session`,
  чтобы SSH-туннели и on-disk состояние оставались согласованными.
- `remove_session` освобождает SSH-туннели через `services::release_ssh_tunnel`.
- `restore_connection_sessions` восстанавливает сессии + таб-драфты на старте.
- `persist_session_state` выгружает запись на диск в `spawn_blocking`.

### 3.4 Кэш эксплорера

`EXPLORER_CACHE` — `HashMap<session_id, sections>` с TTL 5 минут. Функции
`get_cached_explorer`/`cache_explorer` в `app_state`. Устаревшие записи
вычищаются при вставке.

---

## 4. Экран Connect (`ui/src/screens/connect/`)

`DbConnect` — экран подключения. Показывается как полноэкранный (нет сессий)
или как оверлей поверх workspace (кнопка «New Connection»).

Структура:

```text
section.connect-screen
└── div.connect-screen__panel
    ├── div.connect-screen__hero        (заголовок + «Back to Workspace»)
    ├── RecentConnections               (recent_connections.rs)
    ├── [MockDataToggle]                (только debug-сборка)
    └── div.connect-screen__section
        ├── KindSelector                (kind_selector.rs)
        └── {SqliteForm|PostgresForm|MySqlForm|ClickHouseForm}
```

- `KindSelector` выбирает тип БД (`DatabaseKind`).
- Формы (`forms/`) собирают параметры подключения. Для Postgres/MySQL/
  ClickHouse есть `ssh_tunnel.rs` (SSH-туннель).
- `RecentConnections` показывает сохранённые подключения, загружаемые через
  `services::load_saved_connections`.
- `MockDataToggle` (debug) — загружает фейковую сессию для UI-работы без БД.

---

## 5. Экран Workspace (`ui/src/screens/workspace/`)

Это главный экран. Разбит на модули:

| Файл | Назначение |
|------|-----------|
| `mod.rs` | Корневой `Workspace`, компоновка, клавиатурный диспетчер |
| `actions.rs` | Все операции над вкладками (запуск, пагинация, фильтры, правка) |
| `context.rs` | Контексты (tab/query/acp), прокидываемые через `provide_context` |
| `helpers.rs` | Утилиты (ресайз-скрипты, ER-диаграмма, форматирование длительности) |
| `chat.rs` | Создание/удаление/выбор чат-тредов |
| `hooks/` | `use_query_tabs`, `use_explorer_state`, `use_chat_state`, `use_acp_state` |
| `components/` | Все панели и виджеты |

### 5.1 Компоновка `Workspace`

`Workspace` собирает состояние из четырёх хуков:

```rust
let ExplorerState { tree_status, tree_sections, tree_reload } = use_explorer_state();
let QueryTabsState { tabs, active_tab_id, next_tab_id } = use_query_tabs();
let ChatState { chat_threads, active_chat_thread_id, chat_revision, history, next_history_id, saved_queries, next_saved_query_id, .. } = use_chat_state(...);
let AcpState { acp_panel_state, allow_agent_*, .. } = use_acp_state(...);
```

Затем прокидывает три контекста:
- `WorkspaceTabContext { tabs, active_tab_id, next_tab_id }`
- `WorkspaceQueryContext { history, next_history_id, saved_queries, next_saved_query_id }`
- `WorkspaceAcpContext { acp_panel_state, chat_revision, allow_agent_*, chat_threads, active_chat_thread_id, connection_label }`

И рендерит `WorkspaceBody`.

### 5.2 `WorkspaceBody` — каркас

```text
div.workspace
├── div.workspace__top-row
│   ├── aside.workspace__sidebar        (если show_sidebar)
│   │   └── WorkspaceDock (Sidebar)
│   ├── div.workspace__resize-handle    (горизонтальный ресайз сайдбара)
│   └── section.workspace__main
│       ├── header.workspace__header
│       │   └── div.workspace__toolbar  (View menu, Refresh, ER, New Connection)
│       └── div.workspace__content
│           ├── div.workspace__canvas
│           │   └── TabsManager
│           └── [aside.workspace__inspector + resize-handle]  (если show_inspector)
└── [div.workspace__resize-handle--bottom + BottomPanelDock]  (если show_bottom_panel)
```

**Тулбар workspace** (в `WorkspaceBody`):
- **View menu** (иконка меню) — открывает контекстное меню видимости панелей:
  Saved Queries, Connections, Explorer, History, SQL Editor, Agent Panel,
  Bottom Dock, Editor layout (split mode). Кнопка «активна», пока любая панель
  видима.
- **Refresh explorer** — `tree_reload += 1`.
- **ER diagram** — загружает foreign keys и открывает отдельное OS-окно с
  диаграммой (`windows::open_er_diagram_window`).
- **New connection** — `open_connection_screen()`.

### 5.3 Док-система панелей

Панели инструментов (Explorer, Connections, Saved Queries, History, Agent)
можно перетаскивать между **левым сайдбаром** и **правым инспектором**.

Компоненты:
- `WorkspaceDock(dock, panels, ...)` — контейнер для одной стороны.
- `WorkspaceDockPanel` — одна панель с «ручкой» перетаскивания (grip).
- `WorkspaceDropSlot` — зона сброса между панелями.
- `WorkspacePanelContent` — `match` по `WorkspaceToolPanel`, рендерит нужную
  панель.

Логика перетаскивания:
- `dragging_panel: Signal<Option<WorkspaceToolPanel>>` — какая панель тянется.
- `drop_target: Signal<Option<DockDropTarget>>` — куда сбросить.
- `onmouseup` на корне `Workspace` вызывает `apply_tool_panel_drop`.
- Раскладка панелей персистится в `AppUiSettings::tool_panel_layout`.

Каждая панель может быть **свёрнута** до заголовка через
`toggle_panel_collapsed` (in-memory, `APP_COLLAPSED_PANELS`).

### 5.4 Ресайз

Ресайз сайдбара/инспектора/нижнего дока идёт через JS-скрипты
(`workspace_resize_script`, `workspace_vertical_resize_script`) и `document::eval`,
который возвращает новую ширину/высоту. Значения пишутся в CSS-переменные
(`--workspace-sidebar-width`, `--workspace-inspector-width`,
`--workspace-bottom-panel-height`) на корне `Workspace`.

---

## 6. Вкладки и `TabsManager` (`components/tabs.rs`)

### 6.1 Модель вкладки

Вкладка — `models::QueryTabState` (см. `models/src/query.rs`). Это **один
монолитный struct** (в текущем дереве), содержащий:

- `id`, `session_id`, `title`, `pinned`
- `sql`, `status`
- `result: Option<QueryOutput>` (Table | AffectedRows)
- `current_offset`, `page_size`
- `last_run_sql`, `preview_source`
- `filter`, `sort`
- `tab_kind` (Query | TablePreview | Structure)
- `is_loading_more`
- `pending_table_changes`
- `execution_plan`, `show_execution_plan`
- `batch_results`, `batch_outputs`
- `last_duration_ms`

> **Примечание:** план рефакторинга
> (`docs/superpowers/plans/2026-08-25-responsiveness-architecture.md`) предлагает
> разбить это на 4 независимых сигнала (meta/editor/result/pending). Пока это
> **не реализовано** — в дереве один `Signal<Vec<QueryTabState>>`.

### 6.2 `use_query_tabs` (`hooks/use_query_tabs.rs`)

Возвращает `QueryTabsState { tabs, active_tab_id, next_tab_id }`.

Два `use_effect`:
1. **Нормализация вкладок:** удаляет вкладки, чья сессия исчезла; гарантирует
   активную вкладку для активной сессии; при необходимости создаёт вкладку из
   сохранённого `TabDraft` (или дефолт «Query 1» / `select 1 as id;`).
2. **Персистентность драфтов:** пишет `APP_TAB_DRAFTS` из непустых вкладок
   (кроме дефолтного SQL).

### 6.3 `TabsManager`

Рендерит:
- **Таббар** (`div.tabbar`): по одной вкладке на `QueryTabState`. Клик —
  активация + `activate_session`. Средняя кнопка мыши / кнопка «x» — закрытие.
  Двойной клик по заголовку — переименование. Правый клик — контекстное меню
  (Close, Close Others, Close to Right, Close All, Reopen Closed Tab, Pin).
  Кнопка «+ Tab» — новая вкладка.
- **Тело активной вкладки** — выбирает, что показать:
  - `SqlEditor` (если `APP_SHOW_SQL_EDITOR`)
  - Ресайз-хендл редактора (вертикальный или горизонтальный в зависимости от
    `APP_SPLIT_MODE`)
  - Панель действий редактора (Run / Format / Explain / More)
  - Область результатов (`div.workspace__results`):
    - Generate SQL окно (если открыто)
    - `BatchResultsView` (если `batch_results.is_some()`)
    - `ExecutionPlanView` (если `show_execution_plan`)
    - `TableEditor` (если `tab_kind == TablePreview`)
    - `ResultTable` (по умолчанию)

### 6.4 Split-режим редактора

`APP_SPLIT_MODE` (`WorkspaceSplitMode`): `Off` (стек вертикально), `Horizontal`
(две колонки), `Vertical` (стек с явным разделителем). CSS-классы
`editor-shell--split-horizontal` / `--split-vertical`. Ресайз редактора через
`editor_height`/`editor_width` сигналы.

---

## 7. SQL-редактор (`components/sql_editor.rs`)

`SqlEditor` — текстовый редактор SQL с подсветкой, автодополнением и
inline-завершениями.

### 7.1 Ключевые сигналы

- `draft_sql` — текущий SQL (локальный).
- `editor_selection` — выделение.
- `editor_revision` — счётчик изменений (для дебаунса).
- `is_typing` — флаг активного ввода.
- `completion_runtime` — состояние inline-завершений.
- `has_synced_editor_dom` / `synced_editor_tab_id` — синхронизация DOM.

### 7.2 Поток ввода

- `oninput` пишет значение в `draft_sql` и `sync_active_tab_sql_draft`.
- `use_effect` с дебаунсом ~90мс читает значение из DOM
  (`editor_value_and_selection_query_script`) и синхронизирует в `tabs`.
- При смене вкладки/внешнем изменении SQL (Format/Generate) `use_effect`
  пишет значение в textarea через `set_editor_value_script`.

### 7.3 Автодополнение

- `CompletionService` (из `APP_UI_SETTINGS`) — CodeStral/DeepSeek inline
  completion.
- Дебаунс `COMPLETION_DEBOUNCE_MS = 180`.
- Inline-завершение показывается как ghost-текст; принимается по **Tab** или
  авто-вставкой после `AUTO_APPLY_IDLE_MS = 400` (если
  `ai_auto_apply_completions`).
- `apply_inline_completion` вставляет текст, обновляет DOM и `tabs`.

### 7.4 Контекстное меню редактора

Правый клик открывает меню: Copy/Cut/Paste/Select All (через
`document.execCommand`), Clear, Toggle comment, Format SQL, Run query, Explain
query, Explain with AI (если AI включён), Save as saved query.

### 7.5 Клавиатурные шорткаты редактора

- `Ctrl+Enter` — Run
- `Ctrl+Shift+F` — Format
- `Ctrl+Shift+E` — Explain
- `Ctrl+/` — Toggle comment
- `Ctrl+S` — Save as saved query
- `Ctrl+L` — Clear editor
- `Tab` — принять inline-завершение

---

## 8. Результаты (`components/result_table.rs`)

`ResultTable` — виртуализированная таблица результатов. Это самый большой
компонент.

### 8.1 Режимы просмотра

`ResultViewMode`: `Table`, `Records`, `Single`, `Details`. Переключаются в
тулбаре результатов.

### 8.2 Тулбар результатов

- Чип сводки строк (`rows_toolbar_summary`).
- Чип статуса (если есть).
- Мета: «N selected» / сводка pending-изменений / «Select a row for details».
- Кнопки режима просмотра.
- **Quick filter** (если `filter_enabled`).
- **Filters** (панель фильтров).
- **Previous / Next** (пагинация).
- Если `page.editable.is_some()`: **Insert draft row**, **Apply pending
  changes**, **Discard pending changes**, **Delete selected row** (все
  блокируются в read-only).
- **More actions**: Show/Hide row details, Show/Hide chart, Pin for compare,
  Compare with pinned (открывает окно data-diff).

### 8.3 Виртуализация

- `display_rows_cache` — `use_memo`, материализует строки из `result` +
  `pending_table_changes`.
- Виртуальный скролл: `virtual_row_height = 28`, буфер `virtual_buffer = 10`.
- `scroll_offset` / `viewport_height` управляют видимым диапазоном.

### 8.4 Фильтры

- `filter_draft` — локальный черновик фильтра.
- `filter_panel_open` — видимость панели.
- `quick_filter_*` — быстрый фильтр (одна колонка + оператор + значение).
- Операторы: Contains, NotContains, Equals, NotEquals, StartsWith, EndsWith,
  IsNull, IsNotNull.
- Режим: AND / OR.
- Применение через `apply_active_tab_filter` / `clear_active_tab_filter`.

### 8.5 Сортировка

`toggle_active_tab_sort` — переключает сортировку по колонке (asc/desc).

### 8.6 Редактирование ячеек

- `editing_cell`, `value_editor`, `value_editor_target` — состояние редактора
  значения.
- `commit_cell_edit` — применяет правку в `pending_table_changes`.
- `insert_empty_row`, `apply_pending_changes`, `discard_pending_changes`,
  `delete_selected_row` — операции над pending-изменениями.
- `pending_changes` отображаются как draft-строки/ячейки в сетке.

### 8.7 Состояния

- `ResultsStateBlock` — пустое/ошибочное состояние с Retry/Run again.
- Скелетон загрузки при `status.starts_with("Loading"/"Running"/"Preview")`.

---

## 9. Table Editor (`components/table_editor.rs`)

Показывается для вкладок `WorkspaceTabKind::TablePreview`. Обёртка с
суб-вкладками:

| Суб-вкладка | Компонент |
|-------------|-----------|
| Data | `ResultTable` |
| Structure | `StructurePanel` |
| DDL | `DdlPanel` |
| Indexes | `IndexesPanel` |
| Relations | `RelationsPanel` |

Панели Structure/DDL/Indexes/Relations ленивые — грузят данные при первом
выборе и кэшируют.

---

## 10. Панели инструментов

### 10.1 Explorer (`components/explorer/mod.rs`)

`SidebarConnectionTree` — дерево схем/таблиц/колонок.

- Заголовок: «Entities» + счётчик + кнопка **Create table** (открывает
  отдельное OS-окно).
- Поле **Filter entities** — фильтрация по имени/qualified name.
- `tree_views::ExplorerConnectionView` — рендер одной сессии.
- Группы объектов: Tables, Columns, Views, Materialized Views, Sequences,
  Functions, Procedures, Triggers (порядок фиксирован, пустые/отключённые
  пропускаются).
- `filter_system_schemas` — скрывает системные схемы (pg_catalog,
  information_schema, mysql, sys, system) если `show_system_objects == false`.
- Двойной клик по таблице — открывает preview-вкладку.
- Контекстное меню: Preview, Structure, Rename, Drop, Copy name, и т.д.
- Клавиатура: F2 — rename, Delete — drop (через `APP_EXPLORER_SELECTED_NODE`).

### 10.2 Connections (`components/session_rail.rs`)

`SessionRail` — список активных сессий.

- Каждая сессия: kind-бейдж, имя, кнопка Disconnect.
- Клик — `activate_session`.
- Правый клик — контекстное меню (Disconnect).
- Кнопка **Add** — `open_connection_screen()`.

### 10.3 Saved Queries (`components/saved_queries.rs`)

`SavedQueriesPanel` — сохранённые запросы и сниппеты.

- Форма сохранения: поле названия + «Save Snippet» / «Save Query».
- Список: title, kind-бейдж, connection, SQL, кнопки Load/Insert + Delete.
- Контекстное меню: Load/Insert in tab, Copy SQL, Copy title, Delete.
- `SavedQueryKind`: `Snippet` (вставка в конец) vs `Query` (замена SQL).

### 10.4 History (`components/history.rs`)

`QueryHistoryPanel` — история выполненных запросов.

- Поиск по SQL-тексту (FTS5 через `QueryHistoryStore::search`).
- Фильтры: дата (All/Today/Week/Month), connection, outcome (All/Success/Error).
- Пагинация по 50 записей.
- Экспорт в CSV.
- Контекстное меню: Load in tab, Copy SQL, Copy redacted SQL, Activate
  connection, Delete entry, Clear all history.
- Секреты в SQL маскируются (`redact_sql_display`).

### 10.5 Agent Panel (`components/agent_panel/`)

`AcpAgentPanel` — чат с ACP-агентом (DeepSeek/Ollama/registry/custom).

- **Заголовок:** title треда, meta, кнопки Dialogs, Cancel, Disconnect.
- **Dialogs popover:** список чат-тредов, New chat, Delete.
- **Сообщения:** рендер markdown, код-карточки с SQL-действиями (Run, Insert,
  Copy), артефакты (SQL draft, query summary), streaming-каретка.
- **Permission-запросы:** блок с опциями Allow/Deny.
- **Composer:** textarea + Send (Enter / Ctrl+Enter).
- **Setup (не подключено):** выбор режима — DeepSeek (API key), Ollama
  (embedded), OpenCode/Codex (registry), Custom.

Состояние панели — `AcpPanelState` (в `models`), события применяются через
`apply_acp_events` в `state.rs`.

### 10.6 Bottom Dock (`components/bottom_panel.rs`)

`BottomPanelDock` — нижний док с 5 вкладками:

| Вкладка | Содержимое |
|---------|-----------|
| Output | Сводка последнего запроса |
| Messages | Зеркало тостов |
| Query Log | Последние 40 записей истории |
| Transactions | Плейсхолдер |
| Problems | Плейсхолдер |

---

## 11. Тулбар приложения (`layout/toolbar.rs`)

`Toolbar` — верхняя полоса окна:

- **Drag-зона** (перетаскивание окна, двойной клик — максимизация).
- **Бренд:** логотип + «Shovel / Database Client».
- **Connection label:** «{name} active · {n} open» или «No active connection».
- **Действия:** New Connection (или Back to Workspace), Settings.
- **Window controls:** Minimize, Maximize, Close.

`open_settings()` открывает нативное OS-окно настроек и мостит изменения
обратно в глобальные сигналы через `DialogBridge`.

---

## 12. Статус-бар (`layout/status_bar.rs`)

`StatusBar` — нижняя полоса:

- Connection label: «{name} · {kind}».
- «Sessions {n}».
- «Last: {label} · {duration}» (из `APP_LAST_QUERY`), красный при ошибке.

---

## 13. Настройки (`layout/settings_modal.rs`)

`SettingsModal` — проп-управляемая форма (не владеет глобальным состоянием).
Монтируется и как in-app оверлей, и в отдельном OS-окне.

Категории (левая навигация):

| Категория | Содержимое |
|-----------|-----------|
| Appearance | Тема (Dark/Light), плотность (Compact/Comfortable) |
| Database | Плейсхолдер |
| Editor | SQL Formatting, CodeStral Completion |
| Grid | Плейсхолдер |
| Navigation | Плейсхолдер |
| Advanced | DeepSeek Agent, Workspace |

**Workspace section** содержит:
- Reset UI (сброс к дефолтам, сохраняя API-ключи).
- Default page size.
- Restore session on launch.
- Read-only mode.
- Explorer view (show_schemas/tables/views/columns/system_objects/row_counts/
  sort_alphabetical).
- Visible panels by default (saved_queries/connections/explorer/history/
  sql_editor/agent_panel/bottom_panel).
- Editor/result split mode.
- AI features (enable, response language, auto-apply completions).

---

## 14. Оверлеи

### 14.1 Command Palette (`components/command_palette.rs`)

- Открывается `Ctrl+Shift+P`.
- Каталог команд в `app_state/commands.rs`.
- Диспетчеризация через `APP_COMMAND_REQUEST`/`APP_COMMAND_REQUEST_KIND` —
  workspace слушает счётчик в `use_effect` и выполняет действие.

### 14.2 Global Search (`components/global_search.rs`)

- Открывается `Ctrl+K`.
- Ищет по вкладкам, объектам эксплорера, действиям.
- Снапшоты вкладок/дерева кладутся в `APP_GLOBAL_SEARCH_*` при открытии.
- Выбор результата — через `APP_GLOBAL_SEARCH_REQUEST` + kind + payload.

### 14.3 Context Menu (`components/context_menu.rs`)

- `open_context_menu(x, y, items)` — открывает меню.
- `ContextMenuItem` — элемент с иконкой, disabled/danger/separator/active.
- Позиционирование viewport-aware.

### 14.4 Toast (`layout/toast.rs`)

- `show_toast(message, kind)` — Info/Success/Warning/Error.
- Авто-скрытие через 5 секунд (с `CancellationToken` для отмены).
- `dismiss_toast(id)` — ручное закрытие.

### 14.5 Tooltip (`components/tooltip_target.rs`)

- `TooltipTarget` — обёртка, показывающая тултип при наведении.
- `show_tooltip`/`hide_tooltip` пишут `APP_TOOLTIP`.

---

## 15. Клавиатурные шорткаты

Диспетчер в `Workspace` (`onkeydown` на корне) + `app_state/keyboard.rs`.
Единая чистая функция `match_key_combination(key, mods)` возвращает
`ShortcutAction`; вызывающие сайты реализуют действие против своих сигналов.

**Корневой диспетчер (workspace):**

| Шорткат | Действие |
|---------|---------|
| `Ctrl+Shift+F` | Format SQL |
| `Ctrl+Shift+S` | Save as saved query |
| `Ctrl+Shift+P` | Command palette |
| `Ctrl+Shift+M` | Focus agent composer |
| `Ctrl+Shift+N` | New connection |
| `Ctrl+K` | Global search |
| `Ctrl+T` / `Ctrl+N` | New tab |
| `Ctrl+W` | Close tab |
| `Ctrl+Tab` | Next tab |
| `Ctrl+F` | Focus result filter panel |
| `Ctrl+E` | Focus editor |
| `Ctrl+,` | Open settings |
| `F5` | Refresh explorer |
| `F2` | Rename selected explorer object |
| `Delete` | Drop selected explorer table |
| `Esc` | Close topmost overlay |

**Локальные обработчики (не в корневом диспетчере):**
- `Ctrl+Enter` — Run query (обрабатывает textarea SQL-редактора).
- `Ctrl+Shift+E` — Explain query / Explain with AI (обрабатывает редактор;
  корневой матчер возвращает `None`, чтобы не срабатывать дважды).
- `Ctrl+S` — Save (локально в редакторе; корневой матчер оставляет его).
- `Ctrl+L` — Clear editor, `Ctrl+/` — Toggle comment (контекстное меню
  редактора).

Каталог действий — `app_state/actions.rs` (`ActionId`, `dispatch_action`).
Палитра-видимые действия резолвятся через `payload_to_action_id`.
`ShortcutAction::to_action_id()` — единственное место «клавиша = действие».

---

## 16. Потоки данных (важные паттерны)

### 16.1 Запуск запроса

1. Пользователь жмёт **Run** (в `TabsManager`).
2. `run_query_for_tab(tabs, current_id, connection, sql, offset, page_size, history)`.
3. Если SQL многооператорный (>1 stmt) → `run_batch_for_tab`.
4. Если read-only блокирует → статус-сообщение.
5. Снимает `filter`/`sort` из вкладки, ставит статус «Running...».
6. `spawn(async move { services::execute_query_page(...).await })`.
7. После `await` пишет `result`/`status`/`current_offset`/`last_duration_ms`
   обратно во вкладку.
8. Пишет `APP_LAST_QUERY` для статус-бара.
9. Добавляет запись в историю (`history` + `services::append_query_history`).

### 16.2 Команды из палитры

Палитра не может трогать локальные сигналы workspace. Она бампает
`APP_COMMAND_REQUEST` + `APP_COMMAND_REQUEST_KIND`. Workspace слушает в
`use_effect` и выполняет действие против `tabs`/`active_tab_id`/`history`.

### 16.3 Глобальный поиск

Аналогично: снапшоты в `APP_GLOBAL_SEARCH_*`, выбор — через
`APP_GLOBAL_SEARCH_REQUEST` + kind + payload. Workspace реализует выбор
(открыть вкладку / объект / действие).

### 16.4 ACP-события

`use_acp_state` опрашивает ACP-рантайм, применяет `AcpEvent` через
`apply_acp_events` к `AcpPanelState`, бампает `chat_revision`. Панель
перерисовывается по `chat_revision`.

---

## 17. Персистентность

| Что | Файл | Когда |
|-----|------|-------|
| UI-настройки | `app_ui_settings.json` | Из `use_effect` в `app.rs` |
| SQL-формат | `sql_format_settings.json` | Из `use_effect` в `app.rs` |
| Сессии | `session_state.json` | `persist_session_state` |
| Таб-драфты | в `session_state.json` | `use_effect` в `use_query_tabs` |
| История | `query_history.json` + FTS5 | При каждом запросе |
| Saved queries | `saved_queries.json` | При сохранении/удалении |
| Connections | `saved_connections.json` | При сохранении подключения |

Секреты (пароли, API-ключи) — в системном keyring (`shovel.connections`),
не в JSON.

---

## 18. Известные долги и горячие точки

- **`Signal<Vec<QueryTabState>>` монолитен** — план разбить на 4 сигнала
  (см. `docs/superpowers/plans/2026-08-25-responsiveness-architecture.md`).
- **`document::eval` в горячем пути** ввода/ресайза — медленный round-trip.
- **Глубокая прокидывание пропсов** через `WorkspaceBody → WorkspaceDock →
  WorkspaceDockPanel → WorkspacePanelContent`.
- **Тяжёлые синхронные операции** (format_sql, export, ER-диаграмма,
  materialize_display_rows) местами блокируют render-цикл.
- **`services`-фасад не используется как реальная граница** — `ui` местами
  тянет нижние крейты напрямую.
- Горячие файлы (большие, редактировать осторожно): `workspace/mod.rs`,
  `result_table.rs`, `explorer/create_table_modal.rs`, `query-core/src/lib.rs`,
  `acp/src/runtime.rs`, `acp/src/introspection.rs`.

---

## 19. Как добавить новую персистентную UI-настройку

Трогать все вместе (по `AGENTS.md`):
1. `models/src/settings.rs` — дефолт + serde-compat тест.
2. `ui/src/app_state.rs` — `set_*` хелпер + зеркальный сигнал (если нужен).
3. Контрол в `layout/settings_modal.rs`.
4. Хелперы видимости/фильтрации в workspace.
5. Тулбар/переключатель-вход.
6. Любой флоу, который должен авто-открывать панель.
