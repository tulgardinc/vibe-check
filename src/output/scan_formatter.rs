use crate::output::types::{ScanResult, SIMILARITY_TIERS, format_display_name, format_jaccard_suffix};
use std::path::Path;

pub fn format_scan_json(result: &ScanResult) -> String {
    serde_json::to_string_pretty(result).unwrap_or_else(|e| {
        eprintln!("Warning: failed to serialize scan result: {e}");
        "{}".into()
    })
}

pub fn format_scan_human(result: &ScanResult, project_root: &Path) -> String {
    let mut out = String::new();

    if result.matches.is_empty() {
        out.push_str("No similar function pairs found\n");
        return out;
    }

    for tier in SIMILARITY_TIERS {
        let group: Vec<_> = result
            .matches
            .iter()
            .filter(|m| m.similarity == *tier)
            .collect();

        if group.is_empty() {
            continue;
        }

        let padding = 50usize.saturating_sub(tier.len());
        let dashes = "\u{2500}".repeat(padding);
        out.push_str(&format!(
            "\n\u{2500}\u{2500} {} ({}) {dashes}\n",
            tier.to_uppercase(),
            group.len()
        ));

        for m in &group {
            let rel_a = make_relative(&m.a.path, project_root);
            let rel_b = make_relative(&m.b.path, project_root);
            let pct = ((1.0 - m.distance) * 100.0) as u32;

            let name_a = format_display_name(
                &m.a.name,
                m.a.chunk_type,
                m.a.context.as_deref(),
            );
            let name_b = format_display_name(
                &m.b.name,
                m.b.chunk_type,
                m.b.context.as_deref(),
            );

            let jaccard_info = format_jaccard_suffix(m.jaccard_similarity);

            out.push_str(&format!(
                "  {name_a} ({rel_a}:{}, {}L) {}\n",
                m.a.line, m.a.line_count, m.a.signature
            ));
            out.push_str(&format!(
                "  {name_b} ({rel_b}:{}, {}L) {}\n",
                m.b.line, m.b.line_count, m.b.signature
            ));
            out.push_str(&format!(
                "  {pct}% similar (distance: {:.4}{jaccard_info})\n\n",
                m.distance
            ));
        }
    }

    out.push_str(&format!(
        "{} pairs from {} chunks ({}ms)\n",
        result.meta.pairs_found, result.meta.chunks_scanned, result.meta.elapsed_ms
    ));

    out
}

fn make_relative(path: &str, project_root: &Path) -> String {
    let p = Path::new(path);
    p.strip_prefix(project_root)
        .map(|r| r.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string())
}
