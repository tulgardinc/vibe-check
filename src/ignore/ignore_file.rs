use crate::ignore::types::{Exclusion, FileExclusion, FilePairExclusion, IgnoreFile};
use crate::util::logger;
use ignore::gitignore::GitignoreBuilder;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const FILENAME: &str = ".vibecheck-ignore.json";
const LEGACY_FILENAME: &str = ".codereuse-ignore.json";

/// Precomputed set of excluded pairs for O(1) lookup.
/// Pairs are stored as (min_hash, max_hash) for bidirectional matching.
pub struct ExclusionIndex {
    pairs: HashSet<(String, String)>,
}

impl ExclusionIndex {
    pub fn new(ignore_file: &IgnoreFile) -> Self {
        let mut pairs = HashSet::new();
        for e in &ignore_file.exclusions {
            let (a, b) = normalize_pair(&e.pair.a.signature_hash, &e.pair.b.signature_hash);
            pairs.insert((a, b));
        }
        Self { pairs }
    }

    pub fn is_excluded(&self, hash_a: &str, hash_b: &str) -> bool {
        let (a, b) = normalize_pair(hash_a, hash_b);
        self.pairs.contains(&(a, b))
    }
}

fn normalize_pair(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

pub fn load_ignore_file(project_root: &Path) -> IgnoreFile {
    let file_path = project_root.join(FILENAME);

    // Fall back to legacy filename if the new one doesn't exist
    let (file_path, display_name) = if file_path.exists() {
        (file_path, FILENAME)
    } else {
        let legacy_path = project_root.join(LEGACY_FILENAME);
        if legacy_path.exists() {
            (legacy_path, LEGACY_FILENAME)
        } else {
            return IgnoreFile::default();
        }
    };

    let content = match fs::read_to_string(&file_path) {
        Ok(c) => c,
        Err(e) => {
            logger::warn(&format!("Failed to read {display_name}: {e} — treating as empty"));
            return IgnoreFile::default();
        }
    };

    match serde_json::from_str::<IgnoreFile>(&content) {
        Ok(parsed) => {
            if parsed.version != 1 {
                logger::warn(&format!(
                    "{display_name} has unexpected version {} — treating as empty",
                    parsed.version
                ));
                return IgnoreFile::default();
            }
            parsed
        }
        Err(e) => {
            logger::warn(&format!(
                "Failed to parse {display_name}: {e} — treating as empty"
            ));
            IgnoreFile::default()
        }
    }
}

pub fn save_ignore_file(project_root: &Path, ignore_file: &IgnoreFile) {
    let file_path = project_root.join(FILENAME);
    let json = serde_json::to_string_pretty(ignore_file).unwrap_or_default();
    if let Err(e) = fs::write(&file_path, format!("{json}\n")) {
        logger::warn(&format!("Failed to write {FILENAME}: {e}"));
    }
}

pub fn add_exclusion(ignore_file: &IgnoreFile, exclusion: Exclusion) -> IgnoreFile {
    let index = ExclusionIndex::new(ignore_file);
    if index.is_excluded(&exclusion.pair.a.signature_hash, &exclusion.pair.b.signature_hash) {
        return ignore_file.clone();
    }

    let mut new_file = ignore_file.clone();
    new_file.exclusions.push(exclusion);
    new_file
}

pub fn apply_exclusions<T: HasSignatureHash>(
    index: &ExclusionIndex,
    candidates: Vec<T>,
    query_signature_hash: &str,
) -> Vec<T> {
    candidates
        .into_iter()
        .filter(|c| !index.is_excluded(query_signature_hash, c.signature_hash()))
        .collect()
}

pub trait HasSignatureHash {
    fn signature_hash(&self) -> &str;
}

pub trait HasFilePath {
    fn file_path(&self) -> &str;
}

/// Matches file paths against glob patterns from file exclusions.
/// Uses gitignore-style matching (patterns relative to project root).
pub struct FileExclusionMatcher {
    gitignore: ignore::gitignore::Gitignore,
    project_root: PathBuf,
}

impl FileExclusionMatcher {
    pub fn new(exclusions: &[FileExclusion], project_root: &Path) -> Self {
        let mut builder = GitignoreBuilder::new(project_root);
        for e in exclusions {
            builder.add_line(None, &e.pattern).ok();
        }
        Self {
            gitignore: builder.build().unwrap_or_else(|_| {
                GitignoreBuilder::new(project_root).build().unwrap()
            }),
            project_root: project_root.to_path_buf(),
        }
    }

    pub fn is_excluded(&self, path: &Path) -> bool {
        let relative = path.strip_prefix(&self.project_root).unwrap_or(path);
        self.gitignore
            .matched_path_or_any_parents(relative, false)
            .is_ignore()
    }

    pub fn is_empty(&self) -> bool {
        self.gitignore.num_ignores() == 0
    }
}

/// Precomputed set of excluded file pairs for O(1) lookup.
/// Pairs are stored as (min_path, max_path) for bidirectional matching.
pub struct FilePairExclusionIndex {
    pairs: HashSet<(String, String)>,
    project_root: PathBuf,
}

impl FilePairExclusionIndex {
    pub fn new(exclusions: &[FilePairExclusion], project_root: &Path) -> Self {
        let mut pairs = HashSet::new();
        for e in exclusions {
            let (a, b) = normalize_pair(&e.a, &e.b);
            pairs.insert((a, b));
        }
        Self {
            pairs,
            project_root: project_root.to_path_buf(),
        }
    }

    pub fn is_excluded(&self, file_a: &str, file_b: &str) -> bool {
        let norm_a = self.normalize_path(file_a);
        let norm_b = self.normalize_path(file_b);
        let (a, b) = normalize_pair(&norm_a, &norm_b);
        self.pairs.contains(&(a, b))
    }

    fn normalize_path(&self, path: &str) -> String {
        Path::new(path)
            .strip_prefix(&self.project_root)
            .unwrap_or(Path::new(path))
            .to_string_lossy()
            .to_string()
    }
}

pub fn add_file_exclusion(ignore_file: &IgnoreFile, exclusion: FileExclusion) -> IgnoreFile {
    if ignore_file
        .file_exclusions
        .iter()
        .any(|e| e.pattern == exclusion.pattern)
    {
        return ignore_file.clone();
    }
    let mut new_file = ignore_file.clone();
    new_file.file_exclusions.push(exclusion);
    new_file
}

pub fn add_file_pair_exclusion(
    ignore_file: &IgnoreFile,
    exclusion: FilePairExclusion,
) -> IgnoreFile {
    let (norm_a, norm_b) = normalize_pair(&exclusion.a, &exclusion.b);
    let already_exists = ignore_file.file_pair_exclusions.iter().any(|e| {
        let (ea, eb) = normalize_pair(&e.a, &e.b);
        ea == norm_a && eb == norm_b
    });
    if already_exists {
        return ignore_file.clone();
    }
    let mut new_file = ignore_file.clone();
    new_file.file_pair_exclusions.push(exclusion);
    new_file
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ignore::types::{ExclusionPair, ExclusionSide};
    use tempfile::TempDir;

    fn make_exclusion(hash_a: &str, hash_b: &str) -> Exclusion {
        Exclusion {
            reason: "test".into(),
            added: "2024-01-01".into(),
            pair: ExclusionPair {
                a: ExclusionSide {
                    path: "a.ts".into(),
                    function: "funcA".into(),
                    signature_hash: hash_a.into(),
                },
                b: ExclusionSide {
                    path: "b.ts".into(),
                    function: "funcB".into(),
                    signature_hash: hash_b.into(),
                },
            },
        }
    }

    #[test]
    fn load_empty_returns_default() {
        let tmp = TempDir::new().unwrap();
        let result = load_ignore_file(tmp.path());
        assert_eq!(result.version, 1);
        assert!(result.exclusions.is_empty());
    }

    #[test]
    fn round_trip_save_load() {
        let tmp = TempDir::new().unwrap();
        let mut ignore = IgnoreFile::default();
        ignore.exclusions.push(make_exclusion("aaa", "bbb"));
        save_ignore_file(tmp.path(), &ignore);

        let loaded = load_ignore_file(tmp.path());
        assert_eq!(loaded.exclusions.len(), 1);
        assert_eq!(loaded.exclusions[0].pair.a.signature_hash, "aaa");
    }

    #[test]
    fn add_exclusion_deduplicates() {
        let ignore = IgnoreFile::default();
        let e1 = make_exclusion("aaa", "bbb");
        let updated = add_exclusion(&ignore, e1);
        assert_eq!(updated.exclusions.len(), 1);

        // Same pair again
        let e2 = make_exclusion("aaa", "bbb");
        let updated = add_exclusion(&updated, e2);
        assert_eq!(updated.exclusions.len(), 1);

        // Reversed pair
        let e3 = make_exclusion("bbb", "aaa");
        let updated = add_exclusion(&updated, e3);
        assert_eq!(updated.exclusions.len(), 1);

        // Different pair
        let e4 = make_exclusion("ccc", "ddd");
        let updated = add_exclusion(&updated, e4);
        assert_eq!(updated.exclusions.len(), 2);
    }

    #[test]
    fn exclusion_index_bidirectional() {
        let ignore = add_exclusion(&IgnoreFile::default(), make_exclusion("aaa", "bbb"));
        let index = ExclusionIndex::new(&ignore);
        assert!(index.is_excluded("aaa", "bbb"));
        assert!(index.is_excluded("bbb", "aaa"));
        assert!(!index.is_excluded("aaa", "ccc"));
    }

    #[test]
    fn file_exclusion_matcher_glob() {
        let tmp = TempDir::new().unwrap();
        let exclusions = vec![FileExclusion {
            pattern: "src/generated/**".into(),
            reason: "auto-generated".into(),
            added: "2024-01-01".into(),
        }];
        let matcher = FileExclusionMatcher::new(&exclusions, tmp.path());
        assert!(matcher.is_excluded(&tmp.path().join("src/generated/foo.ts")));
        assert!(!matcher.is_excluded(&tmp.path().join("src/real/bar.ts")));
    }

    #[test]
    fn file_exclusion_matcher_exact() {
        let tmp = TempDir::new().unwrap();
        let exclusions = vec![FileExclusion {
            pattern: "src/old.ts".into(),
            reason: "deprecated".into(),
            added: "2024-01-01".into(),
        }];
        let matcher = FileExclusionMatcher::new(&exclusions, tmp.path());
        assert!(matcher.is_excluded(&tmp.path().join("src/old.ts")));
        assert!(!matcher.is_excluded(&tmp.path().join("src/new.ts")));
    }

    #[test]
    fn file_exclusion_matcher_empty() {
        let tmp = TempDir::new().unwrap();
        let matcher = FileExclusionMatcher::new(&[], tmp.path());
        assert!(matcher.is_empty());
        assert!(!matcher.is_excluded(&tmp.path().join("anything.ts")));
    }

    #[test]
    fn add_file_exclusion_deduplicates() {
        let ignore = IgnoreFile::default();
        let e1 = FileExclusion {
            pattern: "src/gen/**".into(),
            reason: "test".into(),
            added: "2024-01-01".into(),
        };
        let updated = add_file_exclusion(&ignore, e1);
        assert_eq!(updated.file_exclusions.len(), 1);

        let e2 = FileExclusion {
            pattern: "src/gen/**".into(),
            reason: "different reason".into(),
            added: "2024-02-01".into(),
        };
        let updated = add_file_exclusion(&updated, e2);
        assert_eq!(updated.file_exclusions.len(), 1);

        let e3 = FileExclusion {
            pattern: "src/other/**".into(),
            reason: "test".into(),
            added: "2024-01-01".into(),
        };
        let updated = add_file_exclusion(&updated, e3);
        assert_eq!(updated.file_exclusions.len(), 2);
    }

    #[test]
    fn file_pair_exclusion_bidirectional() {
        let tmp = TempDir::new().unwrap();
        let exclusions = vec![FilePairExclusion {
            a: "src/a.ts".into(),
            b: "src/b.ts".into(),
            reason: "test".into(),
            added: "2024-01-01".into(),
        }];
        let index = FilePairExclusionIndex::new(&exclusions, tmp.path());
        assert!(index.is_excluded("src/a.ts", "src/b.ts"));
        assert!(index.is_excluded("src/b.ts", "src/a.ts"));
        assert!(!index.is_excluded("src/a.ts", "src/c.ts"));
    }

    #[test]
    fn file_pair_exclusion_normalizes_absolute_paths() {
        let tmp = TempDir::new().unwrap();
        let exclusions = vec![FilePairExclusion {
            a: "src/a.ts".into(),
            b: "src/b.ts".into(),
            reason: "test".into(),
            added: "2024-01-01".into(),
        }];
        let index = FilePairExclusionIndex::new(&exclusions, tmp.path());
        let abs_a = tmp.path().join("src/a.ts").to_string_lossy().to_string();
        let abs_b = tmp.path().join("src/b.ts").to_string_lossy().to_string();
        assert!(index.is_excluded(&abs_a, &abs_b));
    }

    #[test]
    fn add_file_pair_exclusion_deduplicates() {
        let ignore = IgnoreFile::default();
        let e1 = FilePairExclusion {
            a: "src/a.ts".into(),
            b: "src/b.ts".into(),
            reason: "test".into(),
            added: "2024-01-01".into(),
        };
        let updated = add_file_pair_exclusion(&ignore, e1);
        assert_eq!(updated.file_pair_exclusions.len(), 1);

        // Same pair
        let e2 = FilePairExclusion {
            a: "src/a.ts".into(),
            b: "src/b.ts".into(),
            reason: "different".into(),
            added: "2024-02-01".into(),
        };
        let updated = add_file_pair_exclusion(&updated, e2);
        assert_eq!(updated.file_pair_exclusions.len(), 1);

        // Reversed pair
        let e3 = FilePairExclusion {
            a: "src/b.ts".into(),
            b: "src/a.ts".into(),
            reason: "reversed".into(),
            added: "2024-03-01".into(),
        };
        let updated = add_file_pair_exclusion(&updated, e3);
        assert_eq!(updated.file_pair_exclusions.len(), 1);

        // Different pair
        let e4 = FilePairExclusion {
            a: "src/c.ts".into(),
            b: "src/d.ts".into(),
            reason: "new".into(),
            added: "2024-04-01".into(),
        };
        let updated = add_file_pair_exclusion(&updated, e4);
        assert_eq!(updated.file_pair_exclusions.len(), 2);
    }

    #[test]
    fn round_trip_with_file_exclusions() {
        let tmp = TempDir::new().unwrap();
        let mut ignore = IgnoreFile::default();
        ignore.file_exclusions.push(FileExclusion {
            pattern: "src/gen/**".into(),
            reason: "generated".into(),
            added: "2024-01-01".into(),
        });
        ignore.file_pair_exclusions.push(FilePairExclusion {
            a: "src/a.ts".into(),
            b: "src/b.ts".into(),
            reason: "fork".into(),
            added: "2024-01-01".into(),
        });
        save_ignore_file(tmp.path(), &ignore);

        let loaded = load_ignore_file(tmp.path());
        assert_eq!(loaded.file_exclusions.len(), 1);
        assert_eq!(loaded.file_exclusions[0].pattern, "src/gen/**");
        assert_eq!(loaded.file_pair_exclusions.len(), 1);
        assert_eq!(loaded.file_pair_exclusions[0].a, "src/a.ts");
    }

    #[test]
    fn backward_compat_no_new_fields() {
        let tmp = TempDir::new().unwrap();
        let json = r#"{"version": 1, "exclusions": []}"#;
        fs::write(tmp.path().join(".vibecheck-ignore.json"), json).unwrap();

        let loaded = load_ignore_file(tmp.path());
        assert_eq!(loaded.version, 1);
        assert!(loaded.file_exclusions.is_empty());
        assert!(loaded.file_pair_exclusions.is_empty());
    }
}
