use crate::error::VibecheckError;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
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
    cache_path: &Path,
    project_root: &Path,
) -> Result<PruneResult, VibecheckError> {
    use crate::util::config::DB_FILENAME;
    use crate::util::git::{is_git_repo, list_worktree_paths};

    // Must be a git repo
    if !is_git_repo(project_root) {
        return Err(VibecheckError::Git(
            "Not a git repository. Cache prune requires git.".into(),
        ));
    }

    // Cache must exist
    if !cache_path.exists() {
        return Err(VibecheckError::Index("No cache found".into()));
    }

    // Step 1: Discover worktrees
    let worktree_paths = list_worktree_paths(project_root)?;

    // Step 2: Collect referenced content hashes from all worktree DBs
    let mut referenced_hashes: HashSet<String> = HashSet::new();

    for wt_path in &worktree_paths {
        let db_path = wt_path.join(DB_FILENAME);
        if !db_path.exists() {
            // Not all worktrees may be indexed; skip
            continue;
        }

        let db_path_str = db_path.to_string_lossy();
        // Use open_database_no_vec since we only need to read content_hash values
        let conn = crate::store::db::open_database_no_vec(&db_path_str)?;

        {
            let mut stmt = conn.prepare("SELECT DISTINCT content_hash FROM functions")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;

            for row in rows {
                referenced_hashes.insert(row?);
            }
        }

        crate::store::db::close_database(conn)?;
    }

    // Step 3: Open cache DB, get stats before pruning
    let cache_size_before = std::fs::metadata(cache_path)
        .map(|m| m.len())
        .unwrap_or(0);

    let cache_conn = Connection::open(cache_path)?;
    configure_cache_connection(&cache_conn)?;

    let total_entries_before: usize = cache_conn
        .query_row("SELECT COUNT(*) FROM embeddings", [], |row| row.get(0))?;

    // Step 4: Delete unreferenced entries
    if referenced_hashes.is_empty() {
        // No worktree DBs found or none have functions -- delete everything
        cache_conn.execute("DELETE FROM embeddings", [])?;
    } else {
        // Build a temp table with referenced hashes for efficient deletion.
        // This avoids constructing huge NOT IN (...) clauses.
        cache_conn.execute_batch(
            "CREATE TEMP TABLE _referenced_hashes (content_hash TEXT PRIMARY KEY);",
        )?;

        {
            let tx = cache_conn.unchecked_transaction()?;
            {
                let mut insert_stmt = tx.prepare(
                    "INSERT OR IGNORE INTO _referenced_hashes (content_hash) VALUES (?)",
                )?;
                for hash in &referenced_hashes {
                    insert_stmt.execute([hash])?;
                }
            }
            tx.commit()?;
        }

        cache_conn.execute(
            "DELETE FROM embeddings WHERE content_hash NOT IN (SELECT content_hash FROM _referenced_hashes)",
            [],
        )?;

        cache_conn.execute_batch("DROP TABLE IF EXISTS _referenced_hashes;")?;
    }

    let total_entries_after: usize = cache_conn
        .query_row("SELECT COUNT(*) FROM embeddings", [], |row| row.get(0))?;

    // Checkpoint WAL to reclaim space and get accurate file size
    cache_conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;

    // VACUUM to actually free disk space from deleted rows
    if total_entries_before > total_entries_after {
        cache_conn.execute_batch("VACUUM;")?;
    }

    drop(cache_conn);

    let cache_size_after = std::fs::metadata(cache_path)
        .map(|m| m.len())
        .unwrap_or(0);

    let entries_removed = total_entries_before.saturating_sub(total_entries_after);
    let bytes_freed = cache_size_before.saturating_sub(cache_size_after);

    Ok(PruneResult {
        entries_removed,
        bytes_freed,
    })
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

    // --- prune_cache tests ---

    /// Helper to create a git repo in a temp directory.
    fn create_git_repo(dir: &std::path::Path) {
        std::process::Command::new("git")
            .args(["init", &dir.to_string_lossy()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("failed to run git init");

        // Configure git user for commits (needed for some git operations)
        std::process::Command::new("git")
            .args([
                "-C",
                &dir.to_string_lossy(),
                "config",
                "user.email",
                "test@test.com",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok();
        std::process::Command::new("git")
            .args([
                "-C",
                &dir.to_string_lossy(),
                "config",
                "user.name",
                "Test",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok();
    }

    /// Helper to create a .vibecheck.db with given content hashes in a directory.
    fn create_worktree_db(dir: &std::path::Path, content_hashes: &[&str]) {
        let db_path = dir.join(".vibecheck.db");
        let db_path_str = db_path.to_string_lossy().to_string();
        let conn = crate::store::db::open_database_no_vec(&db_path_str).unwrap();

        // We need a tracked_file to insert functions (FK constraint)
        conn.execute(
            "INSERT OR IGNORE INTO tracked_files (file_path, content_hash, mtime_ms, indexed_at) VALUES (?, ?, ?, ?)",
            rusqlite::params!["test.ts", "file_hash", 0, "2024-01-01T00:00:00Z"],
        ).unwrap();

        for (i, hash) in content_hashes.iter().enumerate() {
            let id = format!("test.ts:func{}:{}", i, i + 1);
            conn.execute(
                "INSERT OR IGNORE INTO functions (id, file_path, function_name, source_text, start_line, end_line, params_json, return_type, is_exported, signature_hash, content_hash, chunk_type, signature, tokens_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    id,
                    "test.ts",
                    format!("func{}", i),
                    format!("function func{}() {{}}", i),
                    i + 1,
                    i + 2,
                    "[]",
                    rusqlite::types::Null,
                    0,
                    "sig_hash",
                    hash,
                    "function",
                    "",
                    "[]",
                ],
            ).unwrap();
        }

        crate::store::db::close_database(conn).unwrap();
    }

    #[test]
    fn prune_cache_removes_unreferenced_entries() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project_dir = tmp.path().join("project");
        std::fs::create_dir_all(&project_dir).unwrap();
        create_git_repo(&project_dir);

        // Create a worktree DB that references hashes "hash_a" and "hash_b"
        create_worktree_db(&project_dir, &["hash_a", "hash_b"]);

        // Create a cache with "hash_a", "hash_b", "hash_c", "hash_d"
        let cache_path = project_dir.join(".git").join("vibecheck-cache.db");
        let cache_conn = open_cache(&cache_path, "test-model", 64, 16000, false).unwrap();
        insert_embedding(&cache_conn, "hash_a", &[1; 256]).unwrap();
        insert_embedding(&cache_conn, "hash_b", &[2; 256]).unwrap();
        insert_embedding(&cache_conn, "hash_c", &[3; 256]).unwrap();
        insert_embedding(&cache_conn, "hash_d", &[4; 256]).unwrap();
        close_cache(cache_conn).unwrap();

        // Prune should remove hash_c and hash_d (unreferenced)
        let result = prune_cache(&cache_path, &project_dir).unwrap();

        assert_eq!(
            result.entries_removed, 2,
            "Should have removed 2 unreferenced entries"
        );
        // bytes_freed may be 0 for very small databases where VACUUM doesn't shrink the file
        // The important check is entries_removed

        // Verify remaining entries
        let conn = Connection::open(&cache_path).unwrap();
        let count: usize = conn
            .query_row("SELECT COUNT(*) FROM embeddings", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2, "Should have 2 entries remaining");
    }

    #[test]
    fn prune_cache_all_referenced_removes_nothing() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project_dir = tmp.path().join("project");
        std::fs::create_dir_all(&project_dir).unwrap();
        create_git_repo(&project_dir);

        // Worktree DB references the same hashes as the cache
        create_worktree_db(&project_dir, &["hash_a", "hash_b"]);

        let cache_path = project_dir.join(".git").join("vibecheck-cache.db");
        let cache_conn = open_cache(&cache_path, "test-model", 64, 16000, false).unwrap();
        insert_embedding(&cache_conn, "hash_a", &[1; 64]).unwrap();
        insert_embedding(&cache_conn, "hash_b", &[2; 64]).unwrap();
        close_cache(cache_conn).unwrap();

        let result = prune_cache(&cache_path, &project_dir).unwrap();

        assert_eq!(
            result.entries_removed, 0,
            "No entries should be removed when all are referenced"
        );
    }

    #[test]
    fn prune_cache_no_worktree_db_removes_all() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project_dir = tmp.path().join("project");
        std::fs::create_dir_all(&project_dir).unwrap();
        create_git_repo(&project_dir);

        // No .vibecheck.db in the worktree
        let cache_path = project_dir.join(".git").join("vibecheck-cache.db");
        let cache_conn = open_cache(&cache_path, "test-model", 64, 16000, false).unwrap();
        insert_embedding(&cache_conn, "hash_a", &[1; 64]).unwrap();
        insert_embedding(&cache_conn, "hash_b", &[2; 64]).unwrap();
        close_cache(cache_conn).unwrap();

        let result = prune_cache(&cache_path, &project_dir).unwrap();

        assert_eq!(
            result.entries_removed, 2,
            "All entries removed when no worktree DB exists"
        );
    }

    #[test]
    fn prune_cache_not_a_git_repo_errors() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache_path = tmp.path().join("vibecheck-cache.db");

        // Create a dummy cache file so the "no cache" check doesn't trigger first
        std::fs::write(&cache_path, b"dummy").unwrap();

        let result = prune_cache(&cache_path, tmp.path());
        assert!(result.is_err(), "Should error outside a git repo");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Not a git repository"),
            "Error should mention not a git repo, got: {err}"
        );
    }

    #[test]
    fn prune_cache_no_cache_exists_errors() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project_dir = tmp.path().join("project");
        std::fs::create_dir_all(&project_dir).unwrap();
        create_git_repo(&project_dir);

        let cache_path = project_dir.join(".git").join("vibecheck-cache.db");
        // Don't create the cache file

        let result = prune_cache(&cache_path, &project_dir);
        assert!(result.is_err(), "Should error when no cache exists");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("No cache found"),
            "Error should mention no cache found, got: {err}"
        );
    }

    #[test]
    fn prune_cache_empty_cache_returns_zero() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project_dir = tmp.path().join("project");
        std::fs::create_dir_all(&project_dir).unwrap();
        create_git_repo(&project_dir);

        // Create an empty cache (no entries)
        let cache_path = project_dir.join(".git").join("vibecheck-cache.db");
        let cache_conn = open_cache(&cache_path, "test-model", 64, 16000, false).unwrap();
        close_cache(cache_conn).unwrap();

        let result = prune_cache(&cache_path, &project_dir).unwrap();

        assert_eq!(result.entries_removed, 0);
    }
}
