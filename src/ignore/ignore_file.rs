use crate::ignore::types::{Exclusion, IgnoreFile};
use crate::util::logger;
use std::fs;
use std::path::Path;

const FILENAME: &str = ".vibecheck-ignore.json";
const LEGACY_FILENAME: &str = ".codereuse-ignore.json";

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
    // Check for duplicate (bidirectional)
    let already_exists = ignore_file.exclusions.iter().any(|e| {
        (e.pair.a.signature_hash == exclusion.pair.a.signature_hash
            && e.pair.b.signature_hash == exclusion.pair.b.signature_hash)
            || (e.pair.a.signature_hash == exclusion.pair.b.signature_hash
                && e.pair.b.signature_hash == exclusion.pair.a.signature_hash)
    });

    if already_exists {
        return ignore_file.clone();
    }

    let mut new_file = ignore_file.clone();
    new_file.exclusions.push(exclusion);
    new_file
}

pub fn is_excluded(ignore_file: &IgnoreFile, query_hash: &str, candidate_hash: &str) -> bool {
    ignore_file.exclusions.iter().any(|e| {
        (e.pair.a.signature_hash == query_hash && e.pair.b.signature_hash == candidate_hash)
            || (e.pair.a.signature_hash == candidate_hash
                && e.pair.b.signature_hash == query_hash)
    })
}

pub fn apply_exclusions<T: HasSignatureHash>(
    ignore_file: &IgnoreFile,
    candidates: Vec<T>,
    query_signature_hash: &str,
) -> Vec<T> {
    candidates
        .into_iter()
        .filter(|c| !is_excluded(ignore_file, query_signature_hash, c.signature_hash()))
        .collect()
}

pub trait HasSignatureHash {
    fn signature_hash(&self) -> &str;
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
    fn is_excluded_bidirectional() {
        let ignore = add_exclusion(&IgnoreFile::default(), make_exclusion("aaa", "bbb"));
        assert!(is_excluded(&ignore, "aaa", "bbb"));
        assert!(is_excluded(&ignore, "bbb", "aaa"));
        assert!(!is_excluded(&ignore, "aaa", "ccc"));
    }
}
