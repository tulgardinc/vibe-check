use crate::output::types::QueryResult;

pub fn format_json(result: &QueryResult) -> String {
    serde_json::to_string_pretty(result).unwrap_or_default()
}

pub fn format_human(result: &QueryResult) -> String {
    let mut out = String::new();

    for qf in &result.query_functions {
        out.push_str(&format!(
            "\n{} ({}:{}, {}L) {}\n",
            qf.name, qf.file, qf.line, qf.line_count, qf.signature
        ));

        if qf.candidates.is_empty() {
            out.push_str("  No similar functions found.\n");
        } else {
            for c in &qf.candidates {
                let similarity = format!("{:.2}", 1.0 - c.distance);
                let context_suffix = match (&c.chunk_type, &c.context) {
                    (Some(ct), Some(ctx)) if ct == "block" => format!(" in {ctx}"),
                    _ => String::new(),
                };
                let display_name = format!("{}{context_suffix}", c.name);
                let jaccard_suffix = match c.jaccard_similarity {
                    Some(j) => format!(", jaccard: {j:.2}"),
                    None => String::new(),
                };
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
