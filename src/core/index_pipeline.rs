use crate::embedder::types::{resolve_embedder, Embedder, OllamaConfig, DEFAULT_MAX_INPUT_BYTES};
use crate::error::VibecheckError;
use crate::output::types::IndexResult;
use crate::parser::chunker::{parse_file, parse_source};
use crate::store::cache;
use crate::store::db::{close_database, get_meta_value, open_database, set_meta_value};
use crate::store::file_tracker::{
    compute_changed_files, remove_tracked_file, upsert_tracked_file,
};
use crate::store::index_store::{
    bytes_to_embedding, delete_functions_for_file, get_functions_without_embeddings,
    update_embedding, upsert_functions, vec_table_exists,
};
use crate::ignore::ignore_file::{load_ignore_file, FileExclusionMatcher};
use crate::util::config::{find_project_root, find_source_files, resolve_cache_path, resolve_db_path};
use crate::util::git::{get_head_commit, is_git_repo};
use crate::util::hash::sha256;
use crate::util::logger;
use rayon::prelude::*;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

/// Number of functions to embed per batch during indexing.
const INDEX_EMBED_BATCH_SIZE: usize = 32;

/// Reports progress during the indexing pipeline.
pub trait IndexProgress: Send {
    /// Called before embedding starts. `total_batches` is the number of batches to embed.
    fn on_start(&self, _total_batches: usize) {}
    /// Called after each batch completes.
    fn on_progress(&self) {}
    /// Called when all embedding is finished.
    fn on_done(&self) {}
}

pub struct IndexOptions {
    pub path: Option<String>,
    pub db_path: Option<String>,
    pub force: bool,
    pub ollama: OllamaConfig,
    pub progress: Option<Box<dyn IndexProgress>>,
    /// Set to true to cancel the embedding loop
    pub cancel: Option<Arc<AtomicBool>>,
    /// If provided, skip Ollama preflight and use this embedder directly (for testing).
    pub embedder: Option<Box<dyn Embedder>>,
}

pub fn run_index(options: IndexOptions) -> Result<IndexResult, VibecheckError> {
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

    let ignore_file = load_ignore_file(&project_root);
    let file_matcher = FileExclusionMatcher::new(&ignore_file.file_exclusions, &project_root);

    let mut files = find_source_files(&scan_path);
    if !file_matcher.is_empty() {
        let before = files.len();
        files.retain(|p| !file_matcher.is_excluded(p));
        let excluded = before - files.len();
        if excluded > 0 {
            logger::info(&format!("Excluded {excluded} files via file exclusions."));
        }
    }
    if files.is_empty() {
        logger::info("No source files found.");
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

    logger::info(&format!("Found {} source files.", files.len()));

    let (embedder_box, _) = resolve_embedder(
        options.embedder,
        &options.ollama,
    )?;
    let embedder: &dyn Embedder = embedder_box.as_ref();

    // Open database
    let db_path_str = db_path.to_string_lossy().to_string();
    let mut conn = open_database(&db_path_str)?;

    // Check model mismatch
    if let Some(stored_model) = get_meta_value(&conn, "model_name")?
        && stored_model != embedder.model_name()
    {
        if !options.force {
            return Err(VibecheckError::Index(format!(
                "Model mismatch: index was built with '{}' but current model is '{}'. \
                 Use --force to re-index with the new model.",
                stored_model,
                embedder.model_name()
            )));
        }
        // Force re-index: drop vec table so it gets recreated with the new dimensions
        crate::store::db::drop_vec_table(&conn)?;
    }

    // Check signature hash version
    let sig_hash_version = get_meta_value(&conn, "signature_hash_version")?;
    if sig_hash_version.as_deref() != Some("2") && sig_hash_version.is_some() && !options.force {
        logger::warn(
            "Index uses legacy 8-char signature hashes. Run with --force to upgrade to 16-char hashes.",
        );
    }

    // Compute changes
    let file_paths: Vec<String> = files.iter().map(|p| p.to_string_lossy().to_string()).collect();

    let changes = if options.force {
        crate::store::file_tracker::FileChanges {
            added: file_paths.clone(),
            modified: vec![],
            deleted: vec![],
            unchanged: vec![],
            cached_content: Default::default(),
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

    // Parse all added/modified files in parallel (no DB access).
    // Use cached content from change detection to avoid re-reading modified files.
    let files_to_process: Vec<&String> = changes
        .added
        .iter()
        .chain(changes.modified.iter())
        .collect();

    let parse_results: Vec<_> = files_to_process
        .par_iter()
        .map(|fp| {
            let result = if let Some(cached) = changes.cached_content.get(*fp) {
                Ok(parse_source(cached, fp))
            } else {
                parse_file(Path::new(fp.as_str()))
            };
            match result {
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

    // Atomic transaction: delete stale data + insert new data.
    // If anything fails, the DB rolls back to its pre-index state.
    let tx = conn.transaction()?;
    let vec_exists = vec_table_exists(&tx)?;

    for fp in &changes.deleted {
        delete_functions_for_file(&tx, fp, vec_exists)?;
        remove_tracked_file(&tx, fp)?;
    }

    for fp in &changes.modified {
        delete_functions_for_file(&tx, fp, vec_exists)?;
    }

    let mut total_chunks = 0;
    for (fp, parsed) in &parse_results {
        let hash = sha256(&parsed.source);
        let meta = fs::metadata(fp)?;
        let mtime_ms = meta
            .modified()?
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        upsert_tracked_file(&tx, fp, &hash, mtime_ms)?;

        if !parsed.chunks.is_empty() {
            upsert_functions(&tx, &parsed.chunks)?;
            total_chunks += parsed.chunks.len();
        }
    }

    tx.commit()?;

    // Embed unembedded functions in batches
    let unembedded = get_functions_without_embeddings(&conn)?;
    if !unembedded.is_empty() {
        let vec_exists = vec_table_exists(&conn)?;

        // Try to open embedding cache if in a git repo (graceful fallback)
        let cache_conn = if is_git_repo(&project_root) {
            match resolve_cache_path(&project_root) {
                Some(cache_path) => {
                    let max_input_bytes = options.ollama.max_input_bytes.unwrap_or(DEFAULT_MAX_INPUT_BYTES);
                    match cache::open_cache(
                        &cache_path,
                        embedder.model_name(),
                        embedder.dimensions(),
                        max_input_bytes,
                        options.force,
                    ) {
                        Ok(c) => {
                            logger::info("Embedding cache opened.");
                            Some(c)
                        }
                        Err(e) => {
                            logger::warn(&format!("Could not open embedding cache: {e}"));
                            None
                        }
                    }
                }
                None => None,
            }
        } else {
            None
        };

        // Batch-lookup cached embeddings by content_hash
        let cached_embeddings = if let Some(ref cc) = cache_conn {
            let content_hashes: Vec<&str> =
                unembedded.iter().map(|f| f.content_hash.as_str()).collect();
            match cache::lookup_embeddings(cc, &content_hashes) {
                Ok(hits) => hits,
                Err(e) => {
                    logger::warn(&format!("Cache lookup failed: {e}"));
                    std::collections::HashMap::new()
                }
            }
        } else {
            std::collections::HashMap::new()
        };

        // Separate cache hits from misses
        let mut cache_hit_funcs = Vec::new();
        let mut cache_miss_funcs = Vec::new();
        for func in &unembedded {
            if let Some(embedding_bytes) = cached_embeddings.get(&func.content_hash) {
                cache_hit_funcs.push((func, embedding_bytes.clone()));
            } else {
                cache_miss_funcs.push(func);
            }
        }

        if !cache_hit_funcs.is_empty() {
            logger::info(&format!(
                "Cache hit for {} of {} functions.",
                cache_hit_funcs.len(),
                unembedded.len()
            ));
        }

        // Apply cache hits: write cached embeddings directly to the index DB
        if !cache_hit_funcs.is_empty() {
            let tx = conn.transaction()?;
            for (func, embedding_bytes) in &cache_hit_funcs {
                let embedding = bytes_to_embedding(embedding_bytes);
                update_embedding(&tx, &func.id, &embedding, vec_exists)?;
            }
            tx.commit()?;
        }

        // Embed cache misses via Ollama in batches
        let total_misses = cache_miss_funcs.len();
        let total_batches = (total_misses + INDEX_EMBED_BATCH_SIZE - 1) / INDEX_EMBED_BATCH_SIZE;

        if let Some(ref progress) = options.progress {
            progress.on_start(total_batches);
        }

        let mut cancelled = false;
        for batch in cache_miss_funcs.chunks(INDEX_EMBED_BATCH_SIZE) {
            if let Some(ref cancel) = options.cancel
                && cancel.load(Ordering::Relaxed)
            {
                logger::info("Indexing cancelled. Progress has been saved.");
                cancelled = true;
                break;
            }

            let texts: Vec<&str> = batch.iter().map(|f| f.source_text.as_str()).collect();
            let embeddings = embedder.embed_batch(&texts, None)?;

            let tx = conn.transaction()?;
            for (func, embedding) in batch.iter().zip(embeddings.iter()) {
                update_embedding(&tx, &func.id, embedding, vec_exists)?;

                // Write to cache for future use
                if let Some(ref cc) = cache_conn {
                    let embedding_bytes: Vec<u8> = embedding
                        .iter()
                        .flat_map(|f| f.to_le_bytes())
                        .collect();
                    if let Err(e) = cache::insert_embedding(cc, &func.content_hash, &embedding_bytes) {
                        logger::warn(&format!("Failed to write to cache: {e}"));
                    }
                }
            }
            tx.commit()?;

            if let Some(ref progress) = options.progress {
                progress.on_progress();
            }
        }

        if let Some(ref progress) = options.progress {
            progress.on_done();
        }

        let total = unembedded.len();
        if !cancelled {
            logger::success(&format!("Embedded {total} functions."));
        }

        // Close cache connection
        if let Some(cc) = cache_conn {
            if let Err(e) = cache::close_cache(cc) {
                logger::warn(&format!("Failed to close embedding cache: {e}"));
            }
        }
    } else if let Some(ref progress) = options.progress {
        // Clear the indexing spinner when there's nothing to embed
        progress.on_done();
    }

    // Ensure vec0 virtual table exists for indexed KNN queries
    crate::store::db::ensure_vec_table(&conn, embedder.dimensions())?;

    // Update metadata atomically
    let tx = conn.transaction()?;
    set_meta_value(&tx, "model_name", embedder.model_name())?;
    set_meta_value(&tx, "model_dimensions", &embedder.dimensions().to_string())?;
    set_meta_value(&tx, "signature_hash_version", "2")?;
    set_meta_value(
        &tx,
        "last_indexed_at",
        &chrono::Utc::now().to_rfc3339(),
    )?;
    if get_meta_value(&tx, "created_at")?.is_none() {
        set_meta_value(&tx, "created_at", &chrono::Utc::now().to_rfc3339())?;
    }
    // Store the current HEAD commit hash so query/scan can detect staleness
    if is_git_repo(&project_root) {
        if let Ok(head) = get_head_commit(&project_root) {
            set_meta_value(&tx, "head_commit", &head)?;
        }
    }
    tx.commit()?;

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

    close_database(conn)?;
    Ok(result)
}
