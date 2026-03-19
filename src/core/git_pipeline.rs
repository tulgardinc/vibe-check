use crate::core::query_pipeline::{query_chunks, QueryChunkOptions};
use crate::embedder::types::{resolve_embedder, Embedder, OllamaConfig};
use crate::error::VibecheckError;
use crate::output::types::{QueryMeta, QueryResult};
use crate::parser::chunker::parse_source;
use crate::parser::registry;
use crate::parser::types::FunctionChunk;
use crate::store::db::{close_database, get_meta_value, open_database};
use crate::store::index_store::count_functions;
use crate::util::config::{resolve_existing_db, resolve_project_root};
use crate::util::git::{
    check_staleness, diff_commit, diff_working_tree, is_git_repo, read_working_tree_file,
    show_file, DiffEntry, DiffStatus,
};
use crate::util::hash::sha256;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

pub struct GitQueryOptions {
    pub top_k: usize,
    pub threshold: f64,
    pub db_path: Option<String>,
    pub project_root: Option<String>,
    pub ollama: OllamaConfig,
    pub embedder: Option<Box<dyn Embedder>>,
}

pub struct CommitQueryOptions {
    pub hash: String,
    pub top_k: usize,
    pub threshold: f64,
    pub db_path: Option<String>,
    pub project_root: Option<String>,
    pub ollama: OllamaConfig,
    pub embedder: Option<Box<dyn Embedder>>,
}

/// Query uncommitted changes for duplicates.
/// Returns the same QueryResult as vibec query.
pub fn run_git_query(options: GitQueryOptions) -> Result<QueryResult, VibecheckError> {
    let project_root = resolve_project_root(options.project_root.as_deref());

    // Get diff entries from working tree
    let diff_entries = diff_working_tree(&project_root)?;

    run_diff_query(
        DiffQueryParams {
            top_k: options.top_k,
            threshold: options.threshold,
            db_path: options.db_path,
            ollama: options.ollama,
            embedder: options.embedder,
        },
        &project_root,
        diff_entries,
        |path| read_working_tree_file(&project_root, path),
        |entry, path| {
            if entry.status == DiffStatus::Added {
                Ok(None)
            } else {
                show_file(&project_root, "HEAD", path)
            }
        },
    )
}

/// Query a specific commit's changes for duplicates against the current index.
/// Returns the same QueryResult as vibec query.
pub fn run_commit_query(options: CommitQueryOptions) -> Result<QueryResult, VibecheckError> {
    let project_root = resolve_project_root(options.project_root.as_deref());
    let hash = options.hash.clone();
    let parent_rev = format!("{}^1", hash);

    // Get diff entries for the commit
    let diff_entries = diff_commit(&project_root, &hash)?;

    run_diff_query(
        DiffQueryParams {
            top_k: options.top_k,
            threshold: options.threshold,
            db_path: options.db_path,
            ollama: options.ollama,
            embedder: options.embedder,
        },
        &project_root,
        diff_entries,
        |path| {
            show_file(&project_root, &hash, path)?
                .ok_or_else(|| VibecheckError::Git(format!(
                    "file '{}' not found at revision {}", path, hash
                )))
        },
        |entry, path| {
            if entry.status == DiffStatus::Added {
                Ok(None)
            } else {
                show_file(&project_root, &parent_rev, path)
            }
        },
    )
}

/// Shared parameters for diff-based queries.
struct DiffQueryParams {
    top_k: usize,
    threshold: f64,
    db_path: Option<String>,
    ollama: OllamaConfig,
    embedder: Option<Box<dyn Embedder>>,
}

/// Shared implementation for both `run_git_query` and `run_commit_query`.
///
/// Parameterized by:
/// - `diff_entries`: the list of changed files
/// - `read_new`: how to read the "new" version of a file (returns the source string)
/// - `read_old`: how to read the "old" version of a file (returns `None` if it didn't exist)
fn run_diff_query<FNew, FOld>(
    params: DiffQueryParams,
    project_root: &std::path::Path,
    diff_entries: Vec<DiffEntry>,
    read_new: FNew,
    read_old: FOld,
) -> Result<QueryResult, VibecheckError>
where
    FNew: Fn(&str) -> Result<String, VibecheckError>,
    FOld: Fn(&DiffEntry, &str) -> Result<Option<String>, VibecheckError>,
{
    let start = Instant::now();

    // Validate git repo
    if !is_git_repo(project_root) {
        return Err(VibecheckError::Git(
            "Not a git repository. These commands require git.".into(),
        ));
    }

    // Open DB and resolve embedder
    let db_path = resolve_existing_db(project_root, params.db_path.as_deref())?;
    let conn = open_database(&db_path)?;
    let (embedder_box, _) = resolve_embedder(params.embedder, &params.ollama)?;
    let embedder: &dyn Embedder = embedder_box.as_ref();

    let mut warnings = Vec::new();

    // Check model mismatch
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

    // Check index staleness
    if let Some(staleness_warning) = check_staleness(&conn, project_root) {
        warnings.push(staleness_warning);
    }

    // Filter to supported file extensions
    let diff_entries: Vec<_> = diff_entries
        .into_iter()
        .filter(|e| registry::language_for_file(&e.path).is_some())
        .collect();

    let mut all_query_chunks = Vec::new();
    let mut all_removed: HashSet<(String, String)> = HashSet::new();

    for entry in &diff_entries {
        match entry.status {
            DiffStatus::Added | DiffStatus::Modified => {
                // Read NEW version
                let new_source = read_new(&entry.path)?;
                let new_parsed = parse_source(&new_source, &entry.path);

                // Read OLD version (None for new files -> empty old chunks)
                let old_chunks = match read_old(entry, &entry.path)? {
                    Some(old_source) => parse_source(&old_source, &entry.path).chunks,
                    None => vec![],
                };

                let file_diff = compare_chunks(&old_chunks, &new_parsed.chunks);
                all_query_chunks.extend(file_diff.query_chunks);
                all_removed.extend(file_diff.removed);
            }
            DiffStatus::Deleted => {
                // Parse OLD version, all functions go into removed set
                if let Some(old_source) = read_old(entry, &entry.path)? {
                    let old_parsed = parse_source(&old_source, &entry.path);
                    for chunk in &old_parsed.chunks {
                        all_removed.insert((
                            chunk.file_path.clone(),
                            chunk.function_name.clone(),
                        ));
                    }
                }
            }
        }
    }

    let indexed_count = count_functions(&conn)?;
    let query_count = all_query_chunks.len();

    // If no query chunks, return empty result
    if all_query_chunks.is_empty() {
        close_database(conn)?;
        return Ok(QueryResult {
            query_functions: vec![],
            warnings,
            meta: QueryMeta {
                model: embedder.model_name().to_string(),
                indexed_functions: indexed_count,
                query_functions: 0,
                elapsed_ms: start.elapsed().as_millis(),
            },
        });
    }

    // Call query_chunks with the collected chunks
    let chunk_options = QueryChunkOptions {
        top_k: params.top_k,
        threshold: params.threshold,
        project_root,
    };
    let (mut query_functions, chunk_warnings) =
        query_chunks(&all_query_chunks, &conn, embedder, &chunk_options)?;
    warnings.extend(chunk_warnings);

    // Post-filter: remove candidates where (candidate.path, candidate.name) is in the removed set
    // Build a borrowed reference set to avoid cloning strings for each lookup
    let removed_refs: HashSet<(&str, &str)> = all_removed
        .iter()
        .map(|(p, n)| (p.as_str(), n.as_str()))
        .collect();
    for qf in &mut query_functions {
        qf.candidates
            .retain(|c| !removed_refs.contains(&(c.path.as_str(), c.name.as_str())));
    }

    close_database(conn)?;

    Ok(QueryResult {
        query_functions,
        warnings,
        meta: QueryMeta {
            model: embedder.model_name().to_string(),
            indexed_functions: indexed_count,
            query_functions: query_count,
            elapsed_ms: start.elapsed().as_millis(),
        },
    })
}

/// Represents the diff analysis for a single file.
#[derive(Debug)]
pub struct FileDiff {
    /// Chunks to query (added or modified)
    pub query_chunks: Vec<FunctionChunk>,
    /// Removed functions for candidate filtering (file_path, function_name)
    pub removed: Vec<(String, String)>,
}

/// Compare old and new parsed chunks to determine added/modified/removed.
/// Comparison key: function_name. Modification detected by content hash (sha256 of source_text).
pub fn compare_chunks(old: &[FunctionChunk], new: &[FunctionChunk]) -> FileDiff {
    // Build a map of old chunks: function_name -> sha256(source_text)
    let old_map: HashMap<&str, String> = old
        .iter()
        .map(|chunk| (chunk.function_name.as_str(), sha256(&chunk.source_text)))
        .collect();

    // Build a set of new function names for checking removed
    let new_names: HashSet<&str> = new.iter().map(|c| c.function_name.as_str()).collect();

    let mut query_chunks = Vec::new();

    for chunk in new {
        match old_map.get(chunk.function_name.as_str()) {
            None => {
                // Added: function_name not in old
                query_chunks.push(chunk.clone());
            }
            Some(old_hash) => {
                let new_hash = sha256(&chunk.source_text);
                if *old_hash != new_hash {
                    // Modified: same name, different content hash
                    query_chunks.push(chunk.clone());
                }
                // else: unchanged, skip
            }
        }
    }

    // Removed: functions in old but not in new
    let removed: Vec<(String, String)> = old
        .iter()
        .filter(|chunk| !new_names.contains(chunk.function_name.as_str()))
        .map(|chunk| (chunk.file_path.clone(), chunk.function_name.clone()))
        .collect();

    FileDiff {
        query_chunks,
        removed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::types::ChunkType;
    use std::collections::HashSet;

    /// Helper to create a FunctionChunk for testing compare_chunks.
    fn make_chunk(file_path: &str, name: &str, source: &str, line: usize) -> FunctionChunk {
        FunctionChunk {
            id: format!("{file_path}:{name}:{line}"),
            file_path: file_path.into(),
            function_name: name.into(),
            source_text: source.into(),
            start_line: line,
            end_line: line + 3,
            params: vec![],
            return_type: None,
            is_exported: false,
            signature_hash: format!("sighash_{name}"),
            signature: format!("() => void"),
            tokens: HashSet::new(),
            chunk_type: ChunkType::Function,
            context: None,
        }
    }

    #[test]
    fn compare_chunks_new_function_added() {
        // A function present in `new` but not in `old` should appear in query_chunks
        let old: Vec<FunctionChunk> = vec![];
        let new = vec![make_chunk("file.ts", "newFunc", "function newFunc() { return 1; }", 1)];

        let diff = compare_chunks(&old, &new);

        assert_eq!(diff.query_chunks.len(), 1, "Added function should appear in query_chunks");
        assert_eq!(diff.query_chunks[0].function_name, "newFunc");
        assert!(diff.removed.is_empty(), "No functions should be removed");
    }

    #[test]
    fn compare_chunks_function_removed() {
        // A function present in `old` but not in `new` should appear in removed
        let old = vec![make_chunk("file.ts", "oldFunc", "function oldFunc() { return 1; }", 1)];
        let new: Vec<FunctionChunk> = vec![];

        let diff = compare_chunks(&old, &new);

        assert!(diff.query_chunks.is_empty(), "No functions should be queried");
        assert_eq!(diff.removed.len(), 1, "Removed function should appear in removed list");
        assert_eq!(diff.removed[0], ("file.ts".to_string(), "oldFunc".to_string()));
    }

    #[test]
    fn compare_chunks_function_modified() {
        // Same function name, different source text (different content hash) = modified
        let old = vec![make_chunk("file.ts", "myFunc", "function myFunc() { return 1; }", 1)];
        let new = vec![make_chunk("file.ts", "myFunc", "function myFunc() { return 2; }", 1)];

        let diff = compare_chunks(&old, &new);

        assert_eq!(diff.query_chunks.len(), 1, "Modified function should appear in query_chunks");
        assert_eq!(diff.query_chunks[0].function_name, "myFunc");
        assert!(diff.removed.is_empty(), "Modified function should not be in removed list");
    }

    #[test]
    fn compare_chunks_function_unchanged() {
        // Same function name and same source text = unchanged, should not appear anywhere
        let source = "function stableFunc() { return 42; }";
        let old = vec![make_chunk("file.ts", "stableFunc", source, 1)];
        let new = vec![make_chunk("file.ts", "stableFunc", source, 1)];

        let diff = compare_chunks(&old, &new);

        assert!(diff.query_chunks.is_empty(), "Unchanged function should not be queried");
        assert!(diff.removed.is_empty(), "Unchanged function should not be removed");
    }

    #[test]
    fn compare_chunks_mixed_scenario() {
        // Mix of added, modified, removed, and unchanged functions
        let unchanged_source = "function unchanged() { return 0; }";

        let old = vec![
            make_chunk("file.ts", "unchanged", unchanged_source, 1),
            make_chunk("file.ts", "modified", "function modified() { return 1; }", 5),
            make_chunk("file.ts", "removed", "function removed() { return 2; }", 10),
        ];

        let new = vec![
            make_chunk("file.ts", "unchanged", unchanged_source, 1),
            make_chunk("file.ts", "modified", "function modified() { return 99; }", 5),
            make_chunk("file.ts", "added", "function added() { return 3; }", 15),
        ];

        let diff = compare_chunks(&old, &new);

        // query_chunks should contain "modified" and "added"
        let queried_names: Vec<&str> = diff
            .query_chunks
            .iter()
            .map(|c| c.function_name.as_str())
            .collect();
        assert!(
            queried_names.contains(&"modified"),
            "Modified function should be queried"
        );
        assert!(
            queried_names.contains(&"added"),
            "Added function should be queried"
        );
        assert!(
            !queried_names.contains(&"unchanged"),
            "Unchanged function should not be queried"
        );
        assert!(
            !queried_names.contains(&"removed"),
            "Removed function should not be queried"
        );
        assert_eq!(diff.query_chunks.len(), 2);

        // removed should contain "removed"
        let removed_names: Vec<&str> = diff.removed.iter().map(|(_, n)| n.as_str()).collect();
        assert!(
            removed_names.contains(&"removed"),
            "Removed function should be in removed list"
        );
        assert_eq!(diff.removed.len(), 1);
    }
}
