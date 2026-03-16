use std::collections::HashSet;

/// Default weight for embedding distance vs Jaccard in re-ranking (0.0–1.0).
/// Higher values weight embedding distance more; lower values weight token overlap more.
pub const DEFAULT_RERANK_ALPHA: f64 = 0.7;

/// Compute combined score blending embedding distance and Jaccard similarity.
pub fn combined_score(distance: f64, jaccard: f64, alpha: f64) -> f64 {
    alpha * distance + (1.0 - alpha) * (1.0 - jaccard)
}

pub fn jaccard_similarity(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }

    let (smaller, larger) = if a.len() <= b.len() {
        (a, b)
    } else {
        (b, a)
    };

    let intersection = smaller.iter().filter(|t| larger.contains(*t)).count();
    let union = a.len() + b.len() - intersection;

    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

#[derive(Debug, Clone)]
pub struct RankedCandidate {
    pub candidate: CandidateForRerank,
    pub jaccard_similarity: f64,
    pub combined_score: f64,
}

pub fn rerank_candidates(
    query_tokens: &HashSet<String>,
    candidates: Vec<CandidateForRerank>,
    alpha: f64,
) -> Vec<RankedCandidate> {
    let mut ranked: Vec<RankedCandidate> = candidates
        .into_iter()
        .map(|c| {
            let jaccard = jaccard_similarity(query_tokens, &c.tokens);
            let score = combined_score(c.distance, jaccard, alpha);
            RankedCandidate {
                candidate: c,
                jaccard_similarity: jaccard,
                combined_score: score,
            }
        })
        .collect();

    ranked.sort_by(|a, b| {
        a.combined_score
            .partial_cmp(&b.combined_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    ranked
}

#[derive(Debug, Clone)]
pub struct CandidateForRerank {
    pub name: String,
    pub path: String,
    pub line: usize,
    pub line_count: usize,
    pub signature: String,
    pub distance: f64,
    pub detection_method: String,
    pub source: String,
    pub signature_hash: String,
    pub chunk_type: Option<String>,
    pub context: Option<String>,
    pub tokens: HashSet<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jaccard_identical_sets() {
        let a: HashSet<String> = ["foo", "bar"].iter().map(|s| s.to_string()).collect();
        let b = a.clone();
        assert_eq!(jaccard_similarity(&a, &b), 1.0);
    }

    #[test]
    fn jaccard_disjoint_sets() {
        let a: HashSet<String> = ["foo"].iter().map(|s| s.to_string()).collect();
        let b: HashSet<String> = ["bar"].iter().map(|s| s.to_string()).collect();
        assert_eq!(jaccard_similarity(&a, &b), 0.0);
    }

    #[test]
    fn jaccard_both_empty() {
        let a: HashSet<String> = HashSet::new();
        let b: HashSet<String> = HashSet::new();
        assert_eq!(jaccard_similarity(&a, &b), 1.0);
    }

    #[test]
    fn jaccard_partial_overlap() {
        let a: HashSet<String> = ["foo", "bar", "baz"].iter().map(|s| s.to_string()).collect();
        let b: HashSet<String> = ["foo", "bar", "qux"].iter().map(|s| s.to_string()).collect();
        // intersection=2, union=4
        assert!((jaccard_similarity(&a, &b) - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn rerank_sorts_by_combined_score() {
        let query_tokens: HashSet<String> =
            ["hello_world"].iter().map(|s| s.to_string()).collect();

        let candidates = vec![
            CandidateForRerank {
                name: "far".into(),
                path: "a.ts".into(),
                line: 1,
                line_count: 1,
                signature: "()".into(),
                distance: 0.5,
                detection_method: "embedding".into(),
                source: "function unique_xyz() { return 1; }".into(),
                signature_hash: "aaaaaaaa".into(),
                chunk_type: None,
                context: None,
                tokens: ["unique_xyz"].iter().map(|s| s.to_string()).collect(),
            },
            CandidateForRerank {
                name: "close".into(),
                path: "b.ts".into(),
                line: 1,
                line_count: 1,
                signature: "()".into(),
                distance: 0.1,
                detection_method: "embedding".into(),
                source: "function hello_world() { return 1; }".into(),
                signature_hash: "bbbbbbbb".into(),
                chunk_type: None,
                context: None,
                tokens: ["hello_world"].iter().map(|s| s.to_string()).collect(),
            },
        ];

        let ranked = rerank_candidates(&query_tokens, candidates, 0.7);
        assert_eq!(ranked[0].candidate.name, "close");
        assert!(ranked[0].combined_score < ranked[1].combined_score);
    }
}
