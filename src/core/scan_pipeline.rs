use crate::error::VibecheckError;
use crate::ignore::ignore_file::{
    load_ignore_file, ExclusionIndex, FileExclusionMatcher, FilePairExclusionIndex,
};
use crate::output::types::{ScanMatch, ScanMatchEntry, ScanMeta, ScanResult, similarity_tier};
use crate::ranking::jaccard::{combined_score, jaccard_similarity, DEFAULT_RERANK_ALPHA};
use crate::store::db::{get_meta_value, open_database};
use crate::store::index_store::{get_embedded_functions_slim, query_knn_raw, vec_table_exists};
use crate::store::types::should_filter_neighbor;
use crate::util::config::{resolve_existing_db, resolve_project_root};
use crate::util::logger;
use std::collections::HashSet;
use std::time::Instant;

/// Number of nearest neighbors to fetch per function during scan.
const SCAN_KNN_NEIGHBORS: usize = 6;

pub type ProgressCallback = Box<dyn Fn(&str)>;

pub struct ScanOptions {
    pub top_n: usize,
    pub threshold: f64,
    pub db_path: Option<String>,
    pub project_root: Option<String>,
    pub on_progress: Option<ProgressCallback>,
}

pub fn run_scan(options: ScanOptions) -> Result<ScanResult, VibecheckError> {
    let start = Instant::now();

    let project_root = resolve_project_root(options.project_root.as_deref());

    let db_path = resolve_existing_db(&project_root, options.db_path.as_deref())?;
    let conn = open_database(&db_path)?;
    let mut embedded = get_embedded_functions_slim(&conn)?;
    let ignore_file = load_ignore_file(&project_root);
    let exclusion_index = ExclusionIndex::new(&ignore_file);
    let file_matcher = FileExclusionMatcher::new(&ignore_file.file_exclusions, &project_root);
    let file_pair_index =
        FilePairExclusionIndex::new(&ignore_file.file_pair_exclusions, &project_root);

    // Pre-filter: remove functions from file-excluded paths
    if !file_matcher.is_empty() {
        embedded.retain(|f| !file_matcher.is_excluded(std::path::Path::new(&f.file_path)));
    }

    if let Some(model) = get_meta_value(&conn, "model_name")? {
        logger::verbose(&format!("Using index built with model: {model}"));
    }

    if embedded.is_empty() {
        return Ok(ScanResult {
            matches: vec![],
            meta: ScanMeta {
                model: get_meta_value(&conn, "model_name")?.unwrap_or_default(),
                chunks_scanned: 0,
                pairs_found: 0,
                elapsed_ms: start.elapsed().as_millis(),
            },
        });
    }

    logger::info(&format!(
        "Scanning {} embedded functions...",
        embedded.len()
    ));

    let vec_exists = vec_table_exists(&conn)?;

    // Find pairs — use tuple keys to avoid String allocation for pair dedup
    let mut seen_pairs: HashSet<(usize, usize)> = HashSet::new();
    let mut all_matches: Vec<ScanMatch> = Vec::new();

    // Build an index from function ID → position for dedup without string alloc
    let id_to_idx: std::collections::HashMap<&str, usize> = embedded
        .iter()
        .enumerate()
        .map(|(i, f)| (f.id.as_str(), i))
        .collect();

    for (idx, func) in embedded.iter().enumerate() {
        let embedding_bytes = func.embedding.as_ref().unwrap();
        let neighbors = query_knn_raw(&conn, embedding_bytes, SCAN_KNN_NEIGHBORS, options.threshold, vec_exists)?;

        for (neighbor, distance) in &neighbors {
            if should_filter_neighbor(
                &func.id,
                &func.file_path,
                func.chunk_type,
                func.context.as_deref(),
                &func.function_name,
                neighbor,
            ) {
                continue;
            }

            // Deduplicate unordered pairs using indices
            let neighbor_idx = id_to_idx.get(neighbor.id.as_str()).copied().unwrap_or(usize::MAX);
            let pair_key = if idx < neighbor_idx {
                (idx, neighbor_idx)
            } else {
                (neighbor_idx, idx)
            };

            if !seen_pairs.insert(pair_key) {
                continue;
            }

            if exclusion_index.is_excluded(&func.signature_hash, &neighbor.signature_hash) {
                continue;
            }

            if file_pair_index.is_excluded(&func.file_path, &neighbor.file_path) {
                continue;
            }

            if file_matcher.is_excluded(std::path::Path::new(&neighbor.file_path)) {
                continue;
            }

            let jaccard = jaccard_similarity(&func.tokens, &neighbor.tokens);
            let combined = combined_score(*distance, jaccard, DEFAULT_RERANK_ALPHA);

            let (entry_a, entry_b) = if func.id < neighbor.id {
                (ScanMatchEntry::from(func), ScanMatchEntry::from(neighbor))
            } else {
                (ScanMatchEntry::from(neighbor), ScanMatchEntry::from(func))
            };

            all_matches.push(ScanMatch {
                a: entry_a,
                b: entry_b,
                distance: combined,
                similarity: similarity_tier(combined).to_string(),
                jaccard_similarity: Some(jaccard),
            });
        }
    }

    all_matches.sort_by(|a, b| {
        a.distance
            .partial_cmp(&b.distance)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    all_matches.truncate(options.top_n);

    let pairs_found = all_matches.len();

    Ok(ScanResult {
        matches: all_matches,
        meta: ScanMeta {
            model: get_meta_value(&conn, "model_name")?.unwrap_or_default(),
            chunks_scanned: embedded.len(),
            pairs_found,
            elapsed_ms: start.elapsed().as_millis(),
        },
    })
}
