use crate::parser::types::ChunkType;
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct StoredFunction {
    pub id: String,
    pub file_path: String,
    pub function_name: String,
    pub source_text: String,
    pub start_line: i64,
    pub end_line: i64,
    pub params_json: String,
    pub return_type: Option<String>,
    pub is_exported: bool,
    pub signature_hash: String,
    pub content_hash: String,
    pub embedding: Option<Vec<u8>>,
    pub chunk_type: ChunkType,
    pub context: Option<String>,
    pub signature: String,
    pub tokens: HashSet<String>,
}

impl StoredFunction {
    pub fn line_count(&self) -> usize {
        (self.end_line - self.start_line + 1).max(0) as usize
    }
}

/// Returns true if a stored function should be filtered out as a self-match or related chunk.
pub fn should_filter_neighbor(
    query_id: &str,
    query_file_path: &str,
    query_chunk_type: ChunkType,
    query_context: Option<&str>,
    query_name: &str,
    neighbor: &StoredFunction,
) -> bool {
    if neighbor.id == query_id {
        return true;
    }
    if neighbor.file_path == query_file_path
        && is_related_chunk(
            query_chunk_type,
            query_context,
            query_name,
            neighbor.chunk_type,
            neighbor.context.as_deref(),
            &neighbor.function_name,
        )
    {
        return true;
    }
    false
}

/// Returns true if two chunks in the same file are related (parent-child or siblings
/// within the same function), which makes them noise rather than real duplicates.
/// Both arguments must share the same `file_path` — caller is responsible for that check.
fn is_related_chunk(
    a_chunk_type: ChunkType,
    a_context: Option<&str>,
    a_name: &str,
    b_chunk_type: ChunkType,
    b_context: Option<&str>,
    b_name: &str,
) -> bool {
    // Block is a child of the function
    if a_chunk_type == ChunkType::Block && b_chunk_type == ChunkType::Function && a_context == Some(b_name) {
        return true;
    }
    if b_chunk_type == ChunkType::Block && a_chunk_type == ChunkType::Function && b_context == Some(a_name) {
        return true;
    }
    // Two blocks that share the same parent function
    if a_chunk_type == ChunkType::Block && b_chunk_type == ChunkType::Block && a_context.is_some() && a_context == b_context {
        return true;
    }
    false
}

#[derive(Debug, Clone)]
pub struct FileRecord {
    pub file_path: String,
    pub content_hash: String,
    pub mtime_ms: i64,
    pub indexed_at: String,
}
