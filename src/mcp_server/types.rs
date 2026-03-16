use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

// --- Indexing state shared across tool calls ---

#[derive(Clone)]
pub enum IndexingStatus {
    Idle,
    Running {
        embedded: Arc<std::sync::atomic::AtomicUsize>,
        total: usize,
    },
    Done(String),
    Failed(String),
}

pub struct IndexingState {
    pub status: IndexingStatus,
    pub cancel: Arc<AtomicBool>,
}

impl Default for IndexingState {
    fn default() -> Self {
        Self::new()
    }
}

impl IndexingState {
    pub fn new() -> Self {
        Self {
            status: IndexingStatus::Idle,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}

// --- JSON-RPC types ---

#[derive(Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

#[derive(Serialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

#[derive(Serialize)]
pub struct TextContent {
    #[serde(rename = "type")]
    pub type_: String,
    pub text: String,
}

#[derive(Serialize)]
pub struct ToolResult {
    pub content: Vec<TextContent>,
    #[serde(rename = "isError", skip_serializing_if = "is_false")]
    pub is_error: bool,
}

impl ToolResult {
    pub fn success(text: impl Into<String>) -> Self {
        Self {
            content: vec![TextContent {
                type_: "text".into(),
                text: text.into(),
            }],
            is_error: false,
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            content: vec![TextContent {
                type_: "text".into(),
                text: text.into(),
            }],
            is_error: true,
        }
    }
}

fn is_false(v: &bool) -> bool {
    !v
}
