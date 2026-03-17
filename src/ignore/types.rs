use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExclusionSide {
    pub path: String,
    pub function: String,
    #[serde(rename = "signatureHash")]
    pub signature_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExclusionPair {
    pub a: ExclusionSide,
    pub b: ExclusionSide,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Exclusion {
    pub reason: String,
    pub added: String,
    pub pair: ExclusionPair,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileExclusion {
    pub pattern: String,
    pub reason: String,
    pub added: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilePairExclusion {
    pub a: String,
    pub b: String,
    pub reason: String,
    pub added: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IgnoreFile {
    pub version: u32,
    pub exclusions: Vec<Exclusion>,
    #[serde(default, rename = "fileExclusions")]
    pub file_exclusions: Vec<FileExclusion>,
    #[serde(default, rename = "filePairExclusions")]
    pub file_pair_exclusions: Vec<FilePairExclusion>,
}

impl Default for IgnoreFile {
    fn default() -> Self {
        Self {
            version: 1,
            exclusions: Vec::new(),
            file_exclusions: Vec::new(),
            file_pair_exclusions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum PairSide {
    A,
    B,
}

#[derive(Debug)]
pub struct StaleWarning {
    pub exclusion_index: usize,
    pub side: PairSide,
    pub function_name: String,
    pub path: String,
    pub reason: String,
}
