use crate::error::VibecheckError;
use std::path::{Path, PathBuf};

pub use crate::parser::registry::find_source_files;

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

/// Resolve the shared embedding cache path by finding the git common dir
/// and appending `vibecheck-cache.db`. Returns None if not a git repo.
pub fn resolve_cache_path(project_root: &Path) -> Option<PathBuf> {
    let git_common_dir = crate::util::git::get_git_common_dir(project_root).ok()?;
    Some(git_common_dir.join("vibecheck-cache.db"))
}

/// Resolve DB path and verify it exists. Used by query, scan, and status pipelines.
pub fn resolve_existing_db(
    project_root: &Path,
    override_path: Option<&str>,
) -> Result<String, VibecheckError> {
    let db_path = resolve_db_path(project_root, override_path);
    if !db_path.exists() {
        return Err(VibecheckError::Config(
            "No index found. Run `vibec index` first.".into(),
        ));
    }
    Ok(db_path.to_string_lossy().to_string())
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
}
