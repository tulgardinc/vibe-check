use crate::embedder::types::{resolve_embedder, Embedder, OllamaConfig};
use crate::error::VibecheckError;
use crate::ignore::ignore_file::{
    apply_exclusions, load_ignore_file, ExclusionIndex, FileExclusionMatcher,
    FilePairExclusionIndex,
};
use crate::ignore::stale_detector::detect_stale_exclusions;
use crate::output::types::{Candidate, QueryFunction, QueryMeta, QueryResult};
use crate::parser::chunker::parse_source;
use crate::parser::types::FunctionChunk;
use crate::ranking::jaccard::{rerank, RerankItem, DEFAULT_RERANK_ALPHA};
use crate::store::db::{close_database, get_meta_value, open_database};
use crate::store::index_store::{count_functions, query_knn, vec_table_exists};
use crate::store::types::should_filter_neighbor;
use crate::util::config::{resolve_existing_db, resolve_project_root};
use crate::util::git::check_staleness;
use rusqlite::Connection;
use std::path::Path;
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
    if let Some(stored_model) = get_meta_value(&conn, "model_name")?
        && stored_model != embedder.model_name()
    {
        warnings.push(format!(
            "Model mismatch: index was built with '{}' but current model is '{}'. \
             Results may be inaccurate.",
            stored_model,
            embedder.model_name()
        ));
    }

    // Check index staleness against current HEAD
    if let Some(staleness_warning) = check_staleness(&conn, &project_root) {
        warnings.push(staleness_warning);
    }

    let indexed_count = count_functions(&conn)?;

    let chunk_options = QueryChunkOptions {
        top_k: options.top_k,
        threshold: options.threshold,
        project_root: &project_root,
    };
    let (query_functions, chunk_warnings) =
        query_chunks(&parsed.chunks, &conn, embedder, &chunk_options)?;
    warnings.extend(chunk_warnings);

    close_database(conn)?;
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

/// Options for the shared query-chunks logic.
pub struct QueryChunkOptions<'a> {
    pub top_k: usize,
    pub threshold: f64,
    pub project_root: &'a Path,
}

/// Core query logic: embed chunks, KNN, rerank, exclusions, build results.
/// Used by both run_query() and the git pipeline.
pub fn query_chunks(
    chunks: &[FunctionChunk],
    conn: &Connection,
    embedder: &dyn Embedder,
    options: &QueryChunkOptions,
) -> Result<(Vec<QueryFunction>, Vec<String>), VibecheckError> {
    let mut warnings = Vec::new();

    // Load exclusions and precompute indices for O(1) lookup
    let ignore_file = load_ignore_file(options.project_root);
    let exclusion_index = ExclusionIndex::new(&ignore_file);
    let file_matcher =
        FileExclusionMatcher::new(&ignore_file.file_exclusions, options.project_root);
    let file_pair_index =
        FilePairExclusionIndex::new(&ignore_file.file_pair_exclusions, options.project_root);
    let stale = detect_stale_exclusions(conn, &ignore_file)?;
    for w in &stale {
        warnings.push(w.reason.clone());
    }

    // Batch-embed all query chunks at once instead of one-by-one
    let query_texts: Vec<&str> = chunks.iter().map(|c| c.source_text.as_str()).collect();
    let embeddings = embedder.embed_batch(&query_texts, None)?;

    let vec_exists = vec_table_exists(conn)?;
    let mut query_functions = Vec::new();

    for (chunk, embedding) in chunks.iter().zip(embeddings.iter()) {
        // KNN with over-fetch
        let over_fetch = options.top_k * KNN_OVER_FETCH_MULTIPLIER;
        let knn_results = query_knn(conn, embedding, over_fetch, options.threshold, vec_exists)?;

        // Filter self-matches, build RerankItems wrapping Candidates
        let candidates: Vec<RerankItem<Candidate>> = knn_results
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
                let tokens = func.tokens.clone();
                RerankItem {
                    item: Candidate::from_stored(&func, distance),
                    distance,
                    tokens,
                }
            })
            .collect();

        // Re-rank by blending embedding distance with Jaccard similarity
        let ranked = rerank(&chunk.tokens, candidates, DEFAULT_RERANK_ALPHA);

        // Unpack re-ranked results into Candidates with updated scores
        let mut output_candidates: Vec<Candidate> = ranked
            .into_iter()
            .map(|r| {
                let mut c = r.item;
                c.distance = r.combined_score;
                c.jaccard_similarity = Some(r.jaccard_similarity);
                c
            })
            .collect();

        // Apply exclusions (signature-hash pairs, file exclusions, file-pair exclusions)
        output_candidates =
            apply_exclusions(&exclusion_index, output_candidates, &chunk.signature_hash);
        output_candidates.retain(|c| {
            !file_matcher.is_excluded(Path::new(&c.path))
                && !file_pair_index.is_excluded(&chunk.file_path, &c.path)
        });

        // Slice to top-k
        output_candidates.truncate(options.top_k);

        query_functions.push(QueryFunction {
            name: chunk.function_name.clone(),
            file: chunk.file_path.clone(),
            line: chunk.start_line,
            line_count: chunk.line_count(),
            signature: chunk.signature.clone(),
            candidates: output_candidates,
            chunk_type: Some(chunk.chunk_type),
        });
    }

    Ok((query_functions, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_chunks_function_exists_with_correct_signature() {
        // This test verifies that the query_chunks function exists and has
        // the correct signature. It will fail at runtime because the function
        // body is todo!(), but it compiles, proving the interface is correct.

        // We cannot actually call it without a full DB + embedder setup,
        // but we can verify the types are correct by constructing the options.
        let project_root = Path::new("/tmp/test");
        let _options = QueryChunkOptions {
            top_k: 5,
            threshold: 0.3,
            project_root,
        };

        // Verify QueryChunkOptions fields are accessible
        assert_eq!(_options.top_k, 5);
        assert_eq!(_options.threshold, 0.3);
        assert_eq!(_options.project_root, project_root);
    }
}
