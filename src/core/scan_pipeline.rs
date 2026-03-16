use crate::error::CodeuseError;
use crate::ignore::ignore_file::{is_excluded, load_ignore_file};
use crate::output::types::{ScanMatch, ScanMatchEntry, ScanMeta, ScanResult, similarity_tier};
use crate::ranking::jaccard::{jaccard_similarity, tokenize_code};
use crate::store::db::{get_meta_value, open_database};
use crate::store::index_store::{bytes_to_embedding, get_all_functions, query_knn};
use crate::util::config::find_project_root;
use crate::util::logger;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

pub struct ScanOptions {
    pub top_n: usize,
    pub threshold: f64,
    pub db_path: Option<String>,
    pub project_root: Option<String>,
    pub on_progress: Option<Box<dyn Fn(&str)>>,
}

pub fn run_scan(options: ScanOptions) -> Result<ScanResult, CodeuseError> {
    let start = Instant::now();

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
    let all_functions = get_all_functions(&conn)?;
    let ignore_file = load_ignore_file(&project_root);

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

    // Pre-tokenize all sources in parallel
    let token_cache: HashMap<String, HashSet<String>> = embedded
        .par_iter()
        .map(|f| (f.id.clone(), tokenize_code(&f.source_text)))
        .collect();

    // Find pairs
    let mut seen_pairs: HashSet<String> = HashSet::new();
    let mut all_matches: Vec<ScanMatch> = Vec::new();

    for func in &embedded {
        let embedding = bytes_to_embedding(func.embedding.as_ref().unwrap());
        let neighbors = query_knn(&conn, &embedding, 6, options.threshold)?;

        for (neighbor, distance) in &neighbors {
            // Skip self
            if neighbor.id == func.id {
                continue;
            }

            // Skip parent-child and sibling block matches within the same file
            if func.file_path == neighbor.file_path && is_related_chunk(func, neighbor) {
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

            // Skip excluded
            if is_excluded(&ignore_file, &func.signature_hash, &neighbor.signature_hash) {
                continue;
            }

            // Compute Jaccard
            let tokens_a = token_cache.get(&func.id);
            let tokens_b = token_cache.get(&neighbor.id);

            let jaccard = match (tokens_a, tokens_b) {
                (Some(a), Some(b)) => jaccard_similarity(a, b),
                _ => 0.0,
            };

            let combined = 0.7 * distance + 0.3 * (1.0 - jaccard);

            // Order alphabetically by id and build entries
            let (entry_a, entry_b) = if func.id < neighbor.id {
                (to_scan_entry(func), to_scan_entry(neighbor))
            } else {
                (to_scan_entry(neighbor), to_scan_entry(func))
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

    // Sort by distance (most similar first) and cap
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

/// Returns true if two chunks in the same file are related (parent-child or siblings
/// within the same function), which makes them noise rather than real duplicates.
fn is_related_chunk(
    a: &crate::store::types::StoredFunction,
    b: &crate::store::types::StoredFunction,
) -> bool {
    // Block is a child of the function
    if a.chunk_type == "block" && b.chunk_type == "function" {
        if a.context.as_deref() == Some(&b.function_name) {
            return true;
        }
    }
    if b.chunk_type == "block" && a.chunk_type == "function" {
        if b.context.as_deref() == Some(&a.function_name) {
            return true;
        }
    }
    // Two blocks that share the same parent function
    if a.chunk_type == "block" && b.chunk_type == "block" {
        if a.context.is_some() && a.context == b.context {
            return true;
        }
    }
    false
}

fn to_scan_entry(f: &crate::store::types::StoredFunction) -> ScanMatchEntry {
    ScanMatchEntry {
        name: f.function_name.clone(),
        path: f.file_path.clone(),
        line: f.start_line as usize,
        signature_hash: f.signature_hash.clone(),
        chunk_type: Some(f.chunk_type.clone()),
        context: f.context.clone(),
    }
}
