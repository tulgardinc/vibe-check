use vibecheck::core::index_pipeline::{run_index, IndexOptions};
use vibecheck::core::query_pipeline::{run_query, QueryOptions};
use vibecheck::core::scan_pipeline::{run_scan, ScanOptions};
use vibecheck::core::status_pipeline::{run_status, StatusOptions};
use vibecheck::ignore::ignore_file::{add_exclusion, load_ignore_file, save_ignore_file};
use vibecheck::ignore::types::{Exclusion, ExclusionPair, ExclusionSide};
use vibecheck::util::config::find_project_root;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

// --- Indexing state shared across tool calls ---

#[derive(Clone)]
enum IndexingStatus {
    Idle,
    Running {
        embedded: Arc<std::sync::atomic::AtomicUsize>,
        total: usize,
    },
    Done(String),
    Failed(String),
}

struct IndexingState {
    status: IndexingStatus,
    cancel: Arc<AtomicBool>,
}

impl IndexingState {
    fn new() -> Self {
        Self {
            status: IndexingStatus::Idle,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}

// --- JSON-RPC types ---

#[derive(Deserialize)]
#[allow(dead_code)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

#[derive(Serialize)]
struct ToolInfo {
    name: String,
    description: String,
    #[serde(rename = "inputSchema")]
    input_schema: Value,
}

#[derive(Serialize)]
struct TextContent {
    #[serde(rename = "type")]
    type_: String,
    text: String,
}

#[derive(Serialize)]
struct ToolResult {
    content: Vec<TextContent>,
    #[serde(rename = "isError", skip_serializing_if = "is_false")]
    is_error: bool,
}

fn is_false(v: &bool) -> bool {
    !v
}

fn main() {
    eprintln!("vibecheck MCP server running on stdio");

    let state = Arc::new(Mutex::new(IndexingState::new()));

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        if line.trim().is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let response = handle_request(&request, &state);

        if let Some(resp) = response {
            let json = serde_json::to_string(&resp).unwrap_or_default();
            let _ = writeln!(stdout, "{json}");
            let _ = stdout.flush();
        }
    }
}

fn handle_request(req: &JsonRpcRequest, state: &Arc<Mutex<IndexingState>>) -> Option<JsonRpcResponse> {
    let id = req.id.clone()?;

    let result = match req.method.as_str() {
        "initialize" => Ok(serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "vibecheck",
                "version": "0.1.0"
            }
        })),
        "notifications/initialized" => return None,
        "tools/list" => Ok(serde_json::json!({
            "tools": get_tool_list()
        })),
        "tools/call" => handle_tool_call(&req.params, state),
        _ => Err(format!("Unknown method: {}", req.method)),
    };

    Some(match result {
        Ok(val) => JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id,
            result: Some(val),
            error: None,
        },
        Err(msg) => JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32603,
                message: msg,
            }),
        },
    })
}

fn get_tool_list() -> Vec<ToolInfo> {
    vec![
        ToolInfo {
            name: "vibecheck_query".into(),
            description: "Find existing functions similar to new code".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "Path to TypeScript file" },
                    "source": { "type": "string", "description": "TypeScript source code" },
                    "topK": { "type": "number", "default": 5 },
                    "threshold": { "type": "number", "default": 0.3 }
                }
            }),
        },
        ToolInfo {
            name: "vibecheck_index".into(),
            description: "Build or update the semantic index. Runs in the background — call again to check progress. Parsing is fast; embedding takes ~2s per function. Already-embedded functions are not re-embedded unless force is true.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory to index" },
                    "force": { "type": "boolean", "default": false }
                }
            }),
        },
        ToolInfo {
            name: "vibecheck_index_stop".into(),
            description: "Stop a running index operation. Progress is saved — already-embedded functions are kept. Call vibecheck_index again to resume.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolInfo {
            name: "vibecheck_scan".into(),
            description: "Find all similar function pairs in the index".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "topN": { "type": "number", "default": 50 },
                    "threshold": { "type": "number", "default": 0.25 }
                }
            }),
        },
        ToolInfo {
            name: "vibecheck_status".into(),
            description: "Report index health and statistics".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolInfo {
            name: "vibecheck_add_exclusion".into(),
            description: "Exclude a pair from future results".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["queryFunction", "queryPath", "querySignatureHash",
                              "candidateFunction", "candidatePath", "candidateSignatureHash", "reason"],
                "properties": {
                    "queryFunction": { "type": "string" },
                    "queryPath": { "type": "string" },
                    "querySignatureHash": { "type": "string" },
                    "candidateFunction": { "type": "string" },
                    "candidatePath": { "type": "string" },
                    "candidateSignatureHash": { "type": "string" },
                    "reason": { "type": "string" }
                }
            }),
        },
    ]
}

fn handle_tool_call(params: &Value, state: &Arc<Mutex<IndexingState>>) -> Result<Value, String> {
    let tool_name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or("Missing tool name")?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or(Value::Object(Default::default()));

    let result = match tool_name {
        "vibecheck_query" => handle_query(&args),
        "vibecheck_index" => handle_index(&args, state),
        "vibecheck_index_stop" => handle_index_stop(state),
        "vibecheck_scan" => handle_scan(&args),
        "vibecheck_status" => handle_status(),
        "vibecheck_add_exclusion" => handle_add_exclusion(&args),
        _ => Err(format!("Unknown tool: {tool_name}")),
    };

    match result {
        Ok(text) => Ok(serde_json::to_value(ToolResult {
            content: vec![TextContent {
                type_: "text".into(),
                text,
            }],
            is_error: false,
        })
        .unwrap()),
        Err(e) => Ok(serde_json::to_value(ToolResult {
            content: vec![TextContent {
                type_: "text".into(),
                text: e,
            }],
            is_error: true,
        })
        .unwrap()),
    }
}

fn handle_query(args: &Value) -> Result<String, String> {
    let source = if let Some(file) = args.get("file").and_then(|v| v.as_str()) {
        std::fs::read_to_string(file).map_err(|e| format!("Failed to read file: {e}"))?
    } else if let Some(src) = args.get("source").and_then(|v| v.as_str()) {
        src.to_string()
    } else {
        return Err("Provide either \"file\" or \"source\"".into());
    };

    let top_k = args
        .get("topK")
        .and_then(|v| v.as_u64())
        .unwrap_or(5) as usize;
    let threshold = args
        .get("threshold")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.3);

    let result = run_query(QueryOptions {
        source,
        file_name: args.get("file").and_then(|v| v.as_str()).map(String::from),
        top_k,
        threshold,
        db_path: None,
        project_root: None,
        model: None,
        ollama_host: None,
    })
    .map_err(|e| e.to_string())?;

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

fn handle_index(args: &Value, state: &Arc<Mutex<IndexingState>>) -> Result<String, String> {
    // Check current status
    let current = {
        let s = state.lock().unwrap();
        s.status.clone()
    };

    match current {
        IndexingStatus::Running { embedded, total } => {
            let done = embedded.load(Ordering::Relaxed);
            Ok(format!(
                "Indexing in progress: {done}/{total} functions embedded. Call again to check progress."
            ))
        }
        IndexingStatus::Done(ref msg) => {
            let msg = msg.clone();
            // Reset to idle so next call starts fresh
            state.lock().unwrap().status = IndexingStatus::Idle;
            Ok(msg)
        }
        IndexingStatus::Failed(ref msg) => {
            let msg = msg.clone();
            state.lock().unwrap().status = IndexingStatus::Idle;
            Err(msg)
        }
        IndexingStatus::Idle => {
            // Start indexing in background
            let path = args.get("path").and_then(|v| v.as_str()).map(String::from);
            let force = args.get("force").and_then(|v| v.as_bool()).unwrap_or(false);

            let cancel = Arc::new(AtomicBool::new(false));
            let embedded = Arc::new(std::sync::atomic::AtomicUsize::new(0));

            let cancel_clone = Arc::clone(&cancel);
            let embedded_clone = Arc::clone(&embedded);
            let state_clone = Arc::clone(state);
            let embedded_for_start = Arc::clone(&embedded);

            // Store state before spawning
            {
                let mut s = state.lock().unwrap();
                s.cancel = Arc::clone(&cancel);
                // status will be set to Running once we know the total
                s.status = IndexingStatus::Running {
                    embedded: Arc::clone(&embedded),
                    total: 0,
                };
            }

            let state_for_total = Arc::clone(state);

            thread::spawn(move || {
                let result = run_index(IndexOptions {
                    path,
                    db_path: None,
                    force,
                    model: None,
                    ollama_host: None,
                    on_embed_start: Some(Box::new(move |total| {
                        let mut s = state_for_total.lock().unwrap();
                        s.status = IndexingStatus::Running {
                            embedded: Arc::clone(&embedded_for_start),
                            total,
                        };
                    })),
                    on_embed_progress: Some(Box::new(move |n| {
                        embedded_clone.fetch_add(n, Ordering::Relaxed);
                    })),
                    on_embed_done: None,
                    cancel: Some(cancel_clone),
                });

                let mut s = state_clone.lock().unwrap();
                match result {
                    Ok(r) => {
                        s.status = IndexingStatus::Done(format!(
                            "Indexing complete. {} functions from {} files ({} added, {} modified, {} deleted). Model: {} ({}, {}d).",
                            r.functions_indexed, r.files_scanned,
                            r.added, r.modified, r.deleted,
                            r.model, r.tier, r.dimensions
                        ));
                    }
                    Err(e) => {
                        let cancelled = s.cancel.load(Ordering::Relaxed);
                        if cancelled {
                            s.status = IndexingStatus::Done(
                                "Indexing stopped. Progress has been saved — call vibecheck_index to resume.".into()
                            );
                        } else {
                            s.status = IndexingStatus::Failed(e.to_string());
                        }
                    }
                }
                s.cancel = Arc::new(AtomicBool::new(false));
            });

            Ok("Indexing started. Call vibecheck_index again to check progress.".into())
        }
    }
}

fn handle_index_stop(state: &Arc<Mutex<IndexingState>>) -> Result<String, String> {
    let s = state.lock().unwrap();
    match s.status {
        IndexingStatus::Running { .. } => {
            s.cancel.store(true, Ordering::Relaxed);
            Ok("Stop requested. Indexing will stop after the current embedding completes. Progress is saved — call vibecheck_index to resume.".into())
        }
        _ => Ok("No indexing operation is running.".into()),
    }
}

fn handle_scan(args: &Value) -> Result<String, String> {
    let top_n = args
        .get("topN")
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as usize;
    let threshold = args
        .get("threshold")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.25);

    let result = run_scan(ScanOptions {
        top_n,
        threshold,
        db_path: None,
        project_root: None,
        on_progress: None,
    })
    .map_err(|e| e.to_string())?;

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

fn handle_status() -> Result<String, String> {
    let result = run_status(StatusOptions { db_path: None }).map_err(|e| e.to_string())?;

    if !result.exists {
        return Ok("No index found. Run vibecheck_index to create one.".into());
    }

    let unembedded = if result.unembedded > 0 {
        format!(" ({} awaiting embedding)", result.unembedded)
    } else {
        String::new()
    };

    let stale = if result.stale_exclusions > 0 {
        format!(" ({} stale)", result.stale_exclusions)
    } else {
        String::new()
    };

    Ok(format!(
        "Database: {} ({} MB)\nModel: {}\nDimensions: {}\nIndexed functions: {}{}\nTracked files: {}\nLast indexed: {}\nExclusions: {}{}",
        result.db_path, result.size_mb, result.model, result.dimensions,
        result.indexed_functions, unembedded, result.tracked_files,
        result.last_indexed, result.exclusions, stale
    ))
}

fn handle_add_exclusion(args: &Value) -> Result<String, String> {
    let get_str = |key: &str| -> Result<String, String> {
        args.get(key)
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or(format!("Missing required parameter: {key}"))
    };

    let query_function = get_str("queryFunction")?;
    let query_path = get_str("queryPath")?;
    let query_hash = get_str("querySignatureHash")?;
    let candidate_function = get_str("candidateFunction")?;
    let candidate_path = get_str("candidatePath")?;
    let candidate_hash = get_str("candidateSignatureHash")?;
    let reason = get_str("reason")?;

    let project_root = find_project_root(Path::new("."));
    let ignore_file = load_ignore_file(&project_root);

    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

    let updated = add_exclusion(
        &ignore_file,
        Exclusion {
            reason: reason.clone(),
            added: today,
            pair: ExclusionPair {
                a: ExclusionSide {
                    path: query_path,
                    function: query_function.clone(),
                    signature_hash: query_hash,
                },
                b: ExclusionSide {
                    path: candidate_path,
                    function: candidate_function.clone(),
                    signature_hash: candidate_hash,
                },
            },
        },
    );

    save_ignore_file(&project_root, &updated);

    Ok(format!(
        "Exclusion added: {} ↔ {} (\"{}\"). Total exclusions: {}.",
        query_function,
        candidate_function,
        reason,
        updated.exclusions.len()
    ))
}
