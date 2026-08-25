use models::SavedQuery;

use crate::fs_store::{read_json_file, saved_queries_path, write_json_file};

/// Load all saved queries from `saved_queries.json`.
///
/// Results are sorted by folder name, then title, then ID.
///
/// # Errors
///
/// Returns an error string if the file cannot be read or parsed.
pub async fn load_saved_queries() -> Result<Vec<SavedQuery>, String> {
    let mut items: Vec<SavedQuery> = read_json_file(saved_queries_path()).await?;
    items.sort_by(|left, right| {
        left.folder_name()
            .cmp(right.folder_name())
            .then_with(|| left.title.cmp(&right.title))
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(items)
}

/// Save (upsert) a single saved query to `saved_queries.json`.
///
/// If a query with the same ID already exists, it is replaced.
///
/// # Arguments
///
/// * `item` - The [`SavedQuery`] to persist.
///
/// # Errors
///
/// Returns an error string if the file cannot be written.
pub async fn save_saved_query(item: SavedQuery) -> Result<(), String> {
    // Propagate read/parse errors so a corrupted or unreadable saved_queries.json
    // is not silently overwritten with only the new query.
    let mut items = load_saved_queries().await?;
    items.retain(|existing| existing.id != item.id);
    items.push(item);
    write_json_file(saved_queries_path(), &items).await
}

/// Delete a saved query by its ID.
///
/// If no query with the given ID exists, this is a no-op.
///
/// # Arguments
///
/// * `id` - The unique identifier of the query to delete.
///
/// # Errors
///
/// Returns an error string if the file cannot be written.
pub async fn delete_saved_query(id: u64) -> Result<(), String> {
    let mut items = load_saved_queries().await?;
    items.retain(|existing| existing.id != id);
    write_json_file(saved_queries_path(), &items).await
}
