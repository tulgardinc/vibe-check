use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::TempDir;
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
