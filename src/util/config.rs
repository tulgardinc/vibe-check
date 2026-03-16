use crate::error::VibecheckError;
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

pub const DB_FILENAME: &str = ".vibecheck.db";

pub fn resolve_project_root(provided: Option<&str>) -> PathBuf {
    match provided {
        Some(p) => {
            let path = Path::new(p);
            path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
        }
        None => find_project_root(Path::new(".")),
    }
}

pub fn find_project_root(start_dir: &Path) -> PathBuf {
    let mut dir = start_dir.canonicalize().unwrap_or_else(|_| start_dir.to_path_buf());

    loop {
        if dir.join(".git").exists() || dir.join("package.json").exists() {
            return dir;
        }

        match dir.parent() {
            Some(parent) if parent != dir => dir = parent.to_path_buf(),
            _ => return start_dir.to_path_buf(),
        }
    }
}

pub fn resolve_db_path(project_root: &Path, override_path: Option<&str>) -> PathBuf {
    match override_path {
        Some(p) => PathBuf::from(p).canonicalize().unwrap_or_else(|_| PathBuf::from(p)),
        None => project_root.join(DB_FILENAME),
    }
}

/// Resolve DB path and verify it exists. Used by query, scan, and status pipelines.
pub fn resolve_existing_db(
    project_root: &Path,
    override_path: Option<&str>,
) -> Result<String, VibecheckError> {
    let db_path = match override_path {
        Some(p) => p.to_string(),
        None => project_root
            .join(DB_FILENAME)
            .to_string_lossy()
            .to_string(),
    };
    if !Path::new(&db_path).exists() {
        return Err(VibecheckError::Config(
            "No index found. Run `vibec index` first.".into(),
        ));
    }
    Ok(db_path)
}

pub fn find_typescript_files(root_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    let walker = WalkBuilder::new(root_dir)
        .standard_filters(true)
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };

        if !name.ends_with(".ts") {
            continue;
        }

        if name.ends_with(".d.ts") || name.ends_with(".test.ts") || name.ends_with(".spec.ts") {
            continue;
        }

        files.push(path.to_path_buf());
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
    fn find_project_root_with_git() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join(".git")).unwrap();
        let sub = tmp.path().join("a/b/c");
        fs::create_dir_all(&sub).unwrap();

        let root = find_project_root(&sub);
        assert_eq!(root, tmp.path().canonicalize().unwrap());
    }

    #[test]
    fn find_project_root_with_package_json() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("package.json"), "{}").unwrap();
        let sub = tmp.path().join("src");
        fs::create_dir_all(&sub).unwrap();

        let root = find_project_root(&sub);
        assert_eq!(root, tmp.path().canonicalize().unwrap());
    }

    #[test]
    fn resolve_db_path_default() {
        let root = PathBuf::from("/some/project");
        let db = resolve_db_path(&root, None);
        assert_eq!(db, PathBuf::from("/some/project/.vibecheck.db"));
    }

    #[test]
    fn find_ts_files_filters_correctly() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("app.ts"), "").unwrap();
        fs::write(tmp.path().join("types.d.ts"), "").unwrap();
        fs::write(tmp.path().join("app.test.ts"), "").unwrap();
        fs::write(tmp.path().join("app.spec.ts"), "").unwrap();
        fs::write(tmp.path().join("style.css"), "").unwrap();

        let files = find_typescript_files(tmp.path());
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("app.ts"));
    }
}
