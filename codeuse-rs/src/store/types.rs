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
    pub chunk_type: String,
    pub context: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FileRecord {
    pub file_path: String,
    pub content_hash: String,
    pub mtime_ms: i64,
    pub indexed_at: String,
}
