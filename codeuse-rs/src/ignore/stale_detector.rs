use crate::ignore::types::{IgnoreFile, StaleWarning};
use rusqlite::Connection;

use crate::store::index_store::get_function_by_signature_hash;

pub fn detect_stale_exclusions(
    conn: &Connection,
    ignore_file: &IgnoreFile,
) -> Vec<StaleWarning> {
    let mut warnings = Vec::new();

    for (i, exclusion) in ignore_file.exclusions.iter().enumerate() {
        for (side_char, side) in [('a', &exclusion.pair.a), ('b', &exclusion.pair.b)] {
            if get_function_by_signature_hash(conn, &side.signature_hash).is_none() {
                warnings.push(StaleWarning {
                    exclusion_index: i,
                    side: side_char,
                    function_name: side.function.clone(),
                    path: side.path.clone(),
                    reason: format!(
                        "Function \"{}\" in {} no longer exists or its signature has changed",
                        side.function, side.path
                    ),
                });
            }
        }
    }

    warnings
}
