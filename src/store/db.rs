use crate::error::VibecheckError;
use rusqlite::Connection;
use std::sync::Once;

static VEC_INIT: Once = Once::new();

/// Current schema version. Increment when adding new migrations.
const SCHEMA_VERSION: i32 = 4;

/// Register sqlite-vec as an auto-extension. Called once, applies to all future connections.
fn ensure_vec_registered() {
    VEC_INIT.call_once(|| {
        // SAFETY: `sqlite3_vec_init` is the entry-point function exported by the sqlite-vec
        // C extension.  Its signature matches the `sqlite3_auto_extension` callback type
        // (`fn(*mut sqlite3, *mut *mut c_char, *const sqlite3_api_routines) -> c_int`), but
        // Rust sees it with a different calling-convention wrapper, so a transmute is needed.
        //
        // This is the officially documented way to register sqlite-vec with rusqlite.  If the
        // sqlite-vec crate changes its init function signature, this will still compile but
        // produce undefined behaviour — pin the `sqlite-vec` version and audit on upgrades.
        unsafe {
            let f: unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut std::os::raw::c_char,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> std::os::raw::c_int = std::mem::transmute(sqlite_vec::sqlite3_vec_init as *const ());
            rusqlite::ffi::sqlite3_auto_extension(Some(f));
        }
    });
}

/// Common connection setup: WAL mode, busy timeout, foreign keys.
fn configure_connection(conn: &Connection) -> Result<(), VibecheckError> {
    conn.busy_timeout(std::time::Duration::from_secs(30))?;
    conn.execute_batch("PRAGMA journal_mode = WAL;")?;
    conn.execute_batch("PRAGMA synchronous = NORMAL;")?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    run_migrations(conn)?;
    Ok(())
}

pub fn open_database(db_path: &str) -> Result<Connection, VibecheckError> {
    ensure_vec_registered();
    let conn = Connection::open(db_path)?;
    configure_connection(&conn)?;
    Ok(conn)
}

pub fn open_database_no_vec(db_path: &str) -> Result<Connection, VibecheckError> {
    let conn = Connection::open(db_path)?;
    configure_connection(&conn)?;
    Ok(conn)
}

fn get_schema_version(conn: &Connection) -> i32 {
    conn.pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap_or_else(|e| {
            eprintln!("Warning: failed to read schema version: {e}");
            0
        })
}

fn set_schema_version(conn: &Connection, version: i32) -> Result<(), VibecheckError> {
    conn.pragma_update(None, "user_version", version)?;
    Ok(())
}

/// Check whether a table has a given column. Used by schema migrations.
// Table name is always a compile-time constant — no injection risk.
fn has_column(conn: &Connection, table: &str, column: &str) -> bool {
    debug_assert!(table.chars().all(|c| c.is_alphanumeric() || c == '_'));
    conn.prepare(&format!("PRAGMA table_info({table})"))
        .and_then(|mut stmt| {
            stmt.query_map([], |row| row.get::<_, String>(1))
                .map(|rows| rows.filter_map(|r| r.ok()).any(|name| name == column))
        })
        .unwrap_or(false)
}

fn run_migrations(conn: &Connection) -> Result<(), VibecheckError> {
    let version = get_schema_version(conn);

    if version < 1 {
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
                is_exported INTEGER NOT NULL CHECK(is_exported IN (0, 1)),
                signature_hash TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                embedding BLOB,
                chunk_type TEXT NOT NULL DEFAULT 'function' CHECK(chunk_type IN ('function', 'block')),
                context TEXT,
                signature TEXT NOT NULL DEFAULT '',
                tokens_json TEXT NOT NULL DEFAULT '[]',
                FOREIGN KEY (file_path) REFERENCES tracked_files(file_path) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_functions_file ON functions(file_path);
            CREATE INDEX IF NOT EXISTS idx_functions_sig ON functions(signature_hash);
            ",
        )?;
        set_schema_version(conn, 1)?;
    }

    if version < 2 {
        if !has_column(conn, "functions", "signature") {
            conn.execute_batch(
                "ALTER TABLE functions ADD COLUMN signature TEXT NOT NULL DEFAULT '';
                 ALTER TABLE functions ADD COLUMN tokens_json TEXT NOT NULL DEFAULT '[]';",
            )?;
        }
        set_schema_version(conn, 2)?;
    }

    if version < 3 {
        // vec0 virtual table creation is deferred to ensure_vec_table()
        // because we need to know the embedding dimension, which is only
        // available after the first successful embedding run.
        set_schema_version(conn, 3)?;
    }

    if version < 4 {
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_functions_unembedded ON functions(id) WHERE embedding IS NULL;"
        )?;
        set_schema_version(conn, 4)?;
    }

    debug_assert_eq!(get_schema_version(conn), SCHEMA_VERSION);

    Ok(())
}

#[cfg(test)]
pub fn run_migrations_for_test(conn: &Connection) {
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    run_migrations(conn).unwrap();
}

/// Create the `vec_functions` virtual table for indexed KNN queries if it does
/// not already exist.  Must be called once the embedding dimension is known
/// (i.e. after the first successful embedding run).  Existing embeddings in
/// `functions` are back-filled into the virtual table on creation.
pub fn ensure_vec_table(conn: &Connection, dimensions: usize) -> Result<(), VibecheckError> {
    let table_exists: bool = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='vec_functions'")?
        .exists([])?;

    if !table_exists {
        let tx = conn.unchecked_transaction()?;

        tx.execute_batch(&format!(
            "CREATE VIRTUAL TABLE vec_functions USING vec0(\
                embedding float[{dimensions}] distance_metric=cosine\
            )"
        ))?;

        // Back-fill from existing embeddings
        tx.execute_batch(
            "INSERT INTO vec_functions (rowid, embedding)
             SELECT rowid, embedding FROM functions WHERE embedding IS NOT NULL"
        )?;

        tx.commit()?;
    }
    Ok(())
}

pub fn get_meta_value(
    conn: &Connection,
    key: &str,
) -> Result<Option<String>, VibecheckError> {
    match conn
        .prepare_cached("SELECT value FROM index_meta WHERE key = ?")?
        .query_row([key], |row| row.get(0))
    {
        Ok(val) => Ok(Some(val)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn set_meta_value(
    conn: &Connection,
    key: &str,
    value: &str,
) -> Result<(), VibecheckError> {
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
        run_migrations(&conn).unwrap();

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
        run_migrations(&conn).unwrap();

        set_meta_value(&conn, "model_name", "nomic-embed-code").unwrap();
        assert_eq!(
            get_meta_value(&conn, "model_name").unwrap(),
            Some("nomic-embed-code".into())
        );

        assert_eq!(get_meta_value(&conn, "nonexistent").unwrap(), None);
    }

    #[test]
    fn migration_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap();
    }

    #[test]
    fn schema_version_is_set() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        assert_eq!(get_schema_version(&conn), SCHEMA_VERSION);
    }

    #[test]
    fn has_column_works() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        assert!(has_column(&conn, "functions", "chunk_type"));
        assert!(has_column(&conn, "functions", "context"));
        assert!(!has_column(&conn, "functions", "nonexistent"));
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
