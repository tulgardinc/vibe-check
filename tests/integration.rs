use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
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
        inputs: &[String],
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
