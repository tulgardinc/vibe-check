use crate::error::CodeuseError;
use crate::ignore::ignore_file::load_ignore_file;
use crate::ignore::stale_detector::detect_stale_exclusions;
use crate::output::types::StatusResult;
use crate::store::db::{get_meta_value, open_database_no_vec};
use crate::store::file_tracker::get_tracked_files;
use crate::store::index_store::{count_functions, get_functions_without_embeddings};
use crate::util::config::find_project_root;
use std::fs;
use std::path::Path;

pub struct StatusOptions {
    pub db_path: Option<String>,
}

pub fn run_status(options: StatusOptions) -> Result<StatusResult, CodeuseError> {
    let project_root = find_project_root(Path::new("."));

    let db_path = options
        .db_path
        .clone()
        .unwrap_or_else(|| project_root.join(".vibecheck.db").to_string_lossy().to_string());

    if !Path::new(&db_path).exists() {
        return Ok(StatusResult {
            exists: false,
            db_path,
            size_mb: "0.0".into(),
            model: String::new(),
            dimensions: String::new(),
            indexed_functions: 0,
            unembedded: 0,
            tracked_files: 0,
            last_indexed: String::new(),
            exclusions: 0,
            stale_exclusions: 0,
        });
    }

    let conn = open_database_no_vec(&db_path)?;

    let indexed = count_functions(&conn)?;
    let unembedded = get_functions_without_embeddings(&conn)?.len();
    let tracked = get_tracked_files(&conn)?.len();

    let model = get_meta_value(&conn, "model_name").unwrap_or_default();
    let dimensions = get_meta_value(&conn, "model_dimensions").unwrap_or_default();
    let last_indexed = get_meta_value(&conn, "last_indexed_at").unwrap_or_default();

    let size_mb = fs::metadata(&db_path)
        .map(|m| format!("{:.1}", m.len() as f64 / 1024.0 / 1024.0))
        .unwrap_or_else(|_| "0.0".into());

    let ignore_file = load_ignore_file(&project_root);
    let exclusion_count = ignore_file.exclusions.len();
    let stale = detect_stale_exclusions(&conn, &ignore_file);

    Ok(StatusResult {
        exists: true,
        db_path,
        size_mb,
        model,
        dimensions,
        indexed_functions: indexed,
        unembedded,
        tracked_files: tracked,
        last_indexed,
        exclusions: exclusion_count,
        stale_exclusions: stale.len(),
    })
}
