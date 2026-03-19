use crate::error::VibecheckError;
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::Path;

/// Current cache schema version. Increment when adding new migrations.
const CACHE_SCHEMA_VERSION: i32 = 1;

/// Cache statistics.
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub entry_count: usize,
    pub size_bytes: u64,
}

/// Result of a cache prune operation.
#[derive(Debug, Clone)]
pub struct PruneResult {
    pub entries_removed: usize,
    pub bytes_freed: u64,
}

/// Configure a cache connection with WAL mode, busy timeout, synchronous=NORMAL.
/// Same pattern as `db::configure_connection` but without foreign keys or main-DB migrations.
fn configure_cache_connection(conn: &Connection) -> Result<(), VibecheckError> {
    conn.busy_timeout(std::time::Duration::from_secs(30))?;
    conn.execute_batch("PRAGMA journal_mode = WAL;")?;
    conn.execute_batch("PRAGMA synchronous = NORMAL;")?;
    run_cache_migrations(conn)?;
    Ok(())
}

/// Run cache-specific migrations using user_version for schema versioning.
fn run_cache_migrations(conn: &Connection) -> Result<(), VibecheckError> {
    let version: i32 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap_or(0);

    if version < 1 {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS cache_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS embeddings (
                content_hash TEXT PRIMARY KEY,
                embedding BLOB NOT NULL
            );
            ",
        )?;
        conn.pragma_update(None, "user_version", 1)?;
    }

    debug_assert_eq!(
        conn.pragma_query_value(None, "user_version", |row| row.get::<_, i32>(0))
            .unwrap_or(0),
        CACHE_SCHEMA_VERSION
    );

    Ok(())
}

/// Get a value from cache_meta by key.
fn get_cache_meta(conn: &Connection, key: &str) -> Result<Option<String>, VibecheckError> {
    match conn
        .prepare_cached("SELECT value FROM cache_meta WHERE key = ?")?
        .query_row([key], |row| row.get(0))
    {
        Ok(val) => Ok(Some(val)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Set a value in cache_meta (insert or replace).
fn set_cache_meta(conn: &Connection, key: &str, value: &str) -> Result<(), VibecheckError> {
    conn.prepare_cached("INSERT OR REPLACE INTO cache_meta (key, value) VALUES (?, ?)")?
        .execute([key, value])?;
    Ok(())
}

/// Open the shared embedding cache. Creates it if it doesn't exist.
/// Validates settings match (model, dimensions, max_input_bytes) or errors.
/// If `force` is true and settings mismatch, drops and recreates.
pub fn open_cache(
    cache_path: &Path,
    model: &str,
    dimensions: usize,
    max_input_bytes: usize,
    force: bool,
) -> Result<Connection, VibecheckError> {
    let conn = Connection::open(cache_path)?;
    configure_cache_connection(&conn)?;

    // Check if cache already has settings stored
    let stored_model = get_cache_meta(&conn, "model_name")?;
    let stored_dims = get_cache_meta(&conn, "dimensions")?;
    let stored_max_bytes = get_cache_meta(&conn, "max_input_bytes")?;

    match stored_model {
        Some(ref sm) => {
            // Cache exists with settings -- check for mismatch
            let dims_str = dimensions.to_string();
            let max_bytes_str = max_input_bytes.to_string();

            let model_mismatch = sm != model;
            let dims_mismatch = stored_dims.as_deref() != Some(dims_str.as_str());
            let max_bytes_mismatch = stored_max_bytes.as_deref() != Some(max_bytes_str.as_str());

            if model_mismatch || dims_mismatch || max_bytes_mismatch {
                if !force {
                    return Err(VibecheckError::Index(format!(
                        "Cache was created with model '{}', dimensions {}. \
                         Run with `--force` to rebuild.",
                        sm,
                        stored_dims.as_deref().unwrap_or("?"),
                    )));
                }

                // Force rebuild: drop embeddings, recreate, update meta
                conn.execute_batch("DELETE FROM embeddings;")?;
                set_cache_meta(&conn, "model_name", model)?;
                set_cache_meta(&conn, "dimensions", &dims_str)?;
                set_cache_meta(&conn, "max_input_bytes", &max_bytes_str)?;
            }
        }
        None => {
            // New cache -- store settings
            set_cache_meta(&conn, "model_name", model)?;
            set_cache_meta(&conn, "dimensions", &dimensions.to_string())?;
            set_cache_meta(&conn, "max_input_bytes", &max_input_bytes.to_string())?;
        }
    }

    Ok(conn)
}

/// Close the cache connection with WAL checkpoint.
pub fn close_cache(conn: Connection) -> Result<(), VibecheckError> {
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    // `conn` is dropped here, calling sqlite3_close()
    Ok(())
}

/// Batch lookup: given content hashes, return those with cached embeddings.
/// Returns HashMap<content_hash, embedding_bytes>.
pub fn lookup_embeddings(
    conn: &Connection,
    content_hashes: &[&str],
) -> Result<HashMap<String, Vec<u8>>, VibecheckError> {
    let mut results = HashMap::new();

    if content_hashes.is_empty() {
        return Ok(results);
    }

    // Use batched IN queries for efficiency. SQLite has a limit on the number of
    // variables in a single statement (default 999), so we chunk if needed.
    for chunk in content_hashes.chunks(500) {
        let placeholders: Vec<&str> = chunk.iter().map(|_| "?").collect();
        let sql = format!(
            "SELECT content_hash, embedding FROM embeddings WHERE content_hash IN ({})",
            placeholders.join(", ")
        );

        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::types::ToSql> = chunk
            .iter()
            .map(|h| h as &dyn rusqlite::types::ToSql)
            .collect();

        let rows = stmt.query_map(params.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?;

        for row in rows {
            let (hash, embedding) = row?;
            results.insert(hash, embedding);
        }
    }

    Ok(results)
}

/// Insert an embedding into the cache.
pub fn insert_embedding(
    conn: &Connection,
    content_hash: &str,
    embedding: &[u8],
) -> Result<(), VibecheckError> {
    conn.prepare_cached(
        "INSERT OR REPLACE INTO embeddings (content_hash, embedding) VALUES (?, ?)",
    )?
    .execute(rusqlite::params![content_hash, embedding])?;
    Ok(())
}

/// Get cache statistics. Returns None if cache doesn't exist.
pub fn cache_stats(cache_path: &Path) -> Option<CacheStats> {
    if !cache_path.exists() {
        return None;
    }

    let size_bytes = std::fs::metadata(cache_path).ok()?.len();

    // Open the database read-only to count entries
    let conn = Connection::open_with_flags(
        cache_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;

    let entry_count: usize = conn
        .query_row("SELECT COUNT(*) FROM embeddings", [], |row| row.get(0))
        .ok()?;

    Some(CacheStats {
        entry_count,
        size_bytes,
    })
}

/// Prune entries not referenced by any worktree's function DB.
/// Discovers worktrees via `git worktree list`, opens each .vibecheck.db,
/// collects all content_hashes in use, deletes unreferenced cache entries.
pub fn prune_cache(
    _cache_path: &Path,
    _project_root: &Path,
) -> Result<PruneResult, VibecheckError> {
    todo!("git-integration: not yet implemented")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_cache_creates_new_db_with_schema() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache_path = tmp.path().join("test-cache.db");

        let conn = open_cache(&cache_path, "nomic-embed-code", 768, 16000, false).unwrap();

        // Verify the cache_meta table exists and has the expected keys
        let model: String = conn
            .query_row(
                "SELECT value FROM cache_meta WHERE key = 'model_name'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(model, "nomic-embed-code");

        let dims: String = conn
            .query_row(
                "SELECT value FROM cache_meta WHERE key = 'dimensions'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(dims, "768");

        let max_bytes: String = conn
            .query_row(
                "SELECT value FROM cache_meta WHERE key = 'max_input_bytes'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(max_bytes, "16000");

        // Verify embeddings table exists
        let table_exists: bool = conn
            .prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name='embeddings'")
            .unwrap()
            .exists([])
            .unwrap();
        assert!(table_exists);
    }

    #[test]
    fn insert_and_lookup_round_trip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache_path = tmp.path().join("test-cache.db");

        let conn = open_cache(&cache_path, "nomic-embed-code", 768, 16000, false).unwrap();

        // Insert an embedding
        let hash = "abc123def456";
        let embedding_bytes: Vec<u8> = vec![0, 1, 2, 3, 4, 5, 6, 7];
        insert_embedding(&conn, hash, &embedding_bytes).unwrap();

        // Look it up
        let results = lookup_embeddings(&conn, &[hash]).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results.get(hash).unwrap(), &embedding_bytes);
    }

    #[test]
    fn lookup_returns_empty_for_unknown_hashes() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache_path = tmp.path().join("test-cache.db");

        let conn = open_cache(&cache_path, "nomic-embed-code", 768, 16000, false).unwrap();

        let results = lookup_embeddings(&conn, &["nonexistent_hash"]).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn open_cache_with_mismatched_model_errors() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache_path = tmp.path().join("test-cache.db");

        // First open creates the cache with model A
        let conn = open_cache(&cache_path, "model-a", 768, 16000, false).unwrap();
        drop(conn);

        // Second open with model B should error
        let result = open_cache(&cache_path, "model-b", 768, 16000, false);
        assert!(result.is_err(), "Expected error on model mismatch");
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("Cache was created with model"),
            "Error message should mention model mismatch, got: {msg}"
        );
    }

    #[test]
    fn open_cache_with_mismatched_model_force_succeeds() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache_path = tmp.path().join("test-cache.db");

        // First open creates the cache with model A
        let conn = open_cache(&cache_path, "model-a", 768, 16000, false).unwrap();
        insert_embedding(&conn, "hash1", &[1, 2, 3, 4]).unwrap();
        drop(conn);

        // Second open with model B + force=true should succeed and rebuild
        let conn = open_cache(&cache_path, "model-b", 768, 16000, true).unwrap();

        // Verify the model was updated
        let model: String = conn
            .query_row(
                "SELECT value FROM cache_meta WHERE key = 'model_name'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(model, "model-b");

        // Old entries should be gone after rebuild
        let results = lookup_embeddings(&conn, &["hash1"]).unwrap();
        assert!(results.is_empty(), "Old entries should be cleared on force rebuild");
    }

    #[test]
    fn cache_stats_returns_entry_count_and_size() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache_path = tmp.path().join("test-cache.db");

        // Create cache and insert some entries
        let conn = open_cache(&cache_path, "nomic-embed-code", 768, 16000, false).unwrap();
        insert_embedding(&conn, "hash_a", &[1, 2, 3, 4]).unwrap();
        insert_embedding(&conn, "hash_b", &[5, 6, 7, 8]).unwrap();
        drop(conn);

        let stats = cache_stats(&cache_path);
        assert!(stats.is_some(), "cache_stats should return Some for existing cache");
        let stats = stats.unwrap();
        assert_eq!(stats.entry_count, 2);
        assert!(stats.size_bytes > 0, "Cache size should be > 0");
    }

    #[test]
    fn cache_stats_returns_none_for_nonexistent_cache() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache_path = tmp.path().join("nonexistent-cache.db");

        let stats = cache_stats(&cache_path);
        assert!(stats.is_none(), "cache_stats should return None for nonexistent cache");
    }
}
