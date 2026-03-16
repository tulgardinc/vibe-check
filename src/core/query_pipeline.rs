use crate::embedder::ollama_client::OllamaClient;
use crate::embedder::types::Embedder;
use crate::error::CodeuseError;
use crate::ignore::ignore_file::{apply_exclusions, load_ignore_file};
use crate::ignore::stale_detector::detect_stale_exclusions;
use crate::output::types::{Candidate, QueryFunction, QueryMeta, QueryResult};
use crate::parser::chunker::parse_source;
use crate::ranking::jaccard::{rerank_candidates, CandidateForRerank};
use crate::store::db::{get_meta_value, open_database};
use crate::store::index_store::{count_functions, query_knn};
use crate::util::config::find_project_root;
use std::path::Path;
use std::time::Instant;

pub struct QueryOptions {
    pub source: String,
    pub file_name: Option<String>,
    pub top_k: usize,
    pub threshold: f64,
    pub db_path: Option<String>,
    pub project_root: Option<String>,
    pub model: Option<String>,
    pub ollama_host: Option<String>,
}

pub fn run_query(options: QueryOptions) -> Result<QueryResult, CodeuseError> {
    let start = Instant::now();

    let file_name = options.file_name.as_deref().unwrap_or("input.ts");
    let parsed = parse_source(&options.source, file_name);

    if parsed.chunks.is_empty() {
        return Ok(QueryResult {
            query_functions: vec![],
            warnings: vec!["No functions found in input.".into()],
            meta: QueryMeta {
                model: String::new(),
                indexed_functions: 0,
                query_functions: 0,
                elapsed_ms: start.elapsed().as_millis(),
            },
        });
    }

    // Check DB exists
    let project_root = options
        .project_root
        .as_deref()
        .map(|p| Path::new(p).to_path_buf())
        .unwrap_or_else(|| find_project_root(Path::new(".")));

    let db_path = options
        .db_path
        .clone()
        .unwrap_or_else(|| project_root.join(".vibecheck.db").to_string_lossy().to_string());

    if !Path::new(&db_path).exists() {
        return Err(CodeuseError::Config(
            "No index found. Run `vibec index` first.".into(),
        ));
    }

    let conn = open_database(&db_path)?;

    // Preflight Ollama
    let client = OllamaClient::new(options.ollama_host.as_deref());
    let (embedder, _) = client.preflight(options.model.as_deref())?;

    let mut warnings = Vec::new();

    // Check model mismatch (warn, don't error)
    if let Some(stored_model) = get_meta_value(&conn, "model_name") {
        if stored_model != embedder.model_name() {
            warnings.push(format!(
                "Model mismatch: index was built with '{}' but current model is '{}'. \
                 Results may be inaccurate.",
                stored_model,
                embedder.model_name()
            ));
        }
    }

    // Load exclusions
    let ignore_file = load_ignore_file(&project_root);
    let stale = detect_stale_exclusions(&conn, &ignore_file);
    for w in &stale {
        warnings.push(w.reason.clone());
    }

    // Batch-embed all query chunks
    let texts: Vec<String> = parsed.chunks.iter().map(|c| c.source_text.clone()).collect();
    let embeddings = embedder.embed_batch(&texts, None)?;

    let indexed_count = count_functions(&conn)?;
    let mut query_functions = Vec::new();

    for (chunk, embedding) in parsed.chunks.iter().zip(embeddings.iter()) {
        // KNN with over-fetch
        let over_fetch = options.top_k * 3;
        let knn_results = query_knn(&conn, embedding, over_fetch, options.threshold)?;

        // Filter self-matches and related chunks (parent-child, sibling blocks)
        let candidates: Vec<CandidateForRerank> = knn_results
            .into_iter()
            .filter(|(func, _)| {
                if func.id == chunk.id {
                    return false;
                }
                // Skip parent-child and sibling matches within the same file
                if func.file_path == chunk.file_path {
                    let chunk_type_str = chunk.chunk_type.as_str();
                    // Query block matched its parent function
                    if chunk_type_str == "block"
                        && func.chunk_type == "function"
                        && chunk.context.as_deref() == Some(&func.function_name)
                    {
                        return false;
                    }
                    // Query function matched one of its own blocks
                    if chunk_type_str == "function"
                        && func.chunk_type == "block"
                        && func.context.as_deref() == Some(&chunk.function_name)
                    {
                        return false;
                    }
                    // Two blocks sharing the same parent
                    if chunk_type_str == "block"
                        && func.chunk_type == "block"
                        && chunk.context.is_some()
                        && chunk.context == func.context
                    {
                        return false;
                    }
                }
                true
            })
            .map(|(func, distance)| {
                let line_count = func.line_count();
                let signature = func.signature();
                CandidateForRerank {
                    name: func.function_name,
                    path: func.file_path,
                    line: func.start_line as usize,
                    line_count,
                    signature,
                    distance,
                    detection_method: "embedding".into(),
                    source: func.source_text,
                    signature_hash: func.signature_hash,
                    chunk_type: Some(func.chunk_type),
                    context: func.context,
                }
            })
            .collect();

        // Jaccard re-rank
        let ranked = rerank_candidates(&chunk.source_text, candidates, 0.7);

        // Map to output Candidates
        let mut output_candidates: Vec<Candidate> = ranked
            .into_iter()
            .map(|r| Candidate {
                name: r.name,
                path: r.path,
                line: r.line,
                line_count: r.line_count,
                signature: r.signature,
                distance: r.combined_score,
                detection_method: r.detection_method,
                source: r.source,
                signature_hash: r.signature_hash,
                chunk_type: r.chunk_type,
                context: r.context,
                jaccard_similarity: Some(r.jaccard_similarity),
            })
            .collect();

        // Apply exclusions
        output_candidates =
            apply_exclusions(&ignore_file, output_candidates, &chunk.signature_hash);

        // Slice to top-k
        output_candidates.truncate(options.top_k);

        query_functions.push(QueryFunction {
            name: chunk.function_name.clone(),
            file: chunk.file_path.clone(),
            line: chunk.start_line,
            line_count: chunk.line_count(),
            signature: chunk.signature(),
            candidates: output_candidates,
            chunk_type: Some(chunk.chunk_type.as_str().to_string()),
        });
    }

    Ok(QueryResult {
        query_functions,
        warnings,
        meta: QueryMeta {
            model: embedder.model_name().to_string(),
            indexed_functions: indexed_count,
            query_functions: parsed.chunks.len(),
            elapsed_ms: start.elapsed().as_millis(),
        },
    })
}
