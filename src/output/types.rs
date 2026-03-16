use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub name: String,
    pub path: String,
    pub line: usize,
    pub line_count: usize,
    pub signature: String,
    pub distance: f64,
    pub detection_method: String,
    pub source: String,
    pub signature_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jaccard_similarity: Option<f64>,
}

impl crate::ignore::ignore_file::HasSignatureHash for Candidate {
    fn signature_hash(&self) -> &str {
        &self.signature_hash
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryFunction {
    pub name: String,
    pub file: String,
    pub line: usize,
    pub line_count: usize,
    pub signature: String,
    pub candidates: Vec<Candidate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk_type: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryMeta {
    pub model: String,
    pub indexed_functions: usize,
    pub query_functions: usize,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResult {
    pub query_functions: Vec<QueryFunction>,
    pub warnings: Vec<String>,
    pub meta: QueryMeta,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanMatchEntry {
    pub name: String,
    pub path: String,
    pub line: usize,
    pub line_count: usize,
    pub signature: String,
    pub signature_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanMatch {
    pub a: ScanMatchEntry,
    pub b: ScanMatchEntry,
    pub distance: f64,
    pub similarity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jaccard_similarity: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanMeta {
    pub model: String,
    pub chunks_scanned: usize,
    pub pairs_found: usize,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub matches: Vec<ScanMatch>,
    pub meta: ScanMeta,
}

pub fn similarity_tier(distance: f64) -> &'static str {
    if distance <= 0.01 {
        "identical"
    } else if distance <= 0.05 {
        "nearly identical"
    } else if distance <= 0.12 {
        "very similar"
    } else if distance <= 0.20 {
        "similar"
    } else {
        "weak"
    }
}

#[derive(Debug)]
pub struct StatusResult {
    pub exists: bool,
    pub db_path: String,
    pub size_mb: String,
    pub model: String,
    pub dimensions: String,
    pub indexed_functions: usize,
    pub unembedded: usize,
    pub tracked_files: usize,
    pub last_indexed: String,
    pub exclusions: usize,
    pub stale_exclusions: usize,
}

#[derive(Debug)]
pub struct IndexResult {
    pub files_scanned: usize,
    pub functions_indexed: usize,
    pub added: usize,
    pub modified: usize,
    pub deleted: usize,
    pub model: String,
    pub tier: String,
    pub dimensions: usize,
}
