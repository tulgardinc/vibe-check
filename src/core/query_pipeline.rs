use crate::embedder::types::{resolve_embedder, Embedder, OllamaConfig};
use crate::error::VibecheckError;
use crate::ignore::ignore_file::{apply_exclusions, load_ignore_file};
use crate::ignore::stale_detector::detect_stale_exclusions;
use crate::output::types::{Candidate, QueryFunction, QueryMeta, QueryResult};
use crate::parser::chunker::parse_source;
use crate::ranking::jaccard::{rerank_candidates, CandidateForRerank, DEFAULT_RERANK_ALPHA};
use crate::store::db::{get_meta_value, open_database};
use crate::store::index_store::{count_functions, query_knn};
use crate::store::types::should_filter_neighbor;
use crate::util::config::{resolve_existing_db, resolve_project_root};
use std::time::Instant;

/// Over-fetch multiplier for KNN before filtering and re-ranking.
const KNN_OVER_FETCH_MULTIPLIER: usize = 3;

pub struct QueryOptions {
    pub source: String,
    pub file_name: Option<String>,
    pub top_k: usize,
    pub threshold: f64,
    pub db_path: Option<String>,
    pub project_root: Option<String>,
    pub ollama: OllamaConfig,
    /// If provided, skip Ollama preflight and use this embedder directly (for testing).
    pub embedder: Option<Box<dyn Embedder>>,
}

pub fn run_query(options: QueryOptions) -> Result<QueryResult, VibecheckError> {
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

    let project_root = resolve_project_root(options.project_root.as_deref());

    let db_path = resolve_existing_db(&project_root, options.db_path.as_deref())?;
    let conn = open_database(&db_path)?;

    let (embedder_box, _) = resolve_embedder(
        options.embedder,
        &options.ollama,
    )?;
    let embedder: &dyn Embedder = embedder_box.as_ref();

    let mut warnings = Vec::new();

    // Check model mismatch (warn, don't error)
    if let Some(stored_model) = get_meta_value(&conn, "model_name")
        && stored_model != embedder.model_name()
    {
        warnings.push(format!(
            "Model mismatch: index was built with '{}' but current model is '{}'. \
             Results may be inaccurate.",
            stored_model,
            embedder.model_name()
        ));
    }

    // Load exclusions
    let ignore_file = load_ignore_file(&project_root);
    let stale = detect_stale_exclusions(&conn, &ignore_file);
    for w in &stale {
        warnings.push(w.reason.clone());
    }

    // Embed each query chunk with the search_query prefix for asymmetric retrieval
    let embeddings: Vec<Vec<f32>> = parsed.chunks
        .iter()
        .map(|c| embedder.embed_query(&c.source_text))
        .collect::<Result<Vec<_>, _>>()?;

    let indexed_count = count_functions(&conn)?;
    let mut query_functions = Vec::new();

    for (chunk, embedding) in parsed.chunks.iter().zip(embeddings.iter()) {
        // KNN with over-fetch
        let over_fetch = options.top_k * KNN_OVER_FETCH_MULTIPLIER;
        let knn_results = query_knn(&conn, embedding, over_fetch, options.threshold)?;

        // Filter self-matches and related chunks (parent-child, sibling blocks)
        let candidates: Vec<CandidateForRerank> = knn_results
            .into_iter()
            .filter(|(func, _)| {
                !should_filter_neighbor(
                    &chunk.id,
                    &chunk.file_path,
                    chunk.chunk_type,
                    chunk.context.as_deref(),
                    &chunk.function_name,
                    func,
                )
            })
            .map(|(func, distance)| {
                let line_count = func.line_count();
                CandidateForRerank {
                    name: func.function_name,
                    path: func.file_path,
                    line: func.start_line as usize,
                    line_count,
                    signature: func.signature,
                    distance,
                    detection_method: "embedding".into(),
                    source: func.source_text,
                    signature_hash: func.signature_hash,
                    chunk_type: Some(func.chunk_type.as_str().to_string()),
                    context: func.context,
                    tokens: func.tokens,
                }
            })
            .collect();

        // Jaccard re-rank
        let ranked = rerank_candidates(&chunk.tokens, candidates, DEFAULT_RERANK_ALPHA);

        // Map to output Candidates
        let mut output_candidates: Vec<Candidate> = ranked
            .into_iter()
            .map(|r| Candidate {
                name: r.candidate.name,
                path: r.candidate.path,
                line: r.candidate.line,
                line_count: r.candidate.line_count,
                signature: r.candidate.signature,
                distance: r.combined_score,
                detection_method: r.candidate.detection_method,
                source: r.candidate.source,
                signature_hash: r.candidate.signature_hash,
                chunk_type: r.candidate.chunk_type,
                context: r.candidate.context,
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
            signature: chunk.signature.clone(),
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
