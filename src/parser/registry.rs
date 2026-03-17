use crate::parser::javascript::JavaScriptSupport;
use crate::parser::language::LanguageSupport;
use crate::parser::tsx::TsxSupport;
use crate::parser::typescript::TypeScriptSupport;
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

static LANGUAGES: LazyLock<Vec<&'static dyn LanguageSupport>> =
    LazyLock::new(|| vec![&TypeScriptSupport, &TsxSupport, &JavaScriptSupport]);

/// Find the language support for a given file path, based on extension.
pub fn language_for_file(path: &str) -> Option<&'static dyn LanguageSupport> {
    let filename = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path);

    for lang in LANGUAGES.iter() {
        if lang.is_excluded_file(filename) {
            continue;
        }
        for ext in lang.file_extensions() {
            if filename.ends_with(&format!(".{ext}")) {
                return Some(*lang);
            }
        }
    }
    None
}

/// Walk `root_dir` and return all files that match any registered language.
pub fn find_source_files(root_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    let walker = WalkBuilder::new(root_dir).standard_filters(true).build();

    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let path_str = path.to_string_lossy();
        if language_for_file(&path_str).is_some() {
            files.push(path.to_path_buf());
        }
    }

    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn language_for_ts_file() {
        assert!(language_for_file("src/app.ts").is_some());
    }

    #[test]
    fn language_for_excluded_ts_file() {
        assert!(language_for_file("types.d.ts").is_none());
        assert!(language_for_file("app.test.ts").is_none());
        assert!(language_for_file("app.spec.ts").is_none());
    }

    #[test]
    fn language_for_unknown_extension() {
        assert!(language_for_file("main.py").is_none());
        assert!(language_for_file("lib.rs").is_none());
    }

    #[test]
    fn find_source_files_filters_correctly() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("app.ts"), "").unwrap();
        fs::write(tmp.path().join("types.d.ts"), "").unwrap();
        fs::write(tmp.path().join("app.test.ts"), "").unwrap();
        fs::write(tmp.path().join("app.spec.ts"), "").unwrap();
        fs::write(tmp.path().join("style.css"), "").unwrap();

        let files = find_source_files(tmp.path());
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("app.ts"));
    }
}
