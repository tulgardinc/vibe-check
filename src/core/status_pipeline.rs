use crate::error::VibecheckError;
use crate::ignore::ignore_file::load_ignore_file;
use crate::ignore::stale_detector::detect_stale_exclusions;
use crate::output::types::StatusResult;
use crate::store::db::{get_meta_value, open_database_no_vec};
use crate::store::file_tracker::get_tracked_files;
use crate::store::index_store::{count_functions, count_functions_without_embeddings};
use crate::util::config::{find_project_root, DB_FILENAME};
use std::fs;
use std::path::Path;

pub struct StatusOptions {
    pub db_path: Option<String>,
}

pub fn run_status(options: StatusOptions) -> Result<StatusResult, VibecheckError> {
    let project_root = find_project_root(Path::new("."));

    let db_path = options
        .db_path
        .unwrap_or_else(|| project_root.join(DB_FILENAME).to_string_lossy().to_string());

    if !Path::new(&db_path).exists() {
        return Ok(StatusResult {
            exists: false,
            db_path,
            size_bytes: 0,
            model: String::new(),
            dimensions: 0,
            indexed_functions: 0,
            unembedded: 0,
            tracked_files: 0,
            last_indexed: String::new(),
            exclusions: 0,
            stale_exclusions: 0,
            file_exclusions: 0,
            file_pair_exclusions: 0,
        });
    }

    let conn = open_database_no_vec(&db_path)?;

    let indexed = count_functions(&conn)?;
    let unembedded = count_functions_without_embeddings(&conn)?;
    let tracked = get_tracked_files(&conn)?.len();

    let model = get_meta_value(&conn, "model_name")?.unwrap_or_default();
    let dimensions: usize = get_meta_value(&conn, "model_dimensions")?
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let last_indexed = get_meta_value(&conn, "last_indexed_at")?.unwrap_or_default();

    let size_bytes = fs::metadata(&db_path)
        .map(|m| m.len())
        .unwrap_or(0);

    let ignore_file = load_ignore_file(&project_root);
    let exclusion_count = ignore_file.exclusions.len();
    let file_exclusion_count = ignore_file.file_exclusions.len();
    let file_pair_exclusion_count = ignore_file.file_pair_exclusions.len();
    let stale = detect_stale_exclusions(&conn, &ignore_file)?;

    Ok(StatusResult {
        exists: true,
        db_path,
        size_bytes,
        model,
        dimensions,
        indexed_functions: indexed,
        unembedded,
        tracked_files: tracked,
        last_indexed,
        exclusions: exclusion_count,
        stale_exclusions: stale.len(),
        file_exclusions: file_exclusion_count,
        file_pair_exclusions: file_pair_exclusion_count,
    })
}
