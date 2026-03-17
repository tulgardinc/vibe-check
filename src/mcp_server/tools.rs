use crate::mcp_server::types::ToolInfo;

pub fn get_tool_list() -> Vec<ToolInfo> {
    vec![
        ToolInfo {
            name: "vibecheck_query".into(),
            description: "Find existing functions similar to new code".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "Path to source file" },
                    "source": { "type": "string", "description": "Source code to check" },
                    "topK": { "type": "number", "default": 5 },
                    "threshold": { "type": "number", "default": 0.3 },
                    "model": { "type": "string", "description": "Override embedding model name" },
                    "ollamaHost": { "type": "string", "description": "Override Ollama server URL" },
                    "contextLength": { "type": "number", "description": "Override model context length in tokens" },
                    "maxInputBytes": { "type": "number", "description": "Override max input bytes for truncation" },
                    "queryPrefix": { "type": "string", "description": "Override query prefix prepended to search inputs" },
                    "dimensions": { "type": "number", "description": "Override embedding dimensions" },
                    "db": { "type": "string", "description": "Override database file path" }
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
                    "force": { "type": "boolean", "default": false },
                    "model": { "type": "string", "description": "Override embedding model name" },
                    "ollamaHost": { "type": "string", "description": "Override Ollama server URL" },
                    "contextLength": { "type": "number", "description": "Override model context length in tokens" },
                    "maxInputBytes": { "type": "number", "description": "Override max input bytes for truncation" },
                    "queryPrefix": { "type": "string", "description": "Override query prefix prepended to search inputs" },
                    "dimensions": { "type": "number", "description": "Override embedding dimensions" },
                    "db": { "type": "string", "description": "Override database file path" }
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
                    "threshold": { "type": "number", "default": 0.25 },
                    "db": { "type": "string", "description": "Override database file path" }
                }
            }),
        },
        ToolInfo {
            name: "vibecheck_status".into(),
            description: "Report index health and statistics".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "db": { "type": "string", "description": "Override database file path" }
                }
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
