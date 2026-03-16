use vibecheck::mcp_server::handlers;
use vibecheck::mcp_server::tools::get_tool_list;
use vibecheck::mcp_server::types::*;
use serde_json::Value;
use std::io::{self, BufRead, Write};
use std::sync::{Arc, Mutex};

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
            Err(e) => {
                eprintln!("vibecheck: malformed JSON-RPC request: {e}");
                continue;
            }
        };

        if request.jsonrpc != "2.0" {
            let err_resp = JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: request.id.unwrap_or(Value::Null),
                result: None,
                error: Some(JsonRpcError {
                    code: -32600,
                    message: "Invalid Request: jsonrpc must be \"2.0\"".into(),
                }),
            };
            if !write_response(&mut stdout, &err_resp) {
                break;
            }
            continue;
        }

        let response = handle_request(&request, &state);

        if let Some(resp) = response
            && !write_response(&mut stdout, &resp)
        {
            break; // stdout closed — client disconnected
        }
    }
}

/// Write a JSON-RPC response to stdout. Returns false if writing fails (client disconnected).
fn write_response(stdout: &mut io::Stdout, resp: &JsonRpcResponse) -> bool {
    let json = match serde_json::to_string(resp) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("vibecheck: failed to serialize response: {e}");
            return true; // serialization error, but stdout is still open
        }
    };
    if writeln!(stdout, "{json}").is_err() || stdout.flush().is_err() {
        return false;
    }
    true
}

fn handle_request(
    req: &JsonRpcRequest,
    state: &Arc<Mutex<IndexingState>>,
) -> Option<JsonRpcResponse> {
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

fn handle_tool_call(
    params: &Value,
    state: &Arc<Mutex<IndexingState>>,
) -> Result<Value, String> {
    let tool_name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or("Missing tool name")?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or(Value::Object(Default::default()));

    let result = match tool_name {
        "vibecheck_query" => handlers::handle_query(&args),
        "vibecheck_index" => handlers::handle_index(&args, state),
        "vibecheck_index_stop" => handlers::handle_index_stop(state),
        "vibecheck_scan" => handlers::handle_scan(&args),
        "vibecheck_status" => handlers::handle_status(&args),
        "vibecheck_add_exclusion" => handlers::handle_add_exclusion(&args),
        _ => Err(format!("Unknown tool: {tool_name}")),
    };

    match result {
        Ok(text) => Ok(serde_json::to_value(ToolResult::success(text)).unwrap()),
        Err(e) => Ok(serde_json::to_value(ToolResult::error(e)).unwrap()),
    }
}
