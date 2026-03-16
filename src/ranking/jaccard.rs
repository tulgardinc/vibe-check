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

/// An item to be re-ranked, wrapping an arbitrary payload with its distance and tokens.
#[derive(Debug)]
pub struct RerankItem<T> {
    pub item: T,
    pub distance: f64,
    pub tokens: HashSet<String>,
}

/// Result of re-ranking: the original item plus computed scores.
#[derive(Debug)]
pub struct Reranked<T> {
    pub item: T,
    pub combined_score: f64,
    pub jaccard_similarity: f64,
}

/// Re-rank candidates by blending embedding distance with Jaccard token similarity.
/// Returns results sorted by combined score (ascending = most similar first).
pub fn rerank<T>(
    query_tokens: &HashSet<String>,
    candidates: Vec<RerankItem<T>>,
    alpha: f64,
) -> Vec<Reranked<T>> {
    let mut ranked: Vec<Reranked<T>> = candidates
        .into_iter()
        .map(|c| {
            let jaccard = jaccard_similarity(query_tokens, &c.tokens);
            let score = combined_score(c.distance, jaccard, alpha);
            Reranked {
                item: c.item,
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
            RerankItem {
                item: "far",
                distance: 0.5,
                tokens: ["unique_xyz"].iter().map(|s| s.to_string()).collect(),
            },
            RerankItem {
                item: "close",
                distance: 0.1,
                tokens: ["hello_world"].iter().map(|s| s.to_string()).collect(),
            },
        ];

        let ranked = rerank(&query_tokens, candidates, 0.7);
        assert_eq!(ranked[0].item, "close");
        assert!(ranked[0].combined_score < ranked[1].combined_score);
    }
}
