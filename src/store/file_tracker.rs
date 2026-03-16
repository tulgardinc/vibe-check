use crate::error::VibecheckError;
use crate::store::types::FileRecord;
use crate::util::hash::sha256;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::time::UNIX_EPOCH;

pub fn get_tracked_files(conn: &Connection) -> Result<HashMap<String, FileRecord>, VibecheckError> {
    let mut stmt = conn.prepare_cached("SELECT * FROM tracked_files")?;
    let rows = stmt.query_map([], |row| {
        Ok(FileRecord {
            file_path: row.get("file_path")?,
            content_hash: row.get("content_hash")?,
            mtime_ms: row.get("mtime_ms")?,
            indexed_at: row.get("indexed_at")?,
        })
    })?;

    let mut map = HashMap::new();
    for row in rows {
        let row = row?;
        map.insert(row.file_path.clone(), row);
    }
    Ok(map)
}

pub fn upsert_tracked_file(
    conn: &Connection,
    file_path: &str,
    hash: &str,
    mtime_ms: i64,
) -> Result<(), VibecheckError> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.prepare_cached(
        "INSERT OR REPLACE INTO tracked_files (file_path, content_hash, mtime_ms, indexed_at)
         VALUES (?, ?, ?, ?)",
    )?
    .execute(rusqlite::params![file_path, hash, mtime_ms, now])?;
    Ok(())
}

pub fn remove_tracked_file(conn: &Connection, file_path: &str) -> Result<(), VibecheckError> {
    conn.prepare_cached("DELETE FROM tracked_files WHERE file_path = ?")?
        .execute([file_path])?;
    Ok(())
}

#[derive(Debug, Default)]
pub struct FileChanges {
    pub added: Vec<String>,
    pub modified: Vec<String>,
    pub deleted: Vec<String>,
    pub unchanged: Vec<String>,
    /// Source content for files read during change detection, to avoid re-reading in the parse step.
    pub cached_content: HashMap<String, String>,
}

pub fn compute_changed_files(
    conn: &Connection,
    file_paths: &[String],
) -> Result<FileChanges, VibecheckError> {
    let tracked = get_tracked_files(conn)?;
    let current_set: HashSet<&str> = file_paths.iter().map(|s| s.as_str()).collect();

    let mut changes = FileChanges::default();

    for fp in file_paths {
        match tracked.get(fp) {
            None => {
                changes.added.push(fp.clone());
            }
            Some(existing) => {
                let meta = fs::metadata(fp)?;
                let mtime_ms = meta
                    .modified()?
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64;

                if mtime_ms == existing.mtime_ms {
                    changes.unchanged.push(fp.clone());
                } else {
                    let source = fs::read_to_string(fp)?;
                    let hash = sha256(&source);
                    if hash == existing.content_hash {
                        changes.unchanged.push(fp.clone());
                    } else {
                        changes.modified.push(fp.clone());
                        changes.cached_content.insert(fp.clone(), source);
                    }
                }
            }
        }
    }

    for tracked_path in tracked.keys() {
        if !current_set.contains(tracked_path.as_str()) {
            changes.deleted.push(tracked_path.clone());
        }
    }

    Ok(changes)
}
