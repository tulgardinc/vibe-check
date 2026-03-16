use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChunkType {
    Function,
    Block,
}

impl ChunkType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChunkType::Function => "function",
            ChunkType::Block => "block",
        }
    }
}

#[derive(Debug, Clone)]
pub struct FunctionChunk {
    pub id: String,
    pub file_path: String,
    pub function_name: String,
    pub source_text: String,
    pub start_line: usize,
    pub end_line: usize,
    pub params: Vec<ParamInfo>,
    pub return_type: Option<String>,
    pub is_exported: bool,
    pub signature_hash: String,
    pub signature: String,
    pub tokens: HashSet<String>,
    pub chunk_type: ChunkType,
    pub context: Option<String>,
}

impl FunctionChunk {
    pub fn line_count(&self) -> usize {
        self.end_line.saturating_sub(self.start_line) + 1
    }
}

#[derive(Debug)]
pub struct ParsedFile {
    pub file_path: String,
    pub source: String,
    pub chunks: Vec<FunctionChunk>,
    pub parse_errors: Vec<String>,
}
