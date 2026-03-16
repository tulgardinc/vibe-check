use crate::error::VibecheckError;
use crate::ignore::ignore_file::{is_excluded, load_ignore_file};
use crate::output::types::{ScanMatch, ScanMatchEntry, ScanMeta, ScanResult, similarity_tier};
use crate::ranking::jaccard::{combined_score, jaccard_similarity, DEFAULT_RERANK_ALPHA};
use crate::store::db::{get_meta_value, open_database};
use crate::store::index_store::{get_all_functions, query_knn_raw};
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
    let all_functions = get_all_functions(&conn)?;
    let ignore_file = load_ignore_file(&project_root);

    if let Some(model) = get_meta_value(&conn, "model_name") {
        logger::verbose(&format!("Using index built with model: {model}"));
    }

    // Filter to embedded functions only
    let embedded: Vec<_> = all_functions
        .iter()
        .filter(|f| f.embedding.is_some())
        .collect();

    if embedded.is_empty() {
        return Ok(ScanResult {
            matches: vec![],
            meta: ScanMeta {
                model: get_meta_value(&conn, "model_name").unwrap_or_default(),
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

    // Find pairs
    let mut seen_pairs: HashSet<String> = HashSet::new();
    let mut all_matches: Vec<ScanMatch> = Vec::new();

    for func in &embedded {
        let embedding_bytes = func.embedding.as_ref().unwrap();
        let neighbors = query_knn_raw(&conn, embedding_bytes, SCAN_KNN_NEIGHBORS, options.threshold)?;

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

            // Deduplicate unordered pairs
            let pair_key = if func.id < neighbor.id {
                format!("{}||{}", func.id, neighbor.id)
            } else {
                format!("{}||{}", neighbor.id, func.id)
            };

            if seen_pairs.contains(&pair_key) {
                continue;
            }
            seen_pairs.insert(pair_key);

            if is_excluded(&ignore_file, &func.signature_hash, &neighbor.signature_hash) {
                continue;
            }

            let jaccard = jaccard_similarity(&func.tokens, &neighbor.tokens);
            let combined = combined_score(*distance, jaccard, DEFAULT_RERANK_ALPHA);

            let (entry_a, entry_b) = if func.id < neighbor.id {
                (ScanMatchEntry::from(*func), ScanMatchEntry::from(neighbor))
            } else {
                (ScanMatchEntry::from(neighbor), ScanMatchEntry::from(*func))
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
            model: get_meta_value(&conn, "model_name").unwrap_or_default(),
            chunks_scanned: embedded.len(),
            pairs_found,
            elapsed_ms: start.elapsed().as_millis(),
        },
    })
}
