use dioxus::prelude::*;
use std::collections::HashMap;

#[derive(Clone, PartialEq)]
pub struct ErTable {
    pub schema: String,
    pub name: String,
    pub columns: Vec<ErColumn>,
    pub primary_key: Vec<String>,
    pub foreign_keys: Vec<ErForeignKey>,
}

#[derive(Clone, PartialEq)]
pub struct ErColumn {
    pub name: String,
    pub data_type: String,
    pub is_nullable: bool,
    pub is_primary_key: bool,
    pub is_foreign_key: bool,
}

#[derive(Clone, PartialEq)]
pub struct ErForeignKey {
    pub name: String,
    pub from_table: String,
    pub from_column: String,
    pub to_table: String,
    pub to_column: String,
}

#[derive(Clone, PartialEq)]
pub struct ErDiagramState {
    pub tables: Vec<ErTable>,
    pub relationships: Vec<ErRelationship>,
}

#[derive(Clone, PartialEq)]
pub struct ErRelationship {
    pub from_table: String,
    pub from_column: String,
    pub to_table: String,
    pub to_column: String,
}

#[derive(Clone, PartialEq)]
struct ErCardPos {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

#[derive(Clone)]
struct ErLine {
    d: String,
}

/// Prop-driven ER diagram viewer.
///
/// Lives in its own native OS window; the caller passes the already-resolved
/// diagram as a plain value, so this component needs no global state, signals,
/// or overlay host.
#[component]
pub fn ErDiagramViewer(
    diagram: ErDiagramState,
    on_close: Callback<()>,
    on_table_click: Callback<String>,
) -> Element {
    let mut view_offset = use_signal(|| (0.0f64, 0.0f64));
    let mut zoom = use_signal(|| 1.0f64);
    let mut is_dragging = use_signal(|| false);
    let mut drag_start = use_signal(|| (0.0f64, 0.0f64));

    let table_positions = calculate_table_positions(&diagram.tables, &diagram.relationships);
    let relationship_lines: Vec<ErLine> = diagram
        .relationships
        .iter()
        .filter_map(|rel| {
            let from_table = diagram
                .tables
                .iter()
                .find(|table| table.name == rel.from_table)?;
            let to_table = diagram
                .tables
                .iter()
                .find(|table| table.name == rel.to_table)?;
            let from = table_positions.get(&rel.from_table)?;
            let to = table_positions.get(&rel.to_table)?;
            Some(ErLine {
                d: relationship_path(
                    from,
                    to,
                    column_anchor_y(from_table, from, &rel.from_column),
                    column_anchor_y(to_table, to, &rel.to_column),
                ),
            })
        })
        .collect();
    let (world_w, world_h) = world_bounds(&table_positions);

    rsx! {
        div {
            class: "er-diagram",
            div {
                class: "er-diagram__header",
                span {
                    class: "er-diagram__title",
                    "ER Diagram — {diagram.tables.len()} tables, {diagram.relationships.len()} relationships"
                }
                div {
                    class: "er-diagram__controls",
                    button {
                        class: "er-diagram__zoom-btn",
                        onclick: move |_| zoom.set((zoom() * 1.2).min(3.0)),
                        "+"
                    }
                    button {
                        class: "er-diagram__zoom-btn",
                        onclick: move |_| zoom.set((zoom() / 1.2).max(0.3)),
                        "−"
                    }
                    button {
                        class: "er-diagram__zoom-btn",
                        onclick: move |_| {
                            zoom.set(1.0);
                            view_offset.set((0.0, 0.0));
                        },
                        "Reset"
                    }
                    button {
                        class: "er-diagram__zoom-btn",
                        onclick: move |_| on_close.call(()),
                        "Close"
                    }
                }
            }
            div {
                class: "er-diagram__canvas",
                onmousedown: move |event| {
                    is_dragging.set(true);
                    drag_start.set((event.client_coordinates().x, event.client_coordinates().y));
                },
                onmousemove: move |event| {
                    if is_dragging() {
                        let (start_x, start_y) = drag_start();
                        let delta_x = event.client_coordinates().x - start_x;
                        let delta_y = event.client_coordinates().y - start_y;
                        let (current_x, current_y) = view_offset();
                        view_offset.set((current_x + delta_x, current_y + delta_y));
                        drag_start.set((event.client_coordinates().x, event.client_coordinates().y));
                    }
                },
                onmouseup: move |_| is_dragging.set(false),
                onmouseleave: move |_| is_dragging.set(false),
                onwheel: move |event| {
                    let dy = event.data().delta().strip_units().y;
                    if dy < 0.0 {
                        zoom.set((zoom() * 1.08).min(3.0));
                    } else if dy > 0.0 {
                        zoom.set((zoom() / 1.08).max(0.3));
                    }
                },
                div {
                    class: "er-diagram__world",
                    style: format!(
                        "width: {world_w}px; height: {world_h}px; transform: translate({}px, {}px) scale({});",
                        view_offset().0,
                        view_offset().1,
                        zoom()
                    ),
                    div {
                        class: "er-diagram__tables",
                        for table in diagram.tables.iter() {
                            ErTableCard {
                                table: table.clone(),
                                position: table_positions.get(&table.name).cloned(),
                                on_click: on_table_click,
                            }
                        }
                    }
                    svg {
                        class: "er-diagram__svg",
                        width: "{world_w}",
                        height: "{world_h}",
                        defs {
                            marker {
                                id: "arrowhead",
                                marker_width: "8",
                                marker_height: "8",
                                ref_x: "7",
                                ref_y: "4",
                                orient: "auto",
                                polygon {
                                    points: "0 0, 8 4, 0 8",
                                    fill: "var(--color-primary)",
                                }
                            }
                        }
                        for line in relationship_lines.iter() {
                            path {
                                d: "{line.d}",
                                fill: "none",
                                stroke: "var(--color-primary)",
                                stroke_width: "1.6",
                                marker_end: "url(#arrowhead)",
                            }
                        }
                    }
                }
            }
            div {
                class: "er-diagram__legend",
                div {
                    class: "er-diagram__legend-item",
                    span { class: "er-diagram__legend-line" }
                    "Foreign key"
                }
                div {
                    class: "er-diagram__legend-item",
                    span { class: "er-diagram__legend-pk", "PK" }
                    "Primary key"
                }
                div {
                    class: "er-diagram__legend-item",
                    span { class: "er-diagram__legend-fk", "FK" }
                    "Foreign key column"
                }
            }
        }
    }
}

#[component]
fn ErTableCard(table: ErTable, position: Option<ErCardPos>, on_click: Callback<String>) -> Element {
    let pos = position.unwrap_or(ErCardPos {
        x: 100.0,
        y: 100.0,
        w: CARD_WIDTH,
        h: 80.0,
    });

    rsx! {
        div {
            class: "er-table-card",
            style: format!("left: {}px; top: {}px; width: {}px;", pos.x, pos.y, pos.w),
            onmousedown: move |event| event.stop_propagation(),
            onclick: move |_| on_click.call(table.name.clone()),
            div {
                class: "er-table-card__header",
                span {
                    class: "er-table-card__name",
                    "{table.name}"
                }
                span {
                    class: "er-table-card__schema",
                    "{table.schema}"
                }
            }
            div {
                class: "er-table-card__columns",
                if table.columns.is_empty() {
                    div {
                        class: "er-table-card__column er-table-card__column--empty",
                        "No columns loaded"
                    }
                }
                for column in table.columns.iter() {
                    div {
                        class: if column.is_foreign_key {
                            "er-table-card__column er-table-card__column--fk"
                        } else if column.is_primary_key {
                            "er-table-card__column er-table-card__column--pk"
                        } else {
                            "er-table-card__column"
                        },
                        span {
                            class: if column.is_primary_key {
                                "er-table-card__pk-badge"
                            } else if column.is_foreign_key {
                                "er-table-card__fk-badge"
                            } else {
                                "er-table-card__badge-spacer"
                            },
                            if column.is_primary_key {
                                "PK"
                            } else if column.is_foreign_key {
                                "FK"
                            } else {
                                ""
                            }
                        }
                        span {
                            class: "er-table-card__column-name",
                            "{column.name}"
                            if !column.is_nullable && !column.is_primary_key {
                                span { class: "er-table-card__null-mark", "*" }
                            }
                        }
                        span {
                            class: "er-table-card__column-type",
                            if let Some(fk) = table.foreign_keys.iter().find(|fk| fk.from_column == column.name) {
                                "→ {fk.to_table}.{fk.to_column}"
                            } else {
                                "{column.data_type}"
                            }
                        }
                    }
                }
            }
        }
    }
}

const CARD_WIDTH: f64 = 240.0;
const CARD_HEADER: f64 = 32.0;
const CARD_ROW: f64 = 22.0;
const CARD_PAD: f64 = 4.0;
const H_GAP: f64 = 168.0;
const V_GAP: f64 = 56.0;
const ORIGIN: f64 = 40.0;
const EDGE_STUB: f64 = 20.0;

fn card_height(table: &ErTable) -> f64 {
    let rows = table.columns.len().max(1) as f64;
    CARD_HEADER + CARD_PAD + rows * CARD_ROW
}

fn column_anchor_y(table: &ErTable, pos: &ErCardPos, column: &str) -> f64 {
    let idx = table
        .columns
        .iter()
        .position(|col| col.name.eq_ignore_ascii_case(column))
        .unwrap_or(0);
    pos.y + CARD_HEADER + CARD_PAD / 2.0 + (idx as f64 + 0.5) * CARD_ROW
}

fn calculate_table_positions(
    tables: &[ErTable],
    relationships: &[ErRelationship],
) -> HashMap<String, ErCardPos> {
    let mut positions = HashMap::new();
    if tables.is_empty() {
        return positions;
    }

    let mut rank: HashMap<&str, usize> = tables
        .iter()
        .map(|table| (table.name.as_str(), 0))
        .collect();
    for _ in 0..tables.len() {
        let mut changed = false;
        for rel in relationships {
            let parent = rank.get(rel.to_table.as_str()).copied().unwrap_or(0);
            let child = rank.get(rel.from_table.as_str()).copied().unwrap_or(0);
            if child < parent + 1 {
                rank.insert(rel.from_table.as_str(), parent + 1);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let mut buckets: HashMap<usize, Vec<&ErTable>> = HashMap::new();
    for table in tables {
        buckets
            .entry(*rank.get(table.name.as_str()).unwrap_or(&0))
            .or_default()
            .push(table);
    }

    let mut max_rank = 0;
    for table in tables {
        max_rank = max_rank.max(*rank.get(table.name.as_str()).unwrap_or(&0));
    }

    for column in 0..=max_rank {
        let Some(column_tables) = buckets.get(&column) else {
            continue;
        };
        let mut y = ORIGIN;
        for table in column_tables {
            let h = card_height(table);
            positions.insert(
                table.name.clone(),
                ErCardPos {
                    x: ORIGIN + column as f64 * (CARD_WIDTH + H_GAP),
                    y,
                    w: CARD_WIDTH,
                    h,
                },
            );
            y += h + V_GAP;
        }
    }

    positions
}

/// Orthogonal connector from a source column row to a target column row.
///
/// Leaves the FK row horizontally, turns in the gap, then enters the PK row
/// so `orders.user_id` clearly points at `users.id` instead of the card body.
fn relationship_path(from: &ErCardPos, to: &ErCardPos, y1: f64, y2: f64) -> String {
    let from_left = from.x;
    let from_right = from.x + from.w;
    let to_left = to.x;
    let to_right = to.x + to.w;
    let gap = 8.0;

    if from_right + gap <= to_left {
        manhattan(from_right, y1, to_left, y2)
    } else if to_right + gap <= from_left {
        manhattan(from_left, y1, to_right, y2)
    } else {
        let outer = from_right.max(to_right) + EDGE_STUB;
        format!("M {from_right:.1} {y1:.1} H {outer:.1} V {y2:.1} H {to_right:.1}")
    }
}

fn manhattan(x1: f64, y1: f64, x2: f64, y2: f64) -> String {
    if (y1 - y2).abs() < 0.5 {
        format!("M {x1:.1} {y1:.1} H {x2:.1}")
    } else {
        let mid = (x1 + x2) / 2.0;
        format!("M {x1:.1} {y1:.1} H {mid:.1} V {y2:.1} H {x2:.1}")
    }
}

fn world_bounds(positions: &HashMap<String, ErCardPos>) -> (f64, f64) {
    let mut max_x: f64 = 400.0;
    let mut max_y: f64 = 300.0;
    for pos in positions.values() {
        max_x = max_x.max(pos.x + pos.w + ORIGIN);
        max_y = max_y.max(pos.y + pos.h + ORIGIN);
    }
    (max_x, max_y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_table(name: &str) -> ErTable {
        ErTable {
            schema: "public".to_string(),
            name: name.to_string(),
            columns: vec![ErColumn {
                name: "id".to_string(),
                data_type: "INTEGER".to_string(),
                is_nullable: false,
                is_primary_key: true,
                is_foreign_key: false,
            }],
            primary_key: vec!["id".to_string()],
            foreign_keys: vec![],
        }
    }

    fn make_relationship(from: &str, from_col: &str, to: &str, to_col: &str) -> ErRelationship {
        ErRelationship {
            from_table: from.to_string(),
            from_column: from_col.to_string(),
            to_table: to.to_string(),
            to_column: to_col.to_string(),
        }
    }

    #[test]
    fn column_anchor_y_uses_column_index() {
        let mut table = make_table("orders");
        table.columns.push(ErColumn {
            name: "user_id".to_string(),
            data_type: "INTEGER".to_string(),
            is_nullable: false,
            is_primary_key: false,
            is_foreign_key: true,
        });
        let pos = ErCardPos {
            x: 40.0,
            y: 40.0,
            w: CARD_WIDTH,
            h: card_height(&table),
        };
        let y_id = column_anchor_y(&table, &pos, "id");
        let y_fk = column_anchor_y(&table, &pos, "user_id");
        assert!(y_fk > y_id);
    }

    #[test]
    fn calculate_positions_empty_tables() {
        let positions = calculate_table_positions(&[], &[]);
        assert!(positions.is_empty());
    }

    #[test]
    fn calculate_positions_single_table() {
        let tables = vec![make_table("users")];
        let positions = calculate_table_positions(&tables, &[]);
        assert_eq!(positions.len(), 1);
        let pos = positions.get("users").unwrap();
        assert_eq!(pos.x, 40.0);
        assert_eq!(pos.y, 40.0);
    }

    #[test]
    fn calculate_positions_multiple_tables_no_relationships() {
        let tables = vec![
            make_table("users"),
            make_table("orders"),
            make_table("products"),
        ];
        let positions = calculate_table_positions(&tables, &[]);
        assert_eq!(positions.len(), 3);
        assert!(positions.contains_key("users"));
        assert!(positions.contains_key("orders"));
        assert!(positions.contains_key("products"));
        let users = &positions["users"];
        assert_eq!(users.x, 40.0);
        assert_eq!(users.y, 40.0);
        let orders = &positions["orders"];
        assert_eq!(orders.x, 40.0);
        assert!(orders.y > users.y);
    }

    #[test]
    fn calculate_positions_with_relationships_ranks_children_to_the_right() {
        let tables = vec![
            make_table("users"),
            make_table("orders"),
            make_table("items"),
        ];
        let relationships = vec![
            make_relationship("orders", "user_id", "users", "id"),
            make_relationship("items", "order_id", "orders", "id"),
        ];
        let positions = calculate_table_positions(&tables, &relationships);
        assert_eq!(positions.len(), 3);
        let users = &positions["users"];
        let orders = &positions["orders"];
        let items = &positions["items"];
        assert_eq!(users.x, 40.0);
        assert!(orders.x > users.x);
        assert!(items.x > orders.x);
    }

    #[test]
    fn calculate_positions_disconnected_tables_still_get_positions() {
        let tables = vec![make_table("users"), make_table("orphan_table")];
        // orphan_table has no relationships, only references users
        let relationships = vec![make_relationship("users", "id", "users", "id")];
        let positions = calculate_table_positions(&tables, &relationships);
        assert_eq!(positions.len(), 2);
        assert!(positions.contains_key("users"));
        assert!(positions.contains_key("orphan_table"));
    }

    #[test]
    fn relationship_path_is_orthogonal_from_fk_row_to_pk_row() {
        let users = make_table("users");
        let mut orders = make_table("orders");
        orders.columns.push(ErColumn {
            name: "user_id".to_string(),
            data_type: "INTEGER".to_string(),
            is_nullable: false,
            is_primary_key: false,
            is_foreign_key: true,
        });
        let users_pos = ErCardPos {
            x: 40.0,
            y: 40.0,
            w: CARD_WIDTH,
            h: card_height(&users),
        };
        let orders_pos = ErCardPos {
            x: 40.0 + CARD_WIDTH + H_GAP,
            y: 40.0,
            w: CARD_WIDTH,
            h: card_height(&orders),
        };
        let y_fk = column_anchor_y(&orders, &orders_pos, "user_id");
        let y_pk = column_anchor_y(&users, &users_pos, "id");
        let path = relationship_path(&orders_pos, &users_pos, y_fk, y_pk);
        assert!(
            path.starts_with(&format!("M {:.1} {:.1}", orders_pos.x, y_fk)),
            "arrow should leave the left edge of the FK row: {path}"
        );
        assert!(
            path.contains(&format!("H {:.1}", users_pos.x + users_pos.w)),
            "arrow should enter the right edge of users: {path}"
        );
        assert!(
            path.contains(&format!("V {:.1}", y_pk)),
            "vertical segment should align with users.id: {path}"
        );
        assert!(
            !path.contains('C'),
            "should be orthogonal, not a bezier: {path}"
        );
    }

    #[test]
    fn calculate_positions_grid_layout_many_tables() {
        let tables: Vec<ErTable> = (0..9).map(|i| make_table(&format!("t{i}"))).collect();
        let positions = calculate_table_positions(&tables, &[]);
        assert_eq!(positions.len(), 9);
        let unique_positions: std::collections::HashSet<(u64, u64)> = positions
            .values()
            .map(|pos| (pos.x.to_bits(), pos.y.to_bits()))
            .collect();
        assert_eq!(unique_positions.len(), 9);
    }
}
