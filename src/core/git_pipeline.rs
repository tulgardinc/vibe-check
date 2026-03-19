use crate::embedder::types::{Embedder, OllamaConfig};
use crate::error::VibecheckError;
use crate::output::types::QueryResult;
use crate::parser::types::FunctionChunk;

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
    todo!("git-integration: not yet implemented")
}

/// Query a specific commit's changes for duplicates against the current index.
/// Returns the same QueryResult as vibec query.
pub fn run_commit_query(options: CommitQueryOptions) -> Result<QueryResult, VibecheckError> {
    todo!("git-integration: not yet implemented")
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
    todo!("git-integration: not yet implemented")
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
