use crate::error::CodeuseError;
use rusqlite::Connection;
use std::sync::Once;

static VEC_INIT: Once = Once::new();

/// Register sqlite-vec as an auto-extension. Called once, applies to all future connections.
fn ensure_vec_registered() {
    VEC_INIT.call_once(|| {
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite_vec::sqlite3_vec_init as *const (),
            )));
        }
    });
}

pub fn open_database(db_path: &str) -> Result<Connection, CodeuseError> {
    ensure_vec_registered();

    let conn = Connection::open(db_path)?;
    conn.execute_batch("PRAGMA journal_mode = WAL;")?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    ensure_schema(&conn)?;
    Ok(conn)
}

pub fn open_database_no_vec(db_path: &str) -> Result<Connection, CodeuseError> {
    let conn = Connection::open(db_path)?;
    conn.execute_batch("PRAGMA journal_mode = WAL;")?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    ensure_schema(&conn)?;
    Ok(conn)
}

fn ensure_schema(conn: &Connection) -> Result<(), CodeuseError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS index_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS tracked_files (
            file_path TEXT PRIMARY KEY,
            content_hash TEXT NOT NULL,
            mtime_ms INTEGER NOT NULL,
            indexed_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS functions (
            id TEXT PRIMARY KEY,
            file_path TEXT NOT NULL,
            function_name TEXT NOT NULL,
            source_text TEXT NOT NULL,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            params_json TEXT NOT NULL,
            return_type TEXT,
            is_exported INTEGER NOT NULL,
            signature_hash TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            embedding BLOB,
            FOREIGN KEY (file_path) REFERENCES tracked_files(file_path) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_functions_file ON functions(file_path);
        CREATE INDEX IF NOT EXISTS idx_functions_sig ON functions(signature_hash);
        ",
    )?;

    // Migration: add chunk_type and context columns if missing
    let has_chunk_type = conn
        .prepare("PRAGMA table_info(functions)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(|r| r.ok())
        .any(|name| name == "chunk_type");

    if !has_chunk_type {
        conn.execute_batch(
            "ALTER TABLE functions ADD COLUMN chunk_type TEXT NOT NULL DEFAULT 'function';",
        )?;
    }

    let has_context = conn
        .prepare("PRAGMA table_info(functions)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(|r| r.ok())
        .any(|name| name == "context");

    if !has_context {
        conn.execute_batch("ALTER TABLE functions ADD COLUMN context TEXT;")?;
    }

    Ok(())
}

/// Test-only helper to create schema without loading sqlite-vec
#[cfg(test)]
pub fn ensure_schema_for_test(conn: &Connection) {
    ensure_schema(conn).unwrap();
}

pub fn get_meta_value(conn: &Connection, key: &str) -> Option<String> {
    conn.prepare_cached("SELECT value FROM index_meta WHERE key = ?")
        .ok()?
        .query_row([key], |row| row.get(0))
        .ok()
}

pub fn set_meta_value(
    conn: &Connection,
    key: &str,
    value: &str,
) -> Result<(), CodeuseError> {
    conn.prepare_cached("INSERT OR REPLACE INTO index_meta (key, value) VALUES (?, ?)")?
        .execute([key, value])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_and_schema() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        ensure_schema(&conn).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='functions'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn meta_value_round_trip() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();

        set_meta_value(&conn, "model_name", "nomic-embed-code").unwrap();
        assert_eq!(
            get_meta_value(&conn, "model_name"),
            Some("nomic-embed-code".into())
        );

        assert_eq!(get_meta_value(&conn, "nonexistent"), None);
    }

    #[test]
    fn migration_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();
        ensure_schema(&conn).unwrap();
    }

    #[test]
    fn vec_extension_loads() {
        ensure_vec_registered();
        let conn = Connection::open_in_memory().unwrap();
        let version: String = conn
            .query_row("SELECT vec_version()", [], |row| row.get(0))
            .unwrap();
        assert!(!version.is_empty());
    }
}
