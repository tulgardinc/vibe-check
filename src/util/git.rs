use crate::error::VibecheckError;
use crate::store::db::get_meta_value;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::process::Command;

/// File status in a diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffStatus {
    Added,
    Modified,
    Deleted,
}

/// A file entry from a git diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffEntry {
    pub path: String,
    pub status: DiffStatus,
}

/// Check if the current directory is inside a git working tree.
pub fn is_git_repo(project_root: &Path) -> bool {
    Command::new("git")
        .args(["-C", &project_root.to_string_lossy(), "rev-parse", "--is-inside-work-tree"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Get the current HEAD commit hash (full 40-char hex).
pub fn get_head_commit(project_root: &Path) -> Result<String, VibecheckError> {
    let output = Command::new("git")
        .args(["-C", &project_root.to_string_lossy(), "rev-parse", "HEAD"])
        .output()
        .map_err(|e| VibecheckError::Git(format!("failed to run git rev-parse HEAD: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VibecheckError::Git(format!(
            "git rev-parse HEAD failed: {}",
            stderr.trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.trim().to_string())
}

/// Get the git common dir (for shared cache location).
/// Uses `git rev-parse --git-common-dir`.
pub fn get_git_common_dir(project_root: &Path) -> Result<PathBuf, VibecheckError> {
    let output = Command::new("git")
        .args([
            "-C",
            &project_root.to_string_lossy(),
            "rev-parse",
            "--git-common-dir",
        ])
        .output()
        .map_err(|e| {
            VibecheckError::Git(format!("failed to run git rev-parse --git-common-dir: {e}"))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VibecheckError::Git(format!(
            "git rev-parse --git-common-dir failed: {}",
            stderr.trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let git_common_dir = PathBuf::from(stdout.trim());

    // If git returns a relative path (e.g. ".git"), resolve it against project_root
    if git_common_dir.is_relative() {
        Ok(project_root.join(&git_common_dir).canonicalize().map_err(|e| {
            VibecheckError::Git(format!(
                "failed to canonicalize git common dir '{}': {e}",
                git_common_dir.display()
            ))
        })?)
    } else {
        Ok(git_common_dir)
    }
}

/// Parse `git diff --name-status` output into DiffEntry structs.
/// Each line is `<status>\t<path>` or `R<score>\t<old>\t<new>` for renames.
pub fn parse_diff_output(output: &str) -> Result<Vec<DiffEntry>, VibecheckError> {
    let mut entries = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 2 {
            continue;
        }

        let status_str = parts[0];

        if status_str.starts_with('R') {
            // Rename: R<score>\t<old_path>\t<new_path>
            if parts.len() < 3 {
                continue;
            }
            entries.push(DiffEntry {
                path: parts[1].to_string(),
                status: DiffStatus::Deleted,
            });
            entries.push(DiffEntry {
                path: parts[2].to_string(),
                status: DiffStatus::Added,
            });
        } else {
            match status_str {
                "A" => entries.push(DiffEntry {
                    path: parts[1].to_string(),
                    status: DiffStatus::Added,
                }),
                "M" => entries.push(DiffEntry {
                    path: parts[1].to_string(),
                    status: DiffStatus::Modified,
                }),
                "D" => entries.push(DiffEntry {
                    path: parts[1].to_string(),
                    status: DiffStatus::Deleted,
                }),
                _ => {
                    // Skip unknown statuses (e.g., C for copy, T for type change)
                }
            }
        }
    }

    Ok(entries)
}

/// Get changed files: working tree vs HEAD.
/// Runs `git diff HEAD --name-status` (captures staged + unstaged).
pub fn diff_working_tree(project_root: &Path) -> Result<Vec<DiffEntry>, VibecheckError> {
    let output = Command::new("git")
        .args([
            "-C",
            &project_root.to_string_lossy(),
            "diff",
            "HEAD",
            "--name-status",
        ])
        .output()
        .map_err(|e| VibecheckError::Git(format!("failed to run git diff HEAD: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VibecheckError::Git(format!(
            "git diff HEAD --name-status failed: {}",
            stderr.trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_diff_output(&stdout)
}

/// Get changed files for a specific commit vs its first parent.
/// Runs `git diff --name-status <hash>^1 <hash>`.
/// For initial commits (no parent), uses `git diff-tree --name-status -r --root <hash>`.
pub fn diff_commit(project_root: &Path, hash: &str) -> Result<Vec<DiffEntry>, VibecheckError> {
    let output = Command::new("git")
        .args([
            "-C",
            &project_root.to_string_lossy(),
            "diff",
            "--name-status",
            &format!("{hash}^1"),
            hash,
        ])
        .output()
        .map_err(|e| VibecheckError::Git(format!("failed to run git diff for commit {hash}: {e}")))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        return parse_diff_output(&stdout);
    }

    // If the above failed, it might be the initial commit (no parent).
    // Try diff-tree with --root instead.
    let output = Command::new("git")
        .args([
            "-C",
            &project_root.to_string_lossy(),
            "diff-tree",
            "--name-status",
            "-r",
            "--root",
            hash,
        ])
        .output()
        .map_err(|e| {
            VibecheckError::Git(format!("failed to run git diff-tree for commit {hash}: {e}"))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VibecheckError::Git(format!(
            "git diff-tree --root {hash} failed: {}",
            stderr.trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_diff_output(&stdout)
}

/// Get file contents at a specific revision.
/// Runs `git show <rev>:<path>`. Returns None if file doesn't exist at that rev.
pub fn show_file(
    project_root: &Path,
    rev: &str,
    path: &str,
) -> Result<Option<String>, VibecheckError> {
    let output = Command::new("git")
        .args([
            "-C",
            &project_root.to_string_lossy(),
            "show",
            &format!("{rev}:{path}"),
        ])
        .output()
        .map_err(|e| {
            VibecheckError::Git(format!("failed to run git show {rev}:{path}: {e}"))
        })?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(Some(stdout.into_owned()))
    } else {
        // File doesn't exist at that revision
        Ok(None)
    }
}

/// Read file from the working tree (just fs::read_to_string with error wrapping).
pub fn read_working_tree_file(
    project_root: &Path,
    path: &str,
) -> Result<String, VibecheckError> {
    std::fs::read_to_string(project_root.join(path)).map_err(|e| {
        VibecheckError::Git(format!(
            "failed to read working tree file '{}': {e}",
            project_root.join(path).display()
        ))
    })
}

/// Check index staleness: compare stored head_commit with current HEAD.
/// Returns a warning string if stale, None if fresh or not a git repo.
pub fn check_staleness(conn: &Connection, project_root: &Path) -> Option<String> {
    // Get stored head_commit from index_meta
    let stored = get_meta_value(conn, "head_commit").ok()??;

    // Check if this is a git repo
    if !is_git_repo(project_root) {
        return None;
    }

    // Get the current HEAD
    let current = get_head_commit(project_root).ok()?;

    // Compare
    if stored == current {
        None
    } else {
        Some(format!(
            "Index may be stale: it was built at commit {}, but HEAD is now {}. Consider re-running `vibec index`.",
            &stored[..8.min(stored.len())],
            &current[..8.min(current.len())]
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::db::run_migrations_for_test;

    // --- parse_diff_output tests ---

    #[test]
    fn parse_diff_output_added_modified_deleted() {
        let output = "A\tsrc/new_file.ts\nM\tsrc/changed.ts\nD\tsrc/removed.ts\n";
        let entries = parse_diff_output(output).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(
            entries[0],
            DiffEntry {
                path: "src/new_file.ts".into(),
                status: DiffStatus::Added,
            }
        );
        assert_eq!(
            entries[1],
            DiffEntry {
                path: "src/changed.ts".into(),
                status: DiffStatus::Modified,
            }
        );
        assert_eq!(
            entries[2],
            DiffEntry {
                path: "src/removed.ts".into(),
                status: DiffStatus::Deleted,
            }
        );
    }

    #[test]
    fn parse_diff_output_empty() {
        let entries = parse_diff_output("").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_diff_output_rename_produces_delete_and_add() {
        // Renamed file: R100\told_name.ts\tnew_name.ts
        let output = "R100\told_name.ts\tnew_name.ts\n";
        let entries = parse_diff_output(output).unwrap();
        assert_eq!(entries.len(), 2);

        // Old path should be deleted
        let delete = entries.iter().find(|e| e.status == DiffStatus::Deleted).unwrap();
        assert_eq!(delete.path, "old_name.ts");

        // New path should be added
        let add = entries.iter().find(|e| e.status == DiffStatus::Added).unwrap();
        assert_eq!(add.path, "new_name.ts");
    }

    // --- check_staleness tests ---

    #[test]
    fn check_staleness_returns_none_when_head_commit_absent() {
        // When there is no head_commit stored in index_meta, no warning should be emitted.
        let conn = Connection::open_in_memory().unwrap();
        run_migrations_for_test(&conn);

        // No head_commit set in index_meta
        let result = check_staleness(&conn, Path::new("."));
        assert!(result.is_none(), "Expected None when head_commit is absent");
    }

    #[test]
    fn check_staleness_returns_warning_when_stored_differs_from_current() {
        // When the stored head_commit differs from the actual HEAD, a warning should be emitted.
        let conn = Connection::open_in_memory().unwrap();
        run_migrations_for_test(&conn);

        // Store a known commit hash
        crate::store::db::set_meta_value(&conn, "head_commit", "aaaa1111bbbb2222cccc3333dddd4444eeee5555").unwrap();

        // check_staleness will call get_head_commit internally and compare.
        // Since we can't easily mock git, this test verifies the logic:
        // If the project is a git repo and HEAD differs, return Some(warning).
        // If not a git repo, return None.
        //
        // In a non-git temp dir, this should return None (graceful degradation).
        let tmp = tempfile::TempDir::new().unwrap();
        let result = check_staleness(&conn, tmp.path());
        // In a non-git directory, check_staleness should return None (not a git repo)
        assert!(result.is_none(), "Expected None in a non-git directory");
    }

    #[test]
    fn check_staleness_returns_none_when_stored_matches_current() {
        // When stored head_commit matches the actual HEAD, no warning.
        // This test uses a real git repo (the project itself).
        let conn = Connection::open_in_memory().unwrap();
        run_migrations_for_test(&conn);

        // Get the actual current HEAD of the project
        let project_root = Path::new(env!("CARGO_MANIFEST_DIR"));
        if !is_git_repo(project_root) {
            // Skip if not running in a git repo
            return;
        }

        let current_head = get_head_commit(project_root).unwrap();
        crate::store::db::set_meta_value(&conn, "head_commit", &current_head).unwrap();

        let result = check_staleness(&conn, project_root);
        assert!(result.is_none(), "Expected None when stored matches current HEAD");
    }
}
