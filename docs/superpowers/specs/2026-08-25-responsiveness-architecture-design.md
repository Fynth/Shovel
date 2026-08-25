# Shovel: Отзывчивость и архитектура — дизайн

Дата: 2026-08-25
Статус: утверждён (brainstorming)

## Проблема

Интерфейс Shovel ощущается вялым в целом. Корневые причины, найденные при
анализе кода:

1. **Монолитный `Signal<Vec<QueryTabState>>`.** `QueryTabState` смешивает
   состояние редактора, результат, batch-состояние и pending-изменения в одном
   struct. Любое изменение любого поля (ввод символа, смена статуса, обновление
   результата) пишет весь вектор и уведомляет всех подписчиков. Ввод каждого
   символа в редакторе перерисовывает всё поддерево workspace.
2. **Глубокая прокидывание пропсов** через `WorkspaceBody → WorkspaceDock →
   WorkspaceDockPanel → WorkspacePanelContent`. Каждый рендер корня
   перерисовывает всё дерево.
3. **JS-мост `document::eval`** на каждый ввод/выделение/ресайз — round-trip
   через WebView, медленный.
4. **Тяжёлые синхронные операции** (форматирование SQL, экспорт, построение
   ER-диаграммы, материализация строк) блокируют render-цикл Dioxus.
5. **`services`-фасад существует, но не используется как реальная граница** —
   `ui` местами тянет нижние крейты напрямую.

## Цели

- Максимальная отзывчивость и плавность интерфейса.
- Отзывчивость в приоритете над минимальным diff.
- Глубокая перестройка допустима.

## Подход

Выбран **Подход B**: разделение состояния вкладки на независимые сигналы +
переписывание JS-моста + мемоизационные границы + вынос тяжёлого в фоновые
потоки. Подход A (только мемо-границы) недостаточен — не решает корень.
Подход C (событийная шина) избыточен и ломает существующие паттерны.

## Секция 1: Разделение состояния вкладки на 4 сигнала

Вместо одного `Signal<Vec<QueryTabState>>` вводим
`Signal<HashMap<u64, TabState>>`, где `TabState` — struct с четырьмя
независимыми сигналами:

| Сигнал | Содержимое | Частота изменения | Подписчики |
|--------|-----------|-------------------|-----------|
| `TAB_META` | `id`, `session_id`, `title`, `tab_kind`, `pinned` | Редко (создание, переименование, закрытие) | `TabsManager` (таббар) |
| `TAB_EDITOR` | `sql`, `selection`, `draft` | Каждый ввод | `SqlEditor` |
| `TAB_RESULT` | `result`, `status`, `batch_results`, `batch_outputs`, `execution_plan`, `current_offset`, `page_size`, `filter`, `sort`, `last_duration_ms` | При выполнении запроса | `ResultTable`, `BatchResultsView`, `ExecutionPlanView` |
| `TAB_PENDING` | `pending_table_changes` | При редактировании ячейки/строки | `ResultTable` (только grid), `TableEditor` |

**Ключевой эффект:** ввод в редакторе пишет только `TAB_EDITOR` →
перерисовывается только `SqlEditor`. Выполнение запроса пишет только
`TAB_RESULT`. Редактирование ячейки пишет только `TAB_PENDING` →
перерисовывается только grid.

### Реализация

```rust
struct TabState {
    meta: Signal<TabMeta>,
    editor: Signal<TabEditorState>,
    result: Signal<TabResultState>,
    pending: Signal<TabPendingState>,
}
```

`use_query_tabs` возвращает `Signal<HashMap<u64, TabState>>`. Компоненты
подписываются на конкретный вложенный сигнал через `use_memo`/`use_reactive`,
а не на весь вектор.

### Затрагиваемые файлы

- `models::QueryTabState` — разбивается на `TabMeta`, `TabEditorState`,
  `TabResultState`, `TabPendingState` (или остаётся как агрегат для
  сериализации, но UI работает с разделёнными сигналами).
- `ui/src/screens/workspace/hooks/use_query_tabs.rs` — возвращает новую
  структуру.
- `ui/src/screens/workspace/actions.rs` — все функции переписываются на запись
  в конкретный вложенный сигнал.
- `ui/src/screens/workspace/components/tabs.rs`, `result_table.rs`,
  `sql_editor.rs`, `table_editor.rs` — подписываются на нужный сигнал.

## Секция 2: Переписывание JS-моста

**Принцип:** DOM (textarea) — единственный источник правды для текста и
выделения во время ввода. Rust не читает и не пишет DOM на каждый ввод.

1. **Ввод (hot path):** textarea обрабатывает ввод нативно. Rust получает
   `oninput` → пишет только в `TAB_EDITOR` (без `document::eval` для
   чтения/установки).
2. **Подсветка:** переносится в JS-слой, работает на `requestAnimationFrame`
   с дебаунсом (~90ms). Rust не участвует в каждом кадре.
3. **Автодополнение:** запрос к LLM остаётся в Rust, результат вставляется в
   DOM одним `document::eval` (только когда пришёл ответ).
4. **Ресайз панелей:** уже вынесен в JS-скрипты. JS сам обновляет
   CSS-переменные во время перетаскивания, Rust получает финальное значение
   только на `mouseup`.
5. **Синхронизация в Rust:** Rust синхронизирует `TAB_EDITOR` с DOM только при
   смене вкладки или внешнем изменении SQL (Format/Generate).

### Затрагиваемые файлы

- `ui/src/screens/workspace/components/sql_editor.rs` — убрать `document::eval`
  из hot-path ввода.
- `ui/src/screens/workspace/components/sql_editor/highlight.rs` — подсветка в
  JS-слой по `requestAnimationFrame`.
- `ui/src/screens/workspace/components/sql_editor/selection.rs` — синхронизация
  выделения только при смене вкладки.
- `ui/src/screens/workspace/helpers.rs` — ресайз-скрипты оставить, убрать
  лишние round-trip.

## Секция 3: Вынос тяжёлых операций в фоновые потоки

| Операция | Где сейчас | Куда |
|----------|-----------|------|
| `format_sql` | render-цикл | `spawn_blocking` |
| `export_query_page_*` | async-задача | `spawn_blocking` |
| `build_er_diagram` | render-цикл | `spawn_blocking` |
| `materialize_display_rows` | `use_memo` в render | `spawn_blocking` + кэш по `(tab_id, result_revision)` |
| `format_row_json` / `format_all_rows_*` | render | `spawn_blocking` + кэш |

`materialize_display_rows` — самая частая тяжёлая операция (вызывается на
каждый рендер таблицы). Выносим в `spawn_blocking` и кэшируем результат по
`(tab_id, result_revision)`, чтобы не пересчитывать при скролле/выделении.

### Затрагиваемые файлы

- `ui/src/screens/workspace/components/result_table.rs` — `materialize_display_rows`
  через `spawn_blocking` + кэш.
- `ui/src/screens/workspace/helpers.rs` — `build_er_diagram` через
  `spawn_blocking`.
- `ui/src/screens/workspace/components/tabs.rs` — экспорт через `spawn_blocking`.
- `ui/src/screens/workspace/components/sql_editor.rs` — `format_sql` через
  `spawn_blocking`.

## Секция 4: Мемоизационные границы

1. **`use_memo` на границах панелей** — `WorkspaceDockPanel`,
   `WorkspacePanelContent` оборачиваются в `use_memo`, чтобы переключение одной
   панели не перерисовывало остальные.
2. **`use_reactive` для пропсов** — компоненты подписываются через
   `use_reactive`, чтобы не перерисовываться на несвязанные изменения.
3. **`key` на панелях** — уже есть (`key: "{panel.label()}"`), оставляем.
4. **Изоляция `ResultTable`** — подписывается только на `TAB_RESULT` +
   `TAB_PENDING`, не на `TAB_EDITOR`/`TAB_META`.

### Затрагиваемые файлы

- `ui/src/screens/workspace/mod.rs` — `WorkspaceDockPanel`, `WorkspacePanelContent`.
- `ui/src/screens/workspace/components/result_table.rs` — изоляция подписок.

## Секция 5: Поток данных, обработка ошибок, тестирование

### Поток данных

```
Ввод в редакторе → oninput → TAB_EDITOR (только редактор перерисовывается)
Выполнение запроса → spawn → TAB_RESULT (только область результатов)
Редактирование ячейки → TAB_PENDING (только grid)
Смена вкладки → TAB_META (только таббар)
```

### Обработка ошибок

- `spawn_blocking` — ошибки возвращаются через `Result`, показываются
  тостом/статусом, не паникуют.
- JS-мост — если `document::eval` падает, деградируем к сигналу (fallback), не
  роняем приложение.

### Тестирование

- Юнит-тесты для разделения сигналов (запись в один не уведомляет подписчиков
  другого).
- Юнит-тесты для кэша `materialize_display_rows`.
- Юнит-тесты для `spawn_blocking`-обёрток.
- CI: `cargo fmt`, `cargo clippy -D warnings`, `cargo test`.

## Вне области

- Подход C (событийная шина) — не реализуем.
- `services`-фасад как реальная граница — отдельная задача, не входит в этот
  дизайн.
- ClickHouse row-editing — не входит (уже не поддерживается).
