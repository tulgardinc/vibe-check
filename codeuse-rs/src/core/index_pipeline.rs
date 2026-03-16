use crate::embedder::ollama_client::OllamaClient;
use crate::embedder::types::Embedder;
use crate::error::CodeuseError;
use crate::output::types::IndexResult;
use crate::parser::chunker::parse_file;
use crate::store::db::{get_meta_value, open_database, set_meta_value};
use crate::store::file_tracker::{
    compute_changed_files, remove_tracked_file, upsert_tracked_file,
};
use crate::store::index_store::{
    delete_functions_for_file, get_functions_without_embeddings, update_embedding,
    upsert_functions,
};
use crate::util::config::{find_project_root, find_typescript_files, resolve_db_path};
use crate::util::hash::content_hash;
use crate::util::logger;
use rayon::prelude::*;
use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

pub struct IndexOptions {
    pub path: Option<String>,
    pub db_path: Option<String>,
    pub force: bool,
    pub on_progress: Option<Box<dyn Fn(&str)>>,
}

pub fn run_index(options: IndexOptions) -> Result<IndexResult, CodeuseError> {
    let start_dir = options
        .path
        .as_deref()
        .map(Path::new)
        .unwrap_or_else(|| Path::new("."));
    let project_root = find_project_root(start_dir);
    let db_path = resolve_db_path(&project_root, options.db_path.as_deref());
    let scan_path = options
        .path
        .as_deref()
        .map(|p| Path::new(p).to_path_buf())
        .unwrap_or_else(|| project_root.clone());

    let files = find_typescript_files(&scan_path);
    if files.is_empty() {
        logger::info("No TypeScript files found.");
        return Ok(IndexResult {
            files_scanned: 0,
            functions_indexed: 0,
            added: 0,
            modified: 0,
            deleted: 0,
            model: String::new(),
            tier: String::new(),
            dimensions: 0,
        });
    }

    logger::info(&format!("Found {} TypeScript files.", files.len()));

    // Preflight Ollama
    let client = OllamaClient::new(None);
    let (embedder, msg) = client.preflight()?;
    logger::info(&msg);

    // Open database
    let db_path_str = db_path.to_string_lossy().to_string();
    let conn = open_database(&db_path_str)?;

    // Check model mismatch
    if let Some(stored_model) = get_meta_value(&conn, "model_name") {
        if stored_model != embedder.model_name() && !options.force {
            return Err(CodeuseError::Index(format!(
                "Model mismatch: index was built with '{}' but current model is '{}'. \
                 Use --force to re-index with the new model.",
                stored_model,
                embedder.model_name()
            )));
        }
    }

    // Compute changes
    let file_paths: Vec<String> = files.iter().map(|p| p.to_string_lossy().to_string()).collect();

    let changes = if options.force {
        crate::store::file_tracker::FileChanges {
            added: file_paths.clone(),
            modified: vec![],
            deleted: vec![],
            unchanged: vec![],
        }
    } else {
        compute_changed_files(&conn, &file_paths)?
    };

    logger::info(&format!(
        "Changes: {} added, {} modified, {} deleted, {} unchanged.",
        changes.added.len(),
        changes.modified.len(),
        changes.deleted.len(),
        changes.unchanged.len()
    ));

    // Delete functions for removed files
    for fp in &changes.deleted {
        delete_functions_for_file(&conn, fp)?;
        remove_tracked_file(&conn, fp)?;
    }

    // Delete functions for modified files
    for fp in &changes.modified {
        delete_functions_for_file(&conn, fp)?;
    }

    // Parse all added/modified files in parallel
    let files_to_process: Vec<&String> = changes
        .added
        .iter()
        .chain(changes.modified.iter())
        .collect();

    let parse_results: Vec<_> = files_to_process
        .par_iter()
        .map(|fp| {
            let path = Path::new(fp.as_str());
            match parse_file(path) {
                Ok(parsed) => {
                    for err in &parsed.parse_errors {
                        logger::warn(err);
                    }
                    Ok((fp.to_string(), parsed))
                }
                Err(e) => {
                    logger::warn(&format!("Failed to parse {fp}: {e}"));
                    Err(e)
                }
            }
        })
        .filter_map(|r| r.ok())
        .collect();

    // Upsert tracked files and chunks (sequential — SQLite is single-writer)
    let mut total_chunks = 0;
    for (fp, parsed) in &parse_results {
        let source = fs::read_to_string(fp)?;
        let hash = content_hash(&source);
        let meta = fs::metadata(fp)?;
        let mtime_ms = meta
            .modified()?
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        upsert_tracked_file(&conn, fp, &hash, mtime_ms)?;

        if !parsed.chunks.is_empty() {
            upsert_functions(&conn, &parsed.chunks)?;
            total_chunks += parsed.chunks.len();
        }
    }

    // Embed unembedded functions
    let unembedded = get_functions_without_embeddings(&conn)?;
    if !unembedded.is_empty() {
        logger::info(&format!("Embedding {} functions...", unembedded.len()));

        let texts: Vec<String> = unembedded.iter().map(|f| f.source_text.clone()).collect();
        let progress_cb = |done: usize, total: usize| {
            if let Some(ref cb) = options.on_progress {
                cb(&format!("Embedded {done}/{total}"));
            }
        };

        let embeddings = embedder.embed_batch(&texts, Some(&progress_cb))?;

        for (func, embedding) in unembedded.iter().zip(embeddings.iter()) {
            update_embedding(&conn, &func.id, embedding)?;
        }

        logger::success(&format!("Embedded {} functions.", unembedded.len()));
    }

    // Update metadata
    set_meta_value(&conn, "model_name", embedder.model_name())?;
    set_meta_value(&conn, "model_dimensions", &embedder.dimensions().to_string())?;
    set_meta_value(
        &conn,
        "last_indexed_at",
        &chrono::Utc::now().to_rfc3339(),
    )?;
    if get_meta_value(&conn, "created_at").is_none() {
        set_meta_value(&conn, "created_at", &chrono::Utc::now().to_rfc3339())?;
    }

    let result = IndexResult {
        files_scanned: files_to_process.len(),
        functions_indexed: total_chunks,
        added: changes.added.len(),
        modified: changes.modified.len(),
        deleted: changes.deleted.len(),
        model: embedder.model_name().to_string(),
        tier: embedder.tier().to_string(),
        dimensions: embedder.dimensions(),
    };

    logger::success(&format!(
        "Indexed {} functions from {} files.",
        result.functions_indexed, result.files_scanned
    ));

    Ok(result)
}
