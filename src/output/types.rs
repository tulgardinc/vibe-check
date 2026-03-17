use crate::parser::types::ChunkType;
use crate::store::index_store::SlimFunction;
use crate::store::types::StoredFunction;
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
    pub chunk_type: Option<ChunkType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jaccard_similarity: Option<f64>,
}

impl Candidate {
    /// Build a Candidate from a StoredFunction and its KNN distance.
    pub fn from_stored(func: &StoredFunction, distance: f64) -> Self {
        Self {
            name: func.function_name.clone(),
            path: func.file_path.clone(),
            line: func.start_line,
            line_count: func.line_count(),
            signature: func.signature.clone(),
            distance,
            detection_method: "embedding".into(),
            source: func.source_text.clone(),
            signature_hash: func.signature_hash.clone(),
            chunk_type: Some(func.chunk_type),
            context: func.context.clone(),
            jaccard_similarity: None,
        }
    }
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
    pub chunk_type: Option<ChunkType>,
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
    pub chunk_type: Option<ChunkType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

impl From<&StoredFunction> for ScanMatchEntry {
    fn from(f: &StoredFunction) -> Self {
        Self {
            name: f.function_name.clone(),
            path: f.file_path.clone(),
            line: f.start_line,
            line_count: f.line_count(),
            signature: f.signature.clone(),
            signature_hash: f.signature_hash.clone(),
            chunk_type: Some(f.chunk_type),
            context: f.context.clone(),
        }
    }
}

impl From<&SlimFunction> for ScanMatchEntry {
    fn from(f: &SlimFunction) -> Self {
        Self {
            name: f.function_name.clone(),
            path: f.file_path.clone(),
            line: f.start_line,
            line_count: f.line_count(),
            signature: f.signature.clone(),
            signature_hash: f.signature_hash.clone(),
            chunk_type: Some(f.chunk_type),
            context: f.context.clone(),
        }
    }
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

pub const SIMILARITY_TIERS: &[&str] = &[
    "identical",
    "nearly identical",
    "very similar",
    "similar",
    "weak",
];

const TIER_IDENTICAL: f64 = 0.01;
const TIER_NEARLY_IDENTICAL: f64 = 0.05;
const TIER_VERY_SIMILAR: f64 = 0.12;
const TIER_SIMILAR: f64 = 0.20;

pub fn similarity_tier(distance: f64) -> &'static str {
    if distance <= TIER_IDENTICAL {
        "identical"
    } else if distance <= TIER_NEARLY_IDENTICAL {
        "nearly identical"
    } else if distance <= TIER_VERY_SIMILAR {
        "very similar"
    } else if distance <= TIER_SIMILAR {
        "similar"
    } else {
        "weak"
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusResult {
    pub exists: bool,
    pub db_path: String,
    pub size_bytes: u64,
    pub model: String,
    pub dimensions: usize,
    pub indexed_functions: usize,
    pub unembedded: usize,
    pub tracked_files: usize,
    pub last_indexed: String,
    pub exclusions: usize,
    pub stale_exclusions: usize,
    pub file_exclusions: usize,
    pub file_pair_exclusions: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
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

pub fn format_similarity(distance: f64) -> String {
    format!("{:.2}", 1.0 - distance)
}

pub fn format_jaccard_suffix(jaccard: Option<f64>) -> String {
    match jaccard {
        Some(j) => format!(", jaccard: {j:.2}"),
        None => String::new(),
    }
}

pub fn format_display_name(
    name: &str,
    chunk_type: Option<ChunkType>,
    context: Option<&str>,
) -> String {
    match (chunk_type, context) {
        (Some(ChunkType::Block), Some(ctx)) => format!("{name} in {ctx}"),
        _ => name.to_string(),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DryRunResult {
    pub files: Vec<String>,
    pub file_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExcludeResult {
    pub message: String,
    pub total_exclusions: usize,
    pub total_file_exclusions: usize,
    pub total_file_pair_exclusions: usize,
}
