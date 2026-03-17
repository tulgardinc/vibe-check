use crate::core::index_pipeline::{run_index, IndexOptions, IndexProgress};
use crate::core::query_pipeline::{run_query, QueryOptions};
use crate::core::scan_pipeline::{run_scan, ScanOptions};
use crate::core::status_pipeline::{run_status, StatusOptions};
use crate::embedder::types::OllamaConfig;
use crate::ignore::ignore_file::{add_exclusion, load_ignore_file, save_ignore_file};
use crate::ignore::types::{Exclusion, ExclusionPair, ExclusionSide};
use crate::mcp_server::types::{IndexingState, IndexingStatus};
use crate::util::config::find_project_root;
use serde_json::Value;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

struct CommonOptions {
    db_path: Option<String>,
    ollama: OllamaConfig,
}

impl CommonOptions {
    fn from_args(args: &Value) -> Self {
        Self {
            db_path: args.get("db").and_then(|v| v.as_str()).map(String::from),
            ollama: OllamaConfig {
                model: args.get("model").and_then(|v| v.as_str()).map(String::from),
                host: args.get("ollamaHost").and_then(|v| v.as_str()).map(String::from),
                context_length: args.get("contextLength").and_then(|v| v.as_u64()).map(|v| v as usize),
                max_input_bytes: args.get("maxInputBytes").and_then(|v| v.as_u64()).map(|v| v as usize),
                query_prefix: args.get("queryPrefix").and_then(|v| v.as_str()).map(String::from),
            },
        }
    }
}

fn get_str(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or(format!("Missing required parameter: {key}"))
}

/// Lock the mutex, recovering from poison if needed.
fn lock_state(state: &Mutex<IndexingState>) -> std::sync::MutexGuard<'_, IndexingState> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn handle_query(args: &Value) -> Result<String, String> {
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
    let opts = CommonOptions::from_args(args);

    let result = run_query(QueryOptions {
        source,
        file_name: args.get("file").and_then(|v| v.as_str()).map(String::from),
        top_k,
        threshold,
        db_path: opts.db_path,
        project_root: None,
        ollama: opts.ollama,
        embedder: None,
    })
    .map_err(|e| e.to_string())?;

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

pub fn handle_index(args: &Value, state: &Arc<Mutex<IndexingState>>) -> Result<String, String> {
    let current = {
        let s = lock_state(state);
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
            lock_state(state).status = IndexingStatus::Idle;
            Ok(msg)
        }
        IndexingStatus::Failed(ref msg) => {
            let msg = msg.clone();
            lock_state(state).status = IndexingStatus::Idle;
            Err(msg)
        }
        IndexingStatus::Idle => {
            let path = args.get("path").and_then(|v| v.as_str()).map(String::from);
            let force = args
                .get("force")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let opts = CommonOptions::from_args(args);

            let cancel = Arc::new(AtomicBool::new(false));
            let embedded = Arc::new(std::sync::atomic::AtomicUsize::new(0));

            {
                let mut s = lock_state(state);
                s.cancel = Arc::clone(&cancel);
                s.status = IndexingStatus::Running {
                    embedded: Arc::clone(&embedded),
                    total: 0,
                };
            }

            struct McpProgress {
                embedded: Arc<std::sync::atomic::AtomicUsize>,
                state: Arc<Mutex<IndexingState>>,
            }

            impl IndexProgress for McpProgress {
                fn on_start(&self, total: usize) {
                    let mut s = lock_state(&self.state);
                    s.status = IndexingStatus::Running {
                        embedded: Arc::clone(&self.embedded),
                        total,
                    };
                }

                fn on_progress(&self, count: usize) {
                    self.embedded.fetch_add(count, Ordering::Relaxed);
                }
            }

            let cancel_clone = Arc::clone(&cancel);
            let state_clone = Arc::clone(state);

            thread::spawn(move || {
                let result = run_index(IndexOptions {
                    path,
                    db_path: opts.db_path,
                    force,
                    ollama: opts.ollama,
                    progress: Some(Box::new(McpProgress {
                        embedded: Arc::clone(&embedded),
                        state: Arc::clone(&state_clone),
                    })),
                    cancel: Some(cancel_clone),
                    embedder: None,
                });

                let mut s = lock_state(&state_clone);
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

pub fn handle_index_stop(state: &Arc<Mutex<IndexingState>>) -> Result<String, String> {
    let s = lock_state(state);
    match s.status {
        IndexingStatus::Running { .. } => {
            s.cancel.store(true, Ordering::Relaxed);
            Ok("Stop requested. Indexing will stop after the current embedding completes. Progress is saved — call vibecheck_index to resume.".into())
        }
        _ => Ok("No indexing operation is running.".into()),
    }
}

pub fn handle_scan(args: &Value) -> Result<String, String> {
    let top_n = args
        .get("topN")
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as usize;
    let threshold = args
        .get("threshold")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.25);
    let db_path = args.get("db").and_then(|v| v.as_str()).map(String::from);

    let result = run_scan(ScanOptions {
        top_n,
        threshold,
        db_path,
        project_root: None,
        on_progress: None,
    })
    .map_err(|e| e.to_string())?;

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

pub fn handle_status(args: &Value) -> Result<String, String> {
    let db_path = args.get("db").and_then(|v| v.as_str()).map(String::from);
    let result = run_status(StatusOptions { db_path }).map_err(|e| e.to_string())?;
    Ok(crate::output::formatter::format_status_human(&result))
}

pub fn handle_add_exclusion(args: &Value) -> Result<String, String> {
    let query_function = get_str(args, "queryFunction")?;
    let query_path = get_str(args, "queryPath")?;
    let query_hash = get_str(args, "querySignatureHash")?;
    let candidate_function = get_str(args, "candidateFunction")?;
    let candidate_path = get_str(args, "candidatePath")?;
    let candidate_hash = get_str(args, "candidateSignatureHash")?;
    let reason = get_str(args, "reason")?;

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
        "Exclusion added: {} \u{2194} {} (\"{}\"). Total exclusions: {}.",
        query_function,
        candidate_function,
        reason,
        updated.exclusions.len()
    ))
}
