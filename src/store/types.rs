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

impl StoredFunction {
    pub fn line_count(&self) -> usize {
        (self.end_line - self.start_line + 1).max(0) as usize
    }

    pub fn signature(&self) -> String {
        if self.chunk_type == "block" {
            return format!("<block> ({} lines)", self.line_count());
        }

        let params = self.format_params();
        match &self.return_type {
            Some(rt) => format!("({params}) => {rt}"),
            None => format!("({params})"),
        }
    }

    fn format_params(&self) -> String {
        #[derive(serde::Deserialize)]
        struct Param {
            name: String,
            #[serde(rename = "type")]
            type_: Option<String>,
        }

        let params: Vec<Param> = serde_json::from_str(&self.params_json).unwrap_or_default();
        params
            .iter()
            .map(|p| match &p.type_ {
                Some(t) => format!("{}: {t}", p.name),
                None => p.name.clone(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[derive(Debug, Clone)]
pub struct FileRecord {
    pub file_path: String,
    pub content_hash: String,
    pub mtime_ms: i64,
    pub indexed_at: String,
}
