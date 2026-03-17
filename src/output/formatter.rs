use crate::output::types::{IndexResult, QueryResult, StatusResult, format_display_name, format_jaccard_suffix, format_similarity};

pub fn format_json(result: &QueryResult) -> String {
    serde_json::to_string_pretty(result).unwrap_or_else(|e| {
        eprintln!("Warning: failed to serialize query result: {e}");
        "{}".into()
    })
}

pub fn format_human(result: &QueryResult) -> String {
    let mut out = String::new();

    for qf in &result.query_functions {
        out.push_str(&format!(
            "\n{} ({}:{}, {}L) {}\n",
            qf.name, qf.file, qf.line, qf.line_count, qf.signature
        ));

        if qf.candidates.is_empty() {
            out.push_str("  No similar functions found\n");
        } else {
            for c in &qf.candidates {
                let similarity = format_similarity(c.distance);
                let display_name = format_display_name(
                    &c.name,
                    c.chunk_type,
                    c.context.as_deref(),
                );
                let jaccard_suffix = format_jaccard_suffix(c.jaccard_similarity);
                out.push_str(&format!(
                    "  {display_name} ({}:{}, {}L) {} — similarity: {similarity} [{}{jaccard_suffix}]\n",
                    c.path, c.line, c.line_count, c.signature, c.detection_method
                ));
            }
        }
    }

    if !result.warnings.is_empty() {
        out.push_str("\nWarnings:\n");
        for w in &result.warnings {
            out.push_str(&format!("  {w}\n"));
        }
    }

    out.push_str(&format!(
        "\n{} functions checked against {} indexed ({}ms)\n",
        result.meta.query_functions, result.meta.indexed_functions, result.meta.elapsed_ms
    ));

    out
}

pub fn format_index_json(result: &IndexResult) -> String {
    serde_json::to_string_pretty(result).unwrap_or_else(|e| {
        eprintln!("Warning: failed to serialize index result: {e}");
        "{}".into()
    })
}

pub fn format_status_json(result: &StatusResult) -> String {
    serde_json::to_string_pretty(result).unwrap_or_else(|e| {
        eprintln!("Warning: failed to serialize status result: {e}");
        "{}".into()
    })
}

pub fn format_status_human(result: &StatusResult) -> String {
    if !result.exists {
        return "No index found. Run `vibec index` to create one.".into();
    }

    let size_mb = result.size_bytes as f64 / 1024.0 / 1024.0;

    let unembedded = if result.unembedded > 0 {
        format!(" ({} awaiting embedding)", result.unembedded)
    } else {
        String::new()
    };

    let stale = if result.stale_exclusions > 0 {
        format!(" ({} stale)", result.stale_exclusions)
    } else {
        String::new()
    };

    let file_excl = if result.file_exclusions > 0 {
        format!("\nFile exclusions: {}", result.file_exclusions)
    } else {
        String::new()
    };

    let file_pair_excl = if result.file_pair_exclusions > 0 {
        format!("\nFile pair exclusions: {}", result.file_pair_exclusions)
    } else {
        String::new()
    };

    format!(
        "Database: {} ({:.1} MB)\nModel: {}\nDimensions: {}\nIndexed functions: {}{}\nTracked files: {}\nLast indexed: {}\nExclusions: {}{}{}{}",
        result.db_path, size_mb, result.model, result.dimensions,
        result.indexed_functions, unembedded, result.tracked_files,
        result.last_indexed, result.exclusions, stale, file_excl, file_pair_excl
    )
}
