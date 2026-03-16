use crate::ignore::types::{IgnoreFile, PairSide, StaleWarning};
use crate::store::index_store::get_all_signature_hashes;
use rusqlite::Connection;

pub fn detect_stale_exclusions(
    conn: &Connection,
    ignore_file: &IgnoreFile,
) -> Vec<StaleWarning> {
    let known_hashes = match get_all_signature_hashes(conn) {
        Ok(h) => h,
        Err(_) => return vec![],
    };

    let mut warnings = Vec::new();

    for (i, exclusion) in ignore_file.exclusions.iter().enumerate() {
        for (pair_side, side) in [(PairSide::A, &exclusion.pair.a), (PairSide::B, &exclusion.pair.b)] {
            if !known_hashes.contains(&side.signature_hash) {
                warnings.push(StaleWarning {
                    exclusion_index: i,
                    side: pair_side,
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
