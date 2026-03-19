use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::TempDir;
use vibecheck::core::git_pipeline::{run_commit_query, run_git_query, CommitQueryOptions, GitQueryOptions};
use vibecheck::core::index_pipeline::{run_index, IndexOptions};
use vibecheck::core::query_pipeline::{run_query, QueryOptions};
use vibecheck::core::scan_pipeline::{run_scan, ScanOptions};
use vibecheck::core::status_pipeline::{run_status, StatusOptions};
use vibecheck::embedder::types::{Embedder, OllamaConfig};
use vibecheck::error::VibecheckError;

/// A deterministic mock embedder that produces unique embeddings per input.
struct MockEmbedder {
    dimensions: usize,
}

impl MockEmbedder {
    fn new(dimensions: usize) -> Self {
        Self { dimensions }
    }

    fn hash_to_embedding(&self, input: &str) -> Vec<f32> {
        let mut embedding = vec![0.0f32; self.dimensions];
        let mut hasher = DefaultHasher::new();
        input.hash(&mut hasher);
        let base = hasher.finish();

        for (i, val) in embedding.iter_mut().enumerate() {
            let mut h = DefaultHasher::new();
            (base, i).hash(&mut h);
            let bits = h.finish();
            // Normalize to [-1, 1] range
            *val = (bits as f32 / u64::MAX as f32) * 2.0 - 1.0;
        }

        // Normalize to unit vector for cosine similarity
        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for val in &mut embedding {
                *val /= norm;
            }
        }
        embedding
    }
}

impl Embedder for MockEmbedder {
    fn model_name(&self) -> &str {
        "mock-embed"
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn tier(&self) -> &str {
        "test"
    }

    fn embed_batch(
        &self,
        inputs: &[&str],
        on_progress: Option<&dyn Fn(usize, usize)>,
    ) -> Result<Vec<Vec<f32>>, VibecheckError> {
        let results: Vec<Vec<f32>> = inputs.iter().map(|s| self.hash_to_embedding(s)).collect();
        if let Some(cb) = on_progress {
            cb(results.len(), inputs.len());
        }
        Ok(results)
    }

    fn embed_query(&self, input: &str) -> Result<Vec<f32>, VibecheckError> {
        Ok(self.hash_to_embedding(input))
    }
}

fn write_ts_file(dir: &std::path::Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).unwrap();
}

fn setup_project(dir: &std::path::Path) {
    // Create .git directory so find_project_root works
    std::fs::create_dir_all(dir.join(".git")).unwrap();
}

#[test]
fn index_and_status_round_trip() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    setup_project(dir);

    write_ts_file(
        dir,
        "math.ts",
        r#"
export function add(a: number, b: number): number {
    return a + b;
}

export function multiply(a: number, b: number): number {
    return a * b;
}
"#,
    );

    let db_path = dir.join(".vibecheck.db").to_string_lossy().to_string();

    let result = run_index(IndexOptions {
        path: Some(dir.to_string_lossy().to_string()),
        db_path: Some(db_path.clone()),
        force: false,
        ollama: OllamaConfig::default(),
        progress: None,
        cancel: None,
        embedder: Some(Box::new(MockEmbedder::new(64))),
    })
    .unwrap();

    assert_eq!(result.files_scanned, 1);
    assert!(result.functions_indexed >= 2);
    assert_eq!(result.model, "mock-embed");

    // Status should reflect the indexed data
    let status = run_status(StatusOptions {
        db_path: Some(db_path),
    })
    .unwrap();

    assert!(status.exists);
    assert!(status.indexed_functions >= 2);
    assert_eq!(status.tracked_files, 1);
    assert_eq!(status.unembedded, 0);
    assert_eq!(status.model, "mock-embed");
}

#[test]
fn incremental_indexing() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    setup_project(dir);

    write_ts_file(
        dir,
        "a.ts",
        r#"
export function greet(name: string): string {
    return "Hello " + name;
}
"#,
    );

    let db_path = dir.join(".vibecheck.db").to_string_lossy().to_string();
    let embedder = || Some(Box::new(MockEmbedder::new(64)) as Box<dyn Embedder>);

    // First index
    let r1 = run_index(IndexOptions {
        path: Some(dir.to_string_lossy().to_string()),
        db_path: Some(db_path.clone()),
        force: false,
        ollama: OllamaConfig::default(),
        progress: None,
        cancel: None,
        embedder: embedder(),
    })
    .unwrap();

    assert_eq!(r1.added, 1);
    assert_eq!(r1.modified, 0);

    // Add a new file
    write_ts_file(
        dir,
        "b.ts",
        r#"
export function farewell(name: string): string {
    return "Goodbye " + name;
}
"#,
    );

    // Re-index — should detect 1 added, 0 modified
    let r2 = run_index(IndexOptions {
        path: Some(dir.to_string_lossy().to_string()),
        db_path: Some(db_path.clone()),
        force: false,
        ollama: OllamaConfig::default(),
        progress: None,
        cancel: None,
        embedder: embedder(),
    })
    .unwrap();

    assert_eq!(r2.added, 1);
    assert_eq!(r2.modified, 0);

    let status = run_status(StatusOptions {
        db_path: Some(db_path),
    })
    .unwrap();
    assert_eq!(status.tracked_files, 2);
}

#[test]
fn query_finds_similar_functions() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    setup_project(dir);

    write_ts_file(
        dir,
        "utils.ts",
        r#"
export function calculateSum(a: number, b: number): number {
    return a + b;
}

export function calculateProduct(a: number, b: number): number {
    return a * b;
}

export function formatDate(date: Date): string {
    return date.toISOString();
}
"#,
    );

    let db_path = dir.join(".vibecheck.db").to_string_lossy().to_string();
    let embedder = || Some(Box::new(MockEmbedder::new(64)) as Box<dyn Embedder>);

    // Index
    run_index(IndexOptions {
        path: Some(dir.to_string_lossy().to_string()),
        db_path: Some(db_path.clone()),
        force: false,
        ollama: OllamaConfig::default(),
        progress: None,
        cancel: None,
        embedder: embedder(),
    })
    .unwrap();

    // Query with similar code
    let query_source = r#"
function addNumbers(x: number, y: number): number {
    return x + y;
}
"#;

    let result = run_query(QueryOptions {
        source: query_source.to_string(),
        file_name: Some("query.ts".into()),
        top_k: 5,
        threshold: 1.0, // very permissive for mock embeddings
        db_path: Some(db_path),
        project_root: Some(dir.to_string_lossy().to_string()),
        ollama: OllamaConfig::default(),
        embedder: embedder(),
    })
    .unwrap();

    // Should find at least 1 query function parsed
    assert!(!result.query_functions.is_empty());
    assert_eq!(result.meta.model, "mock-embed");
}

#[test]
fn scan_finds_pairs() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    setup_project(dir);

    // Two files with similar functions
    write_ts_file(
        dir,
        "a.ts",
        r#"
export function processData(items: string[]): string[] {
    return items.map(item => item.trim()).filter(item => item.length > 0);
}
"#,
    );

    write_ts_file(
        dir,
        "b.ts",
        r#"
export function cleanData(entries: string[]): string[] {
    return entries.map(entry => entry.trim()).filter(entry => entry.length > 0);
}
"#,
    );

    let db_path = dir.join(".vibecheck.db").to_string_lossy().to_string();

    run_index(IndexOptions {
        path: Some(dir.to_string_lossy().to_string()),
        db_path: Some(db_path.clone()),
        force: false,
        ollama: OllamaConfig::default(),
        progress: None,
        cancel: None,
        embedder: Some(Box::new(MockEmbedder::new(64))),
    })
    .unwrap();

    let result = run_scan(ScanOptions {
        top_n: 50,
        threshold: 1.0, // permissive for mock embeddings
        db_path: Some(db_path),
        project_root: Some(dir.to_string_lossy().to_string()),
        on_progress: None,
    })
    .unwrap();

    // Scan should find some pairs (exact count depends on mock embeddings)
    assert!(result.meta.chunks_scanned >= 2);
}

#[test]
fn schema_migration_idempotent() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("test.db").to_string_lossy().to_string();

    // Open twice — migrations should be idempotent
    {
        let _conn = vibecheck::store::db::open_database_no_vec(&db_path).unwrap();
    }
    {
        let _conn = vibecheck::store::db::open_database_no_vec(&db_path).unwrap();
    }
}

#[test]
fn empty_project_returns_zero() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    setup_project(dir);

    let db_path = dir.join(".vibecheck.db").to_string_lossy().to_string();

    let result = run_index(IndexOptions {
        path: Some(dir.to_string_lossy().to_string()),
        db_path: Some(db_path),
        force: false,
        ollama: OllamaConfig::default(),
        progress: None,
        cancel: None,
        embedder: Some(Box::new(MockEmbedder::new(64))),
    })
    .unwrap();

    assert_eq!(result.files_scanned, 0);
    assert_eq!(result.functions_indexed, 0);
}

/// A counting mock embedder that tracks how many inputs were embedded via embed_batch.
struct CountingMockEmbedder {
    inner: MockEmbedder,
    embed_count: AtomicUsize,
}

impl CountingMockEmbedder {
    fn new(dimensions: usize) -> Self {
        Self {
            inner: MockEmbedder::new(dimensions),
            embed_count: AtomicUsize::new(0),
        }
    }

    fn embed_call_count(&self) -> usize {
        self.embed_count.load(Ordering::SeqCst)
    }
}

impl Embedder for CountingMockEmbedder {
    fn model_name(&self) -> &str {
        self.inner.model_name()
    }

    fn dimensions(&self) -> usize {
        self.inner.dimensions()
    }

    fn tier(&self) -> &str {
        self.inner.tier()
    }

    fn embed_batch(
        &self,
        inputs: &[&str],
        on_progress: Option<&dyn Fn(usize, usize)>,
    ) -> Result<Vec<Vec<f32>>, VibecheckError> {
        self.embed_count.fetch_add(inputs.len(), Ordering::SeqCst);
        self.inner.embed_batch(inputs, on_progress)
    }

    fn embed_query(&self, input: &str) -> Result<Vec<f32>, VibecheckError> {
        self.inner.embed_query(input)
    }
}

fn setup_git_repo(dir: &std::path::Path) {
    std::process::Command::new("git")
        .args(["init", &dir.to_string_lossy()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("failed to run git init");
    std::process::Command::new("git")
        .args(["-C", &dir.to_string_lossy(), "config", "user.email", "test@test.com"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok();
    std::process::Command::new("git")
        .args(["-C", &dir.to_string_lossy(), "config", "user.name", "Test"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok();
}

/// Test that cache hits skip Ollama calls.
/// First index populates the cache. Delete the DB, then re-index.
/// The second index should find cache hits and skip Ollama entirely.
#[test]
fn cache_hits_skip_ollama_calls() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    setup_git_repo(dir);

    write_ts_file(
        dir,
        "math.ts",
        r#"
export function add(a: number, b: number): number {
    return a + b;
}

export function subtract(a: number, b: number): number {
    return a - b;
}
"#,
    );

    let db_path = dir.join(".vibecheck.db").to_string_lossy().to_string();
    let dims = 64;

    // First index: all functions go through Ollama, populating the cache
    run_index(IndexOptions {
        path: Some(dir.to_string_lossy().to_string()),
        db_path: Some(db_path.clone()),
        force: false,
        ollama: OllamaConfig::default(),
        progress: None,
        cancel: None,
        embedder: Some(Box::new(MockEmbedder::new(dims))),
    })
    .unwrap();

    // Verify cache was populated
    let cache_path = dir.join(".git").join("vibecheck-cache.db");
    assert!(cache_path.exists(), "Cache should exist after first index");
    let stats = vibecheck::store::cache::cache_stats(&cache_path).unwrap();
    assert!(stats.entry_count >= 2, "Cache should have at least 2 entries");

    // Delete the index DB to force re-embedding
    std::fs::remove_file(&db_path).unwrap();
    // Also remove WAL/SHM files if they exist
    let _ = std::fs::remove_file(format!("{db_path}-wal"));
    let _ = std::fs::remove_file(format!("{db_path}-shm"));

    // Second index: should find cache hits and skip Ollama
    let embedder2 = std::sync::Arc::new(CountingMockEmbedder::new(dims));
    let embedder2_clone = embedder2.clone();
    run_index(IndexOptions {
        path: Some(dir.to_string_lossy().to_string()),
        db_path: Some(db_path.clone()),
        force: false,
        ollama: OllamaConfig::default(),
        progress: None,
        cancel: None,
        embedder: Some(Box::new(ArcEmbedder(embedder2.clone()))),
    })
    .unwrap();

    // With cache hits, the embedder should NOT have been called
    let count2 = embedder2_clone.embed_call_count();
    assert_eq!(
        count2, 0,
        "Cache hits should skip Ollama; expected 0 embed calls but got {count2}"
    );
}

/// Wrapper to use Arc<CountingMockEmbedder> as Box<dyn Embedder>
struct ArcEmbedder(std::sync::Arc<CountingMockEmbedder>);

impl Embedder for ArcEmbedder {
    fn model_name(&self) -> &str { self.0.model_name() }
    fn dimensions(&self) -> usize { self.0.dimensions() }
    fn tier(&self) -> &str { self.0.tier() }
    fn embed_batch(
        &self,
        inputs: &[&str],
        on_progress: Option<&dyn Fn(usize, usize)>,
    ) -> Result<Vec<Vec<f32>>, VibecheckError> {
        self.0.embed_batch(inputs, on_progress)
    }
    fn embed_query(&self, input: &str) -> Result<Vec<f32>, VibecheckError> {
        self.0.embed_query(input)
    }
}

/// Test that cache misses are written to the cache.
#[test]
fn cache_misses_are_written_to_cache() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    setup_git_repo(dir);

    write_ts_file(
        dir,
        "utils.ts",
        r#"
export function greet(name: string): string {
    return "Hello " + name;
}
"#,
    );

    let db_path = dir.join(".vibecheck.db").to_string_lossy().to_string();

    run_index(IndexOptions {
        path: Some(dir.to_string_lossy().to_string()),
        db_path: Some(db_path.clone()),
        force: false,
        ollama: OllamaConfig::default(),
        progress: None,
        cancel: None,
        embedder: Some(Box::new(MockEmbedder::new(64))),
    })
    .unwrap();

    // Check that the cache DB was created and has entries
    let cache_path = dir.join(".git").join("vibecheck-cache.db");
    assert!(cache_path.exists(), "Cache DB should exist after indexing in a git repo");

    let stats = vibecheck::store::cache::cache_stats(&cache_path);
    assert!(stats.is_some(), "cache_stats should return Some");
    let stats = stats.unwrap();
    assert!(stats.entry_count > 0, "Cache should have entries after indexing");
}

/// Test that non-git projects skip cache entirely.
#[test]
fn non_git_project_skips_cache() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    // Use setup_project which creates a fake .git dir (not a real git repo)
    setup_project(dir);

    write_ts_file(
        dir,
        "app.ts",
        r#"
export function run(): void {
    console.log("running");
}
"#,
    );

    let db_path = dir.join(".vibecheck.db").to_string_lossy().to_string();

    let result = run_index(IndexOptions {
        path: Some(dir.to_string_lossy().to_string()),
        db_path: Some(db_path.clone()),
        force: false,
        ollama: OllamaConfig::default(),
        progress: None,
        cancel: None,
        embedder: Some(Box::new(MockEmbedder::new(64))),
    })
    .unwrap();

    assert!(result.functions_indexed >= 1, "Should have indexed functions");

    // No cache DB should exist since it's not a real git repo
    let cache_path = dir.join(".git").join("vibecheck-cache.db");
    assert!(
        !cache_path.exists(),
        "Cache DB should NOT exist for non-git projects"
    );
}

// ============================================================================
// Git integration end-to-end tests
// ============================================================================

/// Helper to run a git command in a directory, returning its stdout.
fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([&["-C", &dir.to_string_lossy()], args].concat())
        .output()
        .unwrap_or_else(|e| panic!("failed to run git {}: {e}", args.join(" ")));
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("git {} failed: {stderr}", args.join(" "));
    }
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Helper to set up a git repo with an initial commit containing source files.
/// Returns the initial commit hash.
fn setup_indexed_git_repo(
    dir: &std::path::Path,
    files: &[(&str, &str)],
    dims: usize,
) -> String {
    setup_git_repo(dir);

    for (name, content) in files {
        write_ts_file(dir, name, content);
    }

    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", "initial"]);
    let commit = git(dir, &["rev-parse", "HEAD"]);

    let db_path = dir.join(".vibecheck.db").to_string_lossy().to_string();
    run_index(IndexOptions {
        path: Some(dir.to_string_lossy().to_string()),
        db_path: Some(db_path),
        force: false,
        ollama: OllamaConfig::default(),
        progress: None,
        cancel: None,
        embedder: Some(Box::new(MockEmbedder::new(dims))),
    })
    .unwrap();

    commit
}

/// Helper to build GitQueryOptions from a dir.
fn git_query_opts(dir: &std::path::Path, dims: usize) -> GitQueryOptions {
    GitQueryOptions {
        top_k: 10,
        threshold: 1.0,
        db_path: Some(dir.join(".vibecheck.db").to_string_lossy().to_string()),
        project_root: Some(dir.to_string_lossy().to_string()),
        ollama: OllamaConfig::default(),
        embedder: Some(Box::new(MockEmbedder::new(dims))),
    }
}

// --- run_git_query tests ---

#[test]
fn git_query_detects_new_uncommitted_file() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let dims = 64;

    setup_indexed_git_repo(
        dir,
        &[("existing.ts", r#"
export function existingFunc(x: number): number {
    return x * 2;
}
"#)],
        dims,
    );

    // Add a new file and stage it (git diff HEAD requires files to be tracked or staged)
    write_ts_file(dir, "new_file.ts", r#"
export function brandNewFunc(a: string): string {
    return a.toUpperCase();
}
"#);
    git(dir, &["add", "new_file.ts"]);

    let result = run_git_query(git_query_opts(dir, dims)).unwrap();

    assert_eq!(result.meta.query_functions, 1, "Should detect 1 new function");
    assert_eq!(result.query_functions[0].name, "brandNewFunc");
}

#[test]
fn git_query_detects_modified_function() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let dims = 64;

    setup_indexed_git_repo(
        dir,
        &[("math.ts", r#"
export function compute(a: number, b: number): number {
    return a + b;
}

export function unchanged(x: number): number {
    return x;
}
"#)],
        dims,
    );

    // Modify one function, leave the other unchanged
    write_ts_file(dir, "math.ts", r#"
export function compute(a: number, b: number): number {
    return a * b + a;
}

export function unchanged(x: number): number {
    return x;
}
"#);

    let result = run_git_query(git_query_opts(dir, dims)).unwrap();

    let queried_names: Vec<&str> = result.query_functions.iter().map(|qf| qf.name.as_str()).collect();
    assert!(queried_names.contains(&"compute"), "Modified function should be queried");
    assert!(!queried_names.contains(&"unchanged"), "Unchanged function should not be queried");
}

#[test]
fn git_query_no_changes_returns_empty() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let dims = 64;

    setup_indexed_git_repo(
        dir,
        &[("app.ts", r#"
export function main(): void {
    console.log("hello");
}
"#)],
        dims,
    );

    // No changes made after commit + index
    let result = run_git_query(git_query_opts(dir, dims)).unwrap();

    assert_eq!(result.query_functions.len(), 0, "No changes should produce no query functions");
}

#[test]
fn git_query_on_non_git_repo_errors() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    // Use fake .git dir (not a real git repo)
    setup_project(dir);

    write_ts_file(dir, "test.ts", r#"
export function foo(): void {
    console.log("test");
}
"#);

    let db_path = dir.join(".vibecheck.db").to_string_lossy().to_string();
    run_index(IndexOptions {
        path: Some(dir.to_string_lossy().to_string()),
        db_path: Some(db_path.clone()),
        force: false,
        ollama: OllamaConfig::default(),
        progress: None,
        cancel: None,
        embedder: Some(Box::new(MockEmbedder::new(64))),
    })
    .unwrap();

    let result = run_git_query(GitQueryOptions {
        top_k: 5,
        threshold: 1.0,
        db_path: Some(db_path),
        project_root: Some(dir.to_string_lossy().to_string()),
        ollama: OllamaConfig::default(),
        embedder: Some(Box::new(MockEmbedder::new(64))),
    });

    assert!(result.is_err(), "Should error on non-git repo");
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Not a git repository"), "Error should mention not a git repo, got: {err}");
}

#[test]
fn git_query_shows_staleness_warning() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let dims = 64;

    setup_indexed_git_repo(
        dir,
        &[("a.ts", r#"
export function alpha(): number {
    return 1;
}
"#)],
        dims,
    );

    // Make a new commit (index is now stale)
    write_ts_file(dir, "b.ts", r#"
export function beta(): number {
    return 2;
}
"#);
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", "second commit"]);

    // Now make a working tree change so there's something to query
    write_ts_file(dir, "c.ts", r#"
export function gamma(): number {
    return 3;
}
"#);

    let result = run_git_query(git_query_opts(dir, dims)).unwrap();

    let has_staleness_warning = result.warnings.iter().any(|w| w.contains("Index was built at commit"));
    assert!(has_staleness_warning, "Should include staleness warning, got: {:?}", result.warnings);
}

#[test]
fn git_query_filters_deleted_file_candidates() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let dims = 64;

    // Index two files with similar functions
    setup_indexed_git_repo(
        dir,
        &[
            ("a.ts", r#"
export function processData(items: string[]): string[] {
    return items.map(i => i.trim());
}
"#),
            ("b.ts", r#"
export function cleanData(entries: string[]): string[] {
    return entries.map(e => e.trim());
}
"#),
        ],
        dims,
    );

    // Delete file b.ts and add a new similar function (must stage new file)
    std::fs::remove_file(dir.join("b.ts")).unwrap();
    write_ts_file(dir, "c.ts", r#"
export function sanitizeData(values: string[]): string[] {
    return values.map(v => v.trim());
}
"#);
    git(dir, &["add", "c.ts"]);

    let result = run_git_query(git_query_opts(dir, dims)).unwrap();

    // The new function should be queried
    let queried_names: Vec<&str> = result.query_functions.iter().map(|qf| qf.name.as_str()).collect();
    assert!(queried_names.contains(&"sanitizeData"), "New function should be queried");

    // Candidates for sanitizeData should NOT include cleanData (deleted)
    for qf in &result.query_functions {
        for candidate in &qf.candidates {
            assert_ne!(
                candidate.name, "cleanData",
                "Deleted function 'cleanData' should be filtered from candidates"
            );
        }
    }
}

#[test]
fn git_query_handles_multiple_files_with_mixed_changes() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let dims = 64;

    setup_indexed_git_repo(
        dir,
        &[
            ("utils.ts", r#"
export function formatName(name: string): string {
    return name.trim();
}

export function formatAge(age: number): string {
    return String(age);
}
"#),
            ("helpers.ts", r#"
export function toUpper(s: string): string {
    return s.toUpperCase();
}
"#),
        ],
        dims,
    );

    // Modify one function in utils.ts, delete helpers.ts, add new file
    write_ts_file(dir, "utils.ts", r#"
export function formatName(name: string): string {
    return name.trim().toLowerCase();
}

export function formatAge(age: number): string {
    return String(age);
}
"#);
    std::fs::remove_file(dir.join("helpers.ts")).unwrap();
    write_ts_file(dir, "new.ts", r#"
export function validate(input: string): boolean {
    return input.length > 0;
}
"#);
    git(dir, &["add", "new.ts"]);

    let result = run_git_query(git_query_opts(dir, dims)).unwrap();

    let queried_names: Vec<&str> = result.query_functions.iter().map(|qf| qf.name.as_str()).collect();
    assert!(queried_names.contains(&"formatName"), "Modified function should be queried");
    assert!(queried_names.contains(&"validate"), "New function should be queried");
    assert!(!queried_names.contains(&"formatAge"), "Unchanged function should not be queried");
    assert!(!queried_names.contains(&"toUpper"), "Deleted function should not be queried");
}

// --- run_commit_query tests ---

#[test]
fn commit_query_detects_added_functions() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let dims = 64;

    // Initial commit with one file
    setup_indexed_git_repo(
        dir,
        &[("base.ts", r#"
export function baseFunc(): number {
    return 42;
}
"#)],
        dims,
    );

    // Make a second commit adding a new file
    write_ts_file(dir, "feature.ts", r#"
export function newFeature(x: number): number {
    return x * x;
}
"#);
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", "add feature"]);
    let commit_hash = git(dir, &["rev-parse", "HEAD"]);

    let result = run_commit_query(CommitQueryOptions {
        hash: commit_hash,
        top_k: 10,
        threshold: 1.0,
        db_path: Some(dir.join(".vibecheck.db").to_string_lossy().to_string()),
        project_root: Some(dir.to_string_lossy().to_string()),
        ollama: OllamaConfig::default(),
        embedder: Some(Box::new(MockEmbedder::new(dims))),
    })
    .unwrap();

    assert_eq!(result.meta.query_functions, 1, "Should detect 1 new function in commit");
    assert_eq!(result.query_functions[0].name, "newFeature");
}

#[test]
fn commit_query_detects_modified_functions() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let dims = 64;

    setup_indexed_git_repo(
        dir,
        &[("calc.ts", r#"
export function calculate(a: number, b: number): number {
    return a + b;
}

export function helper(): string {
    return "ok";
}
"#)],
        dims,
    );

    // Make a second commit modifying one function
    write_ts_file(dir, "calc.ts", r#"
export function calculate(a: number, b: number): number {
    return a * b * 2;
}

export function helper(): string {
    return "ok";
}
"#);
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", "modify calculate"]);
    let commit_hash = git(dir, &["rev-parse", "HEAD"]);

    let result = run_commit_query(CommitQueryOptions {
        hash: commit_hash,
        top_k: 10,
        threshold: 1.0,
        db_path: Some(dir.join(".vibecheck.db").to_string_lossy().to_string()),
        project_root: Some(dir.to_string_lossy().to_string()),
        ollama: OllamaConfig::default(),
        embedder: Some(Box::new(MockEmbedder::new(dims))),
    })
    .unwrap();

    let queried_names: Vec<&str> = result.query_functions.iter().map(|qf| qf.name.as_str()).collect();
    assert!(queried_names.contains(&"calculate"), "Modified function should be queried");
    assert!(!queried_names.contains(&"helper"), "Unchanged function should not be queried");
}

#[test]
fn commit_query_initial_commit() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let dims = 64;

    // Create a git repo with a single initial commit
    let commit = setup_indexed_git_repo(
        dir,
        &[("init.ts", r#"
export function initialFunc(): string {
    return "first";
}
"#)],
        dims,
    );

    // Query the initial commit (has no parent)
    let result = run_commit_query(CommitQueryOptions {
        hash: commit,
        top_k: 10,
        threshold: 1.0,
        db_path: Some(dir.join(".vibecheck.db").to_string_lossy().to_string()),
        project_root: Some(dir.to_string_lossy().to_string()),
        ollama: OllamaConfig::default(),
        embedder: Some(Box::new(MockEmbedder::new(dims))),
    })
    .unwrap();

    // Initial commit should show all added functions
    assert_eq!(result.meta.query_functions, 1, "Initial commit should detect 1 function");
    assert_eq!(result.query_functions[0].name, "initialFunc");
}

// --- status with cache fields ---

#[test]
fn cache_populated_after_indexing_in_git_repo() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let dims = 64;

    setup_indexed_git_repo(
        dir,
        &[("app.ts", r#"
export function appMain(): void {
    console.log("running");
}
"#)],
        dims,
    );

    // Verify cache was populated by checking directly via cache_stats
    let cache_path = dir.join(".git").join("vibecheck-cache.db");
    assert!(cache_path.exists(), "Cache file should exist after indexing in git repo");
    let stats = vibecheck::store::cache::cache_stats(&cache_path);
    assert!(stats.is_some(), "cache_stats should return Some");
    let stats = stats.unwrap();
    assert!(stats.entry_count > 0, "Cache should have entries after indexing");
    assert!(stats.size_bytes > 0, "Cache file should have size > 0");
}

#[test]
fn status_shows_no_cache_for_non_git_project() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    setup_project(dir);

    write_ts_file(dir, "test.ts", r#"
export function test(): void {
    console.log("test");
}
"#);

    let db_path = dir.join(".vibecheck.db").to_string_lossy().to_string();
    run_index(IndexOptions {
        path: Some(dir.to_string_lossy().to_string()),
        db_path: Some(db_path.clone()),
        force: false,
        ollama: OllamaConfig::default(),
        progress: None,
        cancel: None,
        embedder: Some(Box::new(MockEmbedder::new(64))),
    })
    .unwrap();

    let status = run_status(StatusOptions {
        db_path: Some(db_path),
    })
    .unwrap();

    assert!(!status.cache_exists, "Non-git project should not have cache");
    assert_eq!(status.cache_entry_count, 0);
}

// --- cache prune e2e ---

#[test]
fn cache_prune_removes_orphaned_entries() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let dims = 64;

    setup_indexed_git_repo(
        dir,
        &[("lib.ts", r#"
export function libFunc(x: number): number {
    return x + 1;
}
"#)],
        dims,
    );

    // Manually insert orphan entries into the cache
    let cache_path = dir.join(".git").join("vibecheck-cache.db");
    assert!(cache_path.exists(), "Cache should exist");

    let cache_conn = vibecheck::store::cache::open_cache(&cache_path, "mock-embed", dims, 16000, false).unwrap();
    vibecheck::store::cache::insert_embedding(&cache_conn, "orphan_hash_1", &[99; 256]).unwrap();
    vibecheck::store::cache::insert_embedding(&cache_conn, "orphan_hash_2", &[88; 256]).unwrap();
    vibecheck::store::cache::close_cache(cache_conn).unwrap();

    let stats_before = vibecheck::store::cache::cache_stats(&cache_path).unwrap();
    let count_before = stats_before.entry_count;

    let prune_result = vibecheck::store::cache::prune_cache(&cache_path, dir).unwrap();

    assert_eq!(prune_result.entries_removed, 2, "Should remove 2 orphan entries");

    let stats_after = vibecheck::store::cache::cache_stats(&cache_path).unwrap();
    assert_eq!(
        stats_after.entry_count,
        count_before - 2,
        "Entry count should decrease by 2"
    );
}

// --- git query with staged changes ---

#[test]
fn git_query_detects_staged_but_uncommitted_changes() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let dims = 64;

    setup_indexed_git_repo(
        dir,
        &[("original.ts", r#"
export function original(): string {
    return "original";
}
"#)],
        dims,
    );

    // Add a new file and stage it (but don't commit)
    write_ts_file(dir, "staged.ts", r#"
export function stagedFunc(): string {
    return "staged";
}
"#);
    git(dir, &["add", "staged.ts"]);

    let result = run_git_query(git_query_opts(dir, dims)).unwrap();

    let queried_names: Vec<&str> = result.query_functions.iter().map(|qf| qf.name.as_str()).collect();
    assert!(queried_names.contains(&"stagedFunc"), "Staged new function should be detected");
}

#[test]
fn git_query_ignores_non_source_files() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let dims = 64;

    setup_indexed_git_repo(
        dir,
        &[("app.ts", r#"
export function appFunc(): number {
    return 1;
}
"#)],
        dims,
    );

    // Add non-source files (should be ignored by the pipeline)
    std::fs::write(dir.join("README.md"), "# Hello").unwrap();
    std::fs::write(dir.join("data.json"), "{}").unwrap();
    std::fs::write(dir.join(".env"), "SECRET=123").unwrap();

    let result = run_git_query(git_query_opts(dir, dims)).unwrap();

    assert_eq!(
        result.query_functions.len(), 0,
        "Non-source files should not produce query functions"
    );
}

#[test]
fn commit_query_filters_deleted_functions_from_candidates() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let dims = 64;

    // Index two files with functions
    setup_indexed_git_repo(
        dir,
        &[
            ("keep.ts", r#"
export function keepFunc(): number {
    return 1;
}
"#),
            ("remove.ts", r#"
export function removeFunc(): number {
    return 2;
}
"#),
        ],
        dims,
    );

    // Commit that deletes remove.ts and adds a replacement
    std::fs::remove_file(dir.join("remove.ts")).unwrap();
    write_ts_file(dir, "replace.ts", r#"
export function replaceFunc(): number {
    return 3;
}
"#);
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", "replace file"]);
    let commit_hash = git(dir, &["rev-parse", "HEAD"]);

    let result = run_commit_query(CommitQueryOptions {
        hash: commit_hash,
        top_k: 10,
        threshold: 1.0,
        db_path: Some(dir.join(".vibecheck.db").to_string_lossy().to_string()),
        project_root: Some(dir.to_string_lossy().to_string()),
        ollama: OllamaConfig::default(),
        embedder: Some(Box::new(MockEmbedder::new(dims))),
    })
    .unwrap();

    // replaceFunc should be queried
    let queried_names: Vec<&str> = result.query_functions.iter().map(|qf| qf.name.as_str()).collect();
    assert!(queried_names.contains(&"replaceFunc"), "New function should be queried");

    // removeFunc should not appear as a candidate
    for qf in &result.query_functions {
        for candidate in &qf.candidates {
            assert_ne!(
                candidate.name, "removeFunc",
                "Deleted function should be filtered from candidates"
            );
        }
    }
}

// ============================================================================
// Git worktree + shared cache tests
// ============================================================================

#[test]
fn worktree_shares_cache_with_main_repo() {
    let tmp = TempDir::new().unwrap();
    let main_dir = tmp.path().join("main-repo");
    std::fs::create_dir_all(&main_dir).unwrap();
    let dims = 64;

    // Set up the main repo with an initial commit and index
    setup_indexed_git_repo(
        &main_dir,
        &[("shared.ts", r#"
export function sharedFunc(x: number): number {
    return x + 1;
}
"#)],
        dims,
    );

    // Verify cache exists in main repo's .git
    let cache_path = main_dir.join(".git").join("vibecheck-cache.db");
    assert!(cache_path.exists(), "Cache should exist in main repo");
    let stats_after_main = vibecheck::store::cache::cache_stats(&cache_path).unwrap();
    let count_after_main = stats_after_main.entry_count;
    assert!(count_after_main > 0, "Cache should have entries from main repo indexing");

    // Create a new branch and a worktree for it
    git(&main_dir, &["branch", "feature-branch"]);
    let wt_dir = tmp.path().join("worktree-feature");
    git(&main_dir, &["worktree", "add", &wt_dir.to_string_lossy(), "feature-branch"]);

    // Add a different file in the worktree
    write_ts_file(&wt_dir, "feature.ts", r#"
export function featureFunc(y: string): string {
    return y.toUpperCase();
}
"#);
    git(&wt_dir, &["add", "feature.ts"]);
    git(&wt_dir, &["commit", "-m", "add feature"]);

    // Index the worktree — should use the SAME shared cache
    let wt_db_path = wt_dir.join(".vibecheck.db").to_string_lossy().to_string();
    run_index(IndexOptions {
        path: Some(wt_dir.to_string_lossy().to_string()),
        db_path: Some(wt_db_path.clone()),
        force: false,
        ollama: OllamaConfig::default(),
        progress: None,
        cancel: None,
        embedder: Some(Box::new(MockEmbedder::new(dims))),
    })
    .unwrap();

    // The shared cache should now have MORE entries (from both main + worktree)
    let stats_after_wt = vibecheck::store::cache::cache_stats(&cache_path).unwrap();
    assert!(
        stats_after_wt.entry_count >= count_after_main,
        "Shared cache should have entries from both main ({}) and worktree ({})",
        count_after_main,
        stats_after_wt.entry_count
    );

    // The worktree should NOT have its own cache — it uses the shared one
    let wt_local_cache = wt_dir.join(".git").join("vibecheck-cache.db");
    assert!(
        !wt_local_cache.exists(),
        "Worktree should not have a local cache file (uses shared cache)"
    );
}

#[test]
fn worktree_cache_hits_from_main_repo() {
    let tmp = TempDir::new().unwrap();
    let main_dir = tmp.path().join("main-repo");
    std::fs::create_dir_all(&main_dir).unwrap();
    let dims = 64;

    // Set up main repo with a file and index it
    setup_indexed_git_repo(
        &main_dir,
        &[("lib.ts", r#"
export function libFunc(a: number, b: number): number {
    return a + b;
}
"#)],
        dims,
    );

    // Create worktree on a new branch (has the same file content)
    git(&main_dir, &["branch", "wt-branch"]);
    let wt_dir = tmp.path().join("wt");
    git(&main_dir, &["worktree", "add", &wt_dir.to_string_lossy(), "wt-branch"]);

    // Index the worktree with a counting embedder — should get cache hits
    let wt_db_path = wt_dir.join(".vibecheck.db").to_string_lossy().to_string();
    let embedder = std::sync::Arc::new(CountingMockEmbedder::new(dims));
    run_index(IndexOptions {
        path: Some(wt_dir.to_string_lossy().to_string()),
        db_path: Some(wt_db_path),
        force: false,
        ollama: OllamaConfig::default(),
        progress: None,
        cancel: None,
        embedder: Some(Box::new(ArcEmbedder(embedder.clone()))),
    })
    .unwrap();

    assert_eq!(
        embedder.embed_call_count(), 0,
        "Worktree indexing should get cache hits from main repo — expected 0 embed calls, got {}",
        embedder.embed_call_count()
    );
}

#[test]
fn prune_preserves_entries_referenced_by_any_worktree() {
    let tmp = TempDir::new().unwrap();
    let main_dir = tmp.path().join("main-repo");
    std::fs::create_dir_all(&main_dir).unwrap();
    let dims = 64;

    // Set up main repo
    setup_indexed_git_repo(
        &main_dir,
        &[("main.ts", r#"
export function mainFunc(): number {
    return 1;
}
"#)],
        dims,
    );

    // Create worktree with a different file
    git(&main_dir, &["branch", "prune-branch"]);
    let wt_dir = tmp.path().join("wt-prune");
    git(&main_dir, &["worktree", "add", &wt_dir.to_string_lossy(), "prune-branch"]);

    write_ts_file(&wt_dir, "wt_only.ts", r#"
export function wtOnlyFunc(): string {
    return "worktree";
}
"#);
    git(&wt_dir, &["add", "wt_only.ts"]);
    git(&wt_dir, &["commit", "-m", "worktree-only file"]);

    let wt_db_path = wt_dir.join(".vibecheck.db").to_string_lossy().to_string();
    run_index(IndexOptions {
        path: Some(wt_dir.to_string_lossy().to_string()),
        db_path: Some(wt_db_path),
        force: false,
        ollama: OllamaConfig::default(),
        progress: None,
        cancel: None,
        embedder: Some(Box::new(MockEmbedder::new(dims))),
    })
    .unwrap();

    let cache_path = main_dir.join(".git").join("vibecheck-cache.db");
    let stats_before = vibecheck::store::cache::cache_stats(&cache_path).unwrap();

    // Insert an orphan entry
    let cache_conn = vibecheck::store::cache::open_cache(&cache_path, "mock-embed", dims, 16000, false).unwrap();
    vibecheck::store::cache::insert_embedding(&cache_conn, "orphan_only", &[42; 256]).unwrap();
    vibecheck::store::cache::close_cache(cache_conn).unwrap();

    // Prune from the main repo
    let prune_result = vibecheck::store::cache::prune_cache(&cache_path, &main_dir).unwrap();

    // Only the orphan should be removed; entries from both worktrees are preserved
    assert_eq!(
        prune_result.entries_removed, 1,
        "Only the orphan entry should be pruned, not entries referenced by either worktree"
    );

    let stats_after = vibecheck::store::cache::cache_stats(&cache_path).unwrap();
    assert_eq!(
        stats_after.entry_count,
        stats_before.entry_count,
        "All originally-referenced entries should be preserved"
    );
}

#[test]
fn git_query_works_from_worktree() {
    let tmp = TempDir::new().unwrap();
    let main_dir = tmp.path().join("main-repo");
    std::fs::create_dir_all(&main_dir).unwrap();
    let dims = 64;

    // Set up main repo with indexed content
    setup_indexed_git_repo(
        &main_dir,
        &[("base.ts", r#"
export function baseFunc(): number {
    return 42;
}
"#)],
        dims,
    );

    // Create worktree
    git(&main_dir, &["branch", "query-branch"]);
    let wt_dir = tmp.path().join("wt-query");
    git(&main_dir, &["worktree", "add", &wt_dir.to_string_lossy(), "query-branch"]);

    // Index the worktree
    let wt_db_path = wt_dir.join(".vibecheck.db").to_string_lossy().to_string();
    run_index(IndexOptions {
        path: Some(wt_dir.to_string_lossy().to_string()),
        db_path: Some(wt_db_path.clone()),
        force: false,
        ollama: OllamaConfig::default(),
        progress: None,
        cancel: None,
        embedder: Some(Box::new(MockEmbedder::new(dims))),
    })
    .unwrap();

    // Make a change in the worktree
    write_ts_file(&wt_dir, "feature.ts", r#"
export function featureFunc(): string {
    return "feature";
}
"#);
    git(&wt_dir, &["add", "feature.ts"]);

    // run_git_query from the worktree should work
    let result = run_git_query(GitQueryOptions {
        top_k: 10,
        threshold: 1.0,
        db_path: Some(wt_db_path),
        project_root: Some(wt_dir.to_string_lossy().to_string()),
        ollama: OllamaConfig::default(),
        embedder: Some(Box::new(MockEmbedder::new(dims))),
    })
    .unwrap();

    assert_eq!(result.meta.query_functions, 1, "Should detect the new function in worktree");
    assert_eq!(result.query_functions[0].name, "featureFunc");
}

// ============================================================================
// Diverged worktree cache correctness tests
// ============================================================================

/// Helper to read the stored embedding bytes for a function from a worktree's DB.
fn read_embedding_from_db(db_path: &str, function_name: &str) -> Option<Vec<u8>> {
    let conn = vibecheck::store::db::open_database_no_vec(db_path).unwrap();
    let result: Option<Vec<u8>> = conn
        .prepare("SELECT embedding FROM functions WHERE function_name = ?")
        .unwrap()
        .query_row([function_name], |row| row.get(0))
        .ok();
    vibecheck::store::db::close_database(conn).unwrap();
    result
}

/// Helper to read the content_hash for a function from a worktree's DB.
fn read_content_hash_from_db(db_path: &str, function_name: &str) -> Option<String> {
    let conn = vibecheck::store::db::open_database_no_vec(db_path).unwrap();
    let result: Option<String> = conn
        .prepare("SELECT content_hash FROM functions WHERE function_name = ?")
        .unwrap()
        .query_row([function_name], |row| row.get(0))
        .ok();
    vibecheck::store::db::close_database(conn).unwrap();
    result
}

/// Helper to build and run index for a dir with options.
fn index_dir(dir: &std::path::Path, dims: usize) {
    let db_path = dir.join(".vibecheck.db").to_string_lossy().to_string();
    run_index(IndexOptions {
        path: Some(dir.to_string_lossy().to_string()),
        db_path: Some(db_path),
        force: false,
        ollama: OllamaConfig::default(),
        progress: None,
        cancel: None,
        embedder: Some(Box::new(MockEmbedder::new(dims))),
    })
    .unwrap();
}

#[test]
fn diverged_worktrees_identical_function_gets_same_embedding() {
    // Both worktrees share an unchanged function.
    // The second indexing should get a cache hit and produce
    // byte-for-byte identical embedding in its own DB.
    let tmp = TempDir::new().unwrap();
    let main_dir = tmp.path().join("main-repo");
    std::fs::create_dir_all(&main_dir).unwrap();
    let dims = 64;

    let shared_source = r#"
export function sharedLogic(items: string[]): string[] {
    return items.filter(i => i.length > 0).map(i => i.trim());
}
"#;

    // Main: shared + main-only function
    setup_indexed_git_repo(
        &main_dir,
        &[
            ("shared.ts", shared_source),
            ("main_only.ts", r#"
export function mainOnly(): number {
    return 100;
}
"#),
        ],
        dims,
    );

    // Create worktree on new branch, add a worktree-only file
    git(&main_dir, &["branch", "diverge-a"]);
    let wt_dir = tmp.path().join("wt-diverge-a");
    git(&main_dir, &["worktree", "add", &wt_dir.to_string_lossy(), "diverge-a"]);

    write_ts_file(&wt_dir, "wt_only.ts", r#"
export function wtOnly(): string {
    return "worktree";
}
"#);
    git(&wt_dir, &["add", "wt_only.ts"]);
    git(&wt_dir, &["commit", "-m", "add wt_only"]);

    // Index worktree — shared.ts should get cache hit
    index_dir(&wt_dir, dims);

    // Read the embedding for sharedLogic from both DBs
    let main_db = main_dir.join(".vibecheck.db").to_string_lossy().to_string();
    let wt_db = wt_dir.join(".vibecheck.db").to_string_lossy().to_string();

    let main_embedding = read_embedding_from_db(&main_db, "sharedLogic")
        .expect("sharedLogic should have embedding in main DB");
    let wt_embedding = read_embedding_from_db(&wt_db, "sharedLogic")
        .expect("sharedLogic should have embedding in worktree DB");

    assert_eq!(
        main_embedding, wt_embedding,
        "Identical source text must produce identical embedding bytes across worktrees"
    );

    // Content hashes should also match
    let main_hash = read_content_hash_from_db(&main_db, "sharedLogic").unwrap();
    let wt_hash = read_content_hash_from_db(&wt_db, "sharedLogic").unwrap();
    assert_eq!(main_hash, wt_hash, "Identical source text must produce identical content_hash");
}

#[test]
fn diverged_worktrees_modified_function_gets_different_embedding() {
    // Main and worktree modify the same function differently.
    // Each should have a distinct content_hash and distinct embedding.
    // The cache must not serve one worktree's embedding to the other.
    let tmp = TempDir::new().unwrap();
    let main_dir = tmp.path().join("main-repo");
    std::fs::create_dir_all(&main_dir).unwrap();
    let dims = 64;

    // Initial commit with original function
    setup_git_repo(&main_dir);
    write_ts_file(&main_dir, "diverge.ts", r#"
export function compute(x: number): number {
    return x + 1;
}
"#);
    git(&main_dir, &["add", "."]);
    git(&main_dir, &["commit", "-m", "initial"]);

    // Create worktree branch before modifying main
    git(&main_dir, &["branch", "diverge-b"]);
    let wt_dir = tmp.path().join("wt-diverge-b");
    git(&main_dir, &["worktree", "add", &wt_dir.to_string_lossy(), "diverge-b"]);

    // Modify function differently in main
    write_ts_file(&main_dir, "diverge.ts", r#"
export function compute(x: number): number {
    return x * 2 + 10;
}
"#);
    git(&main_dir, &["add", "."]);
    git(&main_dir, &["commit", "-m", "main version"]);

    // Modify function differently in worktree
    write_ts_file(&wt_dir, "diverge.ts", r#"
export function compute(x: number): number {
    return Math.pow(x, 3) - 5;
}
"#);
    git(&wt_dir, &["add", "."]);
    git(&wt_dir, &["commit", "-m", "worktree version"]);

    // Index both
    index_dir(&main_dir, dims);
    index_dir(&wt_dir, dims);

    let main_db = main_dir.join(".vibecheck.db").to_string_lossy().to_string();
    let wt_db = wt_dir.join(".vibecheck.db").to_string_lossy().to_string();

    // Content hashes must differ (different source text)
    let main_hash = read_content_hash_from_db(&main_db, "compute").unwrap();
    let wt_hash = read_content_hash_from_db(&wt_db, "compute").unwrap();
    assert_ne!(
        main_hash, wt_hash,
        "Diverged functions must have different content_hash values"
    );

    // Embeddings must differ
    let main_embedding = read_embedding_from_db(&main_db, "compute").unwrap();
    let wt_embedding = read_embedding_from_db(&wt_db, "compute").unwrap();
    assert_ne!(
        main_embedding, wt_embedding,
        "Diverged functions must have different embeddings — cache must not cross-contaminate"
    );

    // Both content hashes should exist in the shared cache
    let cache_path = main_dir.join(".git").join("vibecheck-cache.db");
    let cache_conn = vibecheck::store::cache::open_cache(&cache_path, "mock-embed", dims, 16000, false).unwrap();
    let cached = vibecheck::store::cache::lookup_embeddings(
        &cache_conn,
        &[&main_hash, &wt_hash],
    )
    .unwrap();
    vibecheck::store::cache::close_cache(cache_conn).unwrap();

    assert!(
        cached.contains_key(&main_hash),
        "Cache should contain main's content_hash"
    );
    assert!(
        cached.contains_key(&wt_hash),
        "Cache should contain worktree's content_hash"
    );
    assert_ne!(
        cached[&main_hash], cached[&wt_hash],
        "Cache entries for diverged functions must store different embedding bytes"
    );
}

#[test]
fn diverged_worktrees_reindex_after_divergence_is_correct() {
    // After both worktrees diverge and are indexed, deleting one DB and re-indexing
    // should produce the same embeddings from cache (not the other worktree's).
    let tmp = TempDir::new().unwrap();
    let main_dir = tmp.path().join("main-repo");
    std::fs::create_dir_all(&main_dir).unwrap();
    let dims = 64;

    // Initial commit
    setup_git_repo(&main_dir);
    write_ts_file(&main_dir, "lib.ts", r#"
export function transform(s: string): string {
    return s.toLowerCase();
}
"#);
    git(&main_dir, &["add", "."]);
    git(&main_dir, &["commit", "-m", "initial"]);

    // Branch and create worktree
    git(&main_dir, &["branch", "diverge-c"]);
    let wt_dir = tmp.path().join("wt-diverge-c");
    git(&main_dir, &["worktree", "add", &wt_dir.to_string_lossy(), "diverge-c"]);

    // Diverge: main modifies
    write_ts_file(&main_dir, "lib.ts", r#"
export function transform(s: string): string {
    return s.toUpperCase().trim();
}
"#);
    git(&main_dir, &["add", "."]);
    git(&main_dir, &["commit", "-m", "main diverge"]);

    // Diverge: worktree modifies differently
    write_ts_file(&wt_dir, "lib.ts", r#"
export function transform(s: string): string {
    return s.replace(/\s+/g, "-");
}
"#);
    git(&wt_dir, &["add", "."]);
    git(&wt_dir, &["commit", "-m", "wt diverge"]);

    // Index both
    index_dir(&main_dir, dims);
    index_dir(&wt_dir, dims);

    // Record the worktree's embedding before deleting
    let wt_db_path = wt_dir.join(".vibecheck.db").to_string_lossy().to_string();
    let embedding_before = read_embedding_from_db(&wt_db_path, "transform").unwrap();

    // Delete worktree's DB and re-index — should get same embedding from cache
    std::fs::remove_file(&wt_db_path).unwrap();
    let _ = std::fs::remove_file(format!("{wt_db_path}-wal"));
    let _ = std::fs::remove_file(format!("{wt_db_path}-shm"));
    index_dir(&wt_dir, dims);

    let embedding_after = read_embedding_from_db(&wt_db_path, "transform").unwrap();
    assert_eq!(
        embedding_before, embedding_after,
        "Re-indexing from cache must produce the same embedding as the original indexing"
    );

    // Verify it's NOT the main repo's embedding
    let main_db_path = main_dir.join(".vibecheck.db").to_string_lossy().to_string();
    let main_embedding = read_embedding_from_db(&main_db_path, "transform").unwrap();
    assert_ne!(
        embedding_after, main_embedding,
        "Worktree's re-indexed embedding must not be main's embedding"
    );
}

#[test]
fn diverged_worktrees_query_returns_correct_matches() {
    // Two diverged worktrees indexed independently. Querying each index
    // with its own content should return relevant matches, not results
    // polluted by the other worktree's embeddings.
    let tmp = TempDir::new().unwrap();
    let main_dir = tmp.path().join("main-repo");
    std::fs::create_dir_all(&main_dir).unwrap();
    let dims = 64;

    // Initial commit with two files
    setup_git_repo(&main_dir);
    write_ts_file(&main_dir, "alpha.ts", r#"
export function alpha(): number {
    return 1;
}
"#);
    write_ts_file(&main_dir, "beta.ts", r#"
export function beta(): number {
    return 2;
}
"#);
    git(&main_dir, &["add", "."]);
    git(&main_dir, &["commit", "-m", "initial"]);

    // Branch and create worktree
    git(&main_dir, &["branch", "diverge-d"]);
    let wt_dir = tmp.path().join("wt-diverge-d");
    git(&main_dir, &["worktree", "add", &wt_dir.to_string_lossy(), "diverge-d"]);

    // Main: add a new file, keep originals
    write_ts_file(&main_dir, "main_extra.ts", r#"
export function mainExtra(): string {
    return "main-specific";
}
"#);
    git(&main_dir, &["add", "."]);
    git(&main_dir, &["commit", "-m", "main extra"]);

    // Worktree: delete beta.ts, add different file
    std::fs::remove_file(wt_dir.join("beta.ts")).unwrap();
    write_ts_file(&wt_dir, "wt_extra.ts", r#"
export function wtExtra(): string {
    return "worktree-specific";
}
"#);
    git(&wt_dir, &["add", "."]);
    git(&wt_dir, &["commit", "-m", "wt extra"]);

    // Index both
    index_dir(&main_dir, dims);
    index_dir(&wt_dir, dims);

    let main_db = main_dir.join(".vibecheck.db").to_string_lossy().to_string();
    let wt_db = wt_dir.join(".vibecheck.db").to_string_lossy().to_string();

    /// Helper to get all function names from a DB.
    fn function_names_in_db(db_path: &str) -> Vec<String> {
        let conn = vibecheck::store::db::open_database_no_vec(db_path).unwrap();
        let names: Vec<String> = {
            let mut stmt = conn.prepare("SELECT function_name FROM functions ORDER BY function_name").unwrap();
            stmt.query_map([], |row| row.get(0))
                .unwrap()
                .filter_map(|r| r.ok())
                .collect()
        };
        vibecheck::store::db::close_database(conn).unwrap();
        names
    }

    // Main should have: alpha, beta, mainExtra
    let main_funcs = function_names_in_db(&main_db);
    assert_eq!(main_funcs.len(), 3, "Main index should have 3 functions");
    assert!(main_funcs.contains(&"alpha".to_string()), "Main should have alpha");
    assert!(main_funcs.contains(&"beta".to_string()), "Main should have beta");
    assert!(main_funcs.contains(&"mainExtra".to_string()), "Main should have mainExtra");

    // Worktree should have: alpha, wtExtra (NOT beta, NOT mainExtra)
    let wt_funcs = function_names_in_db(&wt_db);
    assert_eq!(wt_funcs.len(), 2, "Worktree index should have 2 functions");
    assert!(wt_funcs.contains(&"alpha".to_string()), "Worktree should have alpha");
    assert!(wt_funcs.contains(&"wtExtra".to_string()), "Worktree should have wtExtra");
    assert!(!wt_funcs.contains(&"beta".to_string()), "Worktree should NOT have beta (deleted)");
    assert!(!wt_funcs.contains(&"mainExtra".to_string()), "Worktree should NOT have mainExtra");

    // Shared function "alpha" should have identical embeddings in both indices
    let main_alpha_emb = read_embedding_from_db(&main_db, "alpha").unwrap();
    let wt_alpha_emb = read_embedding_from_db(&wt_db, "alpha").unwrap();
    assert_eq!(
        main_alpha_emb, wt_alpha_emb,
        "Shared function 'alpha' should have identical embeddings in both worktrees"
    );
}
