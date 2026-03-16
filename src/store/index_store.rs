use crate::error::VibecheckError;
use crate::parser::types::{ChunkType, FunctionChunk};
use crate::store::types::StoredFunction;
use crate::util::hash::sha256;
use rusqlite::Connection;
use std::collections::HashSet;

/// A lightweight version of StoredFunction for scan operations.
/// Omits source_text and params_json to reduce memory usage.
pub struct SlimFunction {
    pub id: String,
    pub file_path: String,
    pub function_name: String,
    pub start_line: usize,
    pub end_line: usize,
    pub signature: String,
    pub signature_hash: String,
    pub chunk_type: ChunkType,
    pub context: Option<String>,
    pub tokens: HashSet<String>,
    pub embedding: Option<Vec<u8>>,
}

impl SlimFunction {
    pub fn line_count(&self) -> usize {
        self.end_line.saturating_sub(self.start_line) + 1
    }
}

pub fn upsert_functions(
    conn: &Connection,
    chunks: &[FunctionChunk],
) -> Result<(), VibecheckError> {
    let mut stmt = conn.prepare_cached(
        "INSERT INTO functions
            (id, file_path, function_name, source_text, start_line, end_line,
             params_json, return_type, is_exported, signature_hash, content_hash,
             embedding, chunk_type, context, signature, tokens_json)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
            file_path=excluded.file_path, function_name=excluded.function_name,
            source_text=excluded.source_text, start_line=excluded.start_line,
            end_line=excluded.end_line, params_json=excluded.params_json,
            return_type=excluded.return_type, is_exported=excluded.is_exported,
            signature_hash=excluded.signature_hash, content_hash=excluded.content_hash,
            embedding=NULL, chunk_type=excluded.chunk_type, context=excluded.context,
            signature=excluded.signature, tokens_json=excluded.tokens_json",
    )?;

    for chunk in chunks {
        let params_json = serde_json::to_string(&chunk.params).unwrap_or_default();
        let hash = sha256(&chunk.source_text);
        let tokens_json = serde_json::to_string(&chunk.tokens).unwrap_or_default();

        stmt.execute(rusqlite::params![
            chunk.id,
            chunk.file_path,
            chunk.function_name,
            chunk.source_text,
            chunk.start_line as i64,
            chunk.end_line as i64,
            params_json,
            chunk.return_type,
            chunk.is_exported as i32,
            chunk.signature_hash,
            hash,
            chunk.chunk_type.as_str(),
            chunk.context,
            chunk.signature,
            tokens_json,
        ])?;
    }

    Ok(())
}

pub fn vec_table_exists(conn: &Connection) -> Result<bool, VibecheckError> {
    Ok(conn
        .prepare_cached("SELECT 1 FROM sqlite_master WHERE type='table' AND name='vec_functions'")?
        .exists([])?)
}

pub fn delete_functions_for_file(
    conn: &Connection,
    file_path: &str,
    vec_exists: bool,
) -> Result<(), VibecheckError> {
    if vec_exists {
        conn.prepare_cached(
            "DELETE FROM vec_functions WHERE rowid IN (SELECT rowid FROM functions WHERE file_path = ?)"
        )?.execute([file_path])?;
    }
    conn.prepare_cached("DELETE FROM functions WHERE file_path = ?")?
        .execute([file_path])?;
    Ok(())
}

pub fn get_function_by_signature_hash(
    conn: &Connection,
    hash: &str,
) -> Result<Option<StoredFunction>, VibecheckError> {
    match conn
        .prepare_cached("SELECT * FROM functions WHERE signature_hash = ? LIMIT 1")?
        .query_row([hash], map_row)
    {
        Ok(func) => Ok(Some(func)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn get_all_functions(conn: &Connection) -> Result<Vec<StoredFunction>, VibecheckError> {
    let mut stmt = conn.prepare_cached("SELECT * FROM functions")?;
    let rows = stmt.query_map([], map_row)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_embedded_functions(conn: &Connection) -> Result<Vec<StoredFunction>, VibecheckError> {
    let mut stmt = conn.prepare_cached("SELECT * FROM functions WHERE embedding IS NOT NULL")?;
    let rows = stmt.query_map([], map_row)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Like `get_embedded_functions` but only fetches columns needed for scan operations.
/// Avoids loading source_text and params_json, significantly reducing memory for large codebases.
pub fn get_embedded_functions_slim(conn: &Connection) -> Result<Vec<SlimFunction>, VibecheckError> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, file_path, function_name, start_line, end_line,
                signature, signature_hash, chunk_type, context, tokens_json, embedding
         FROM functions WHERE embedding IS NOT NULL"
    )?;
    let rows = stmt.query_map([], map_row_slim)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn count_functions(conn: &Connection) -> Result<usize, VibecheckError> {
    let count: i64 =
        conn.prepare_cached("SELECT COUNT(*) FROM functions")?
            .query_row([], |row| row.get(0))?;
    Ok(count as usize)
}

pub fn get_functions_without_embeddings(
    conn: &Connection,
) -> Result<Vec<StoredFunction>, VibecheckError> {
    let mut stmt = conn.prepare_cached("SELECT * FROM functions WHERE embedding IS NULL")?;
    let rows = stmt.query_map([], map_row)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn count_functions_without_embeddings(conn: &Connection) -> Result<usize, VibecheckError> {
    let count: i64 = conn
        .prepare_cached("SELECT COUNT(*) FROM functions WHERE embedding IS NULL")?
        .query_row([], |row| row.get(0))?;
    Ok(count as usize)
}

pub fn update_embedding(
    conn: &Connection,
    function_id: &str,
    embedding: &[f32],
    vec_exists: bool,
) -> Result<(), VibecheckError> {
    let bytes = embedding_to_bytes(embedding);
    conn.prepare_cached("UPDATE functions SET embedding = ? WHERE id = ?")?
        .execute(rusqlite::params![bytes, function_id])?;

    if vec_exists {
        let rowid: i64 = conn.query_row(
            "SELECT rowid FROM functions WHERE id = ?",
            [function_id],
            |row| row.get(0),
        )?;
        conn.prepare_cached(
            "INSERT OR REPLACE INTO vec_functions (rowid, embedding) VALUES (?, ?)"
        )?.execute(rusqlite::params![rowid, bytes])?;
    }

    Ok(())
}

pub fn query_knn(
    conn: &Connection,
    query_embedding: &[f32],
    top_k: usize,
    threshold: f64,
    vec_exists: bool,
) -> Result<Vec<(StoredFunction, f64)>, VibecheckError> {
    query_knn_impl(conn, &embedding_to_bytes(query_embedding), top_k, threshold, vec_exists)
}

pub fn query_knn_raw(
    conn: &Connection,
    embedding_bytes: &[u8],
    top_k: usize,
    threshold: f64,
    vec_exists: bool,
) -> Result<Vec<(StoredFunction, f64)>, VibecheckError> {
    query_knn_impl(conn, embedding_bytes, top_k, threshold, vec_exists)
}

fn query_knn_impl(
    conn: &Connection,
    embedding_bytes: &[u8],
    top_k: usize,
    threshold: f64,
    vec_exists: bool,
) -> Result<Vec<(StoredFunction, f64)>, VibecheckError> {
    if vec_exists {
        // Use indexed KNN via vec0 virtual table
        let mut stmt = conn.prepare_cached(
            "SELECT f.id, f.file_path, f.function_name, f.source_text,
                    f.start_line, f.end_line, f.params_json, f.return_type,
                    f.is_exported, f.signature_hash, f.content_hash,
                    f.embedding, f.chunk_type, f.context, f.signature,
                    f.tokens_json, vf.distance
             FROM vec_functions vf
             JOIN functions f ON f.rowid = vf.rowid
             WHERE vf.embedding MATCH ? AND k = ?"
        )?;
        let rows: Vec<(StoredFunction, f64)> = stmt
            .query_map(rusqlite::params![embedding_bytes, top_k as i64], |row| {
                let func = map_row(row)?;
                let distance: f64 = row.get("distance")?;
                Ok((func, distance))
            })?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        Ok(rows.into_iter().filter(|(_, dist)| *dist <= threshold).collect())
    } else {
        // Fallback: brute-force scan (no vec0 table yet)
        let mut stmt = conn.prepare_cached(
            "SELECT *, vec_distance_cosine(embedding, ?) AS distance
             FROM functions
             WHERE embedding IS NOT NULL
             ORDER BY distance ASC
             LIMIT ?",
        )?;
        let rows: Vec<_> = stmt
            .query_map(rusqlite::params![embedding_bytes, top_k as i64], |row| {
                let func = map_row(row)?;
                let distance: f64 = row.get("distance")?;
                Ok((func, distance))
            })?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        Ok(rows.into_iter().filter(|(_, dist)| *dist <= threshold).collect())
    }
}

pub fn get_all_signature_hashes(conn: &Connection) -> Result<HashSet<String>, VibecheckError> {
    let mut stmt = conn.prepare_cached("SELECT signature_hash FROM functions")?;
    let hashes = stmt.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<HashSet<_>, _>>()?;
    Ok(hashes)
}

fn embedding_to_bytes(embedding: &[f32]) -> Vec<u8> {
    // Note: a zero-copy alternative (bytemuck::cast_slice) would avoid this allocation,
    // but adding a dependency for this isn't worthwhile at current scale.
    embedding
        .iter()
        .flat_map(|f| f.to_le_bytes())
        .collect()
}

pub fn bytes_to_embedding(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

fn parse_chunk_type(s: &str) -> ChunkType {
    match s {
        "block" => ChunkType::Block,
        _ => ChunkType::Function,
    }
}

fn deserialize_tokens(json: &str) -> HashSet<String> {
    serde_json::from_str(json).unwrap_or_default()
}

fn map_row(row: &rusqlite::Row) -> rusqlite::Result<StoredFunction> {
    let is_exported_int: i32 = row.get("is_exported")?;
    let start_line: i64 = row.get("start_line")?;
    let end_line: i64 = row.get("end_line")?;
    let tokens_json: String = row
        .get::<_, Option<String>>("tokens_json")?
        .unwrap_or_else(|| "[]".into());
    let chunk_type_str: String = row.get::<_, Option<String>>("chunk_type")?.unwrap_or_else(|| "function".into());
    Ok(StoredFunction {
        id: row.get("id")?,
        file_path: row.get("file_path")?,
        function_name: row.get("function_name")?,
        source_text: row.get("source_text")?,
        start_line: start_line as usize,
        end_line: end_line as usize,
        params_json: row.get("params_json")?,
        return_type: row.get("return_type")?,
        is_exported: is_exported_int != 0,
        signature_hash: row.get("signature_hash")?,
        content_hash: row.get("content_hash")?,
        embedding: row.get("embedding")?,
        chunk_type: parse_chunk_type(&chunk_type_str),
        context: row.get("context")?,
        signature: row.get::<_, Option<String>>("signature")?
            .unwrap_or_default(),
        tokens: deserialize_tokens(&tokens_json),
    })
}

fn map_row_slim(row: &rusqlite::Row) -> rusqlite::Result<SlimFunction> {
    let start_line: i64 = row.get("start_line")?;
    let end_line: i64 = row.get("end_line")?;
    let tokens_json: String = row
        .get::<_, Option<String>>("tokens_json")?
        .unwrap_or_else(|| "[]".into());
    let chunk_type_str: String = row.get::<_, Option<String>>("chunk_type")?.unwrap_or_else(|| "function".into());
    Ok(SlimFunction {
        id: row.get("id")?,
        file_path: row.get("file_path")?,
        function_name: row.get("function_name")?,
        start_line: start_line as usize,
        end_line: end_line as usize,
        signature: row.get::<_, Option<String>>("signature")?.unwrap_or_default(),
        signature_hash: row.get("signature_hash")?,
        chunk_type: parse_chunk_type(&chunk_type_str),
        context: row.get("context")?,
        tokens: deserialize_tokens(&tokens_json),
        embedding: row.get("embedding")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::types::{ChunkType, ParamInfo};
    use crate::store::db::run_migrations_for_test;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations_for_test(&conn);

        // Insert a tracked file for FK constraint
        conn.execute(
            "INSERT INTO tracked_files (file_path, content_hash, mtime_ms, indexed_at)
             VALUES ('test.ts', 'abc123', 0, '2024-01-01T00:00:00Z')",
            [],
        )
        .unwrap();

        conn
    }

    fn make_chunk(name: &str, line: usize) -> FunctionChunk {
        FunctionChunk {
            id: format!("test.ts:{name}:{line}"),
            file_path: "test.ts".into(),
            function_name: name.into(),
            source_text: format!("function {name}() {{ return 1; }}"),
            start_line: line,
            end_line: line + 3,
            params: vec![ParamInfo {
                name: "x".into(),
                type_: Some("number".into()),
            }],
            return_type: Some("number".into()),
            is_exported: false,
            signature_hash: format!("{:016x}", line),
            signature: "(x: number) => number".into(),
            tokens: ["x".to_string(), "number".to_string()].into_iter().collect(),
            chunk_type: ChunkType::Function,
            context: None,
        }
    }

    #[test]
    fn upsert_and_count() {
        let conn = setup_db();
        let chunks = vec![make_chunk("foo", 1), make_chunk("bar", 5)];
        upsert_functions(&conn, &chunks).unwrap();
        assert_eq!(count_functions(&conn).unwrap(), 2);
    }

    #[test]
    fn delete_for_file() {
        let conn = setup_db();
        upsert_functions(&conn, &[make_chunk("foo", 1)]).unwrap();
        assert_eq!(count_functions(&conn).unwrap(), 1);

        delete_functions_for_file(&conn, "test.ts", false).unwrap();
        assert_eq!(count_functions(&conn).unwrap(), 0);
    }

    #[test]
    fn get_unembedded() {
        let conn = setup_db();
        upsert_functions(&conn, &[make_chunk("foo", 1)]).unwrap();

        let unembedded = get_functions_without_embeddings(&conn).unwrap();
        assert_eq!(unembedded.len(), 1);
    }

    #[test]
    fn update_and_retrieve_embedding() {
        let conn = setup_db();
        upsert_functions(&conn, &[make_chunk("foo", 1)]).unwrap();

        let embedding = vec![0.1f32, 0.2, 0.3, 0.4];
        update_embedding(&conn, "test.ts:foo:1", &embedding, false).unwrap();

        let unembedded = get_functions_without_embeddings(&conn).unwrap();
        assert!(unembedded.is_empty());

        let all = get_all_functions(&conn).unwrap();
        assert!(all[0].embedding.is_some());

        let stored_bytes = all[0].embedding.as_ref().unwrap();
        let restored = bytes_to_embedding(stored_bytes);
        assert_eq!(restored, embedding);
    }

    #[test]
    fn find_by_signature_hash() {
        let conn = setup_db();
        upsert_functions(&conn, &[make_chunk("foo", 1)]).unwrap();

        let found = get_function_by_signature_hash(&conn, "0000000000000001").unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().function_name, "foo");

        let not_found = get_function_by_signature_hash(&conn, "ffffffffffffffff").unwrap();
        assert!(not_found.is_none());
    }

    #[test]
    fn signature_and_tokens_round_trip() {
        let conn = setup_db();
        let chunk = make_chunk("foo", 1);
        upsert_functions(&conn, &[chunk]).unwrap();

        let all = get_all_functions(&conn).unwrap();
        assert_eq!(all[0].signature, "(x: number) => number");
        assert!(all[0].tokens.contains("number"));
    }
}
