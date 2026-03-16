use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

static STOP_WORDS: LazyLock<HashSet<&str>> = LazyLock::new(|| {
    [
        "const",
        "let",
        "var",
        "function",
        "return",
        "if",
        "else",
        "for",
        "while",
        "do",
        "switch",
        "case",
        "break",
        "continue",
        "try",
        "catch",
        "finally",
        "throw",
        "new",
        "this",
        "typeof",
        "instanceof",
        "void",
        "delete",
        "in",
        "of",
        "import",
        "export",
        "from",
        "default",
        "async",
        "await",
        "class",
        "extends",
        "implements",
        "interface",
        "type",
        "enum",
        "true",
        "false",
        "null",
        "undefined",
    ]
    .into_iter()
    .collect()
});

// Strips `: Type` annotations. The JS version uses a lookahead (?=[;,)=\n{]) which
// the Rust regex crate doesn't support. This simpler pattern matches `: CapitalWord...`
// up to the next delimiter. It's used only for Jaccard tokenization, not parsing.
static TYPE_ANNOTATION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r":\s*[A-Z][\w<>,\s|&\[\]]+").unwrap());

static AS_CAST_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bas\s+\w+").unwrap());

static GENERIC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<[A-Z][\w<>,\s|&]*>").unwrap());

static TOKEN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[a-zA-Z_$][\w$]*|[+\-*/%=<>!&|^~?:]+").unwrap());

pub fn tokenize_code(code: &str) -> HashSet<String> {
    // Strip type annotations
    let stripped = TYPE_ANNOTATION_RE.replace_all(code, "");
    let stripped = AS_CAST_RE.replace_all(&stripped, "");
    let stripped = GENERIC_RE.replace_all(&stripped, "");

    let mut tokens = HashSet::new();

    for m in TOKEN_RE.find_iter(&stripped) {
        let lower = m.as_str().to_lowercase();
        if lower.len() > 1 && !STOP_WORDS.contains(lower.as_str()) {
            tokens.insert(lower);
        }
    }

    tokens
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
    pub jaccard_similarity: f64,
    pub combined_score: f64,
}

pub fn rerank_candidates(
    query_source: &str,
    candidates: Vec<CandidateForRerank>,
    alpha: f64,
) -> Vec<RankedCandidate> {
    let query_tokens = tokenize_code(query_source);

    let mut ranked: Vec<RankedCandidate> = candidates
        .into_iter()
        .map(|c| {
            let candidate_tokens = tokenize_code(&c.source);
            let jaccard = jaccard_similarity(&query_tokens, &candidate_tokens);
            let combined_score = alpha * c.distance + (1.0 - alpha) * (1.0 - jaccard);

            RankedCandidate {
                name: c.name,
                path: c.path,
                line: c.line,
                line_count: c.line_count,
                signature: c.signature,
                distance: c.distance,
                detection_method: c.detection_method,
                source: c.source,
                signature_hash: c.signature_hash,
                chunk_type: c.chunk_type,
                context: c.context,
                jaccard_similarity: jaccard,
                combined_score,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_filters_stop_words() {
        let tokens = tokenize_code("const x = function() { return true; }");
        assert!(!tokens.contains("const"));
        assert!(!tokens.contains("function"));
        assert!(!tokens.contains("return"));
        assert!(!tokens.contains("true"));
    }

    #[test]
    fn tokenize_filters_short_tokens() {
        let tokens = tokenize_code("a b c ab cd");
        assert!(!tokens.contains("a"));
        assert!(!tokens.contains("b"));
        assert!(!tokens.contains("c"));
        assert!(tokens.contains("ab"));
        assert!(tokens.contains("cd"));
    }

    #[test]
    fn tokenize_lowercases() {
        let tokens = tokenize_code("MyVariable AnotherOne");
        assert!(tokens.contains("myvariable"));
        assert!(tokens.contains("anotherone"));
    }

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
        let candidates = vec![
            CandidateForRerank {
                name: "far".into(),
                path: "a.ts".into(),
                line: 1,
                distance: 0.5,
                detection_method: "embedding".into(),
                source: "function unique_xyz() { return 1; }".into(),
                signature_hash: "aaaaaaaa".into(),
                chunk_type: None,
                context: None,
            },
            CandidateForRerank {
                name: "close".into(),
                path: "b.ts".into(),
                line: 1,
                distance: 0.1,
                detection_method: "embedding".into(),
                source: "function hello_world() { return 1; }".into(),
                signature_hash: "bbbbbbbb".into(),
                chunk_type: None,
                context: None,
            },
        ];

        let ranked = rerank_candidates("function hello_world() { return 1; }", candidates, 0.7);
        assert_eq!(ranked[0].name, "close");
        assert!(ranked[0].combined_score < ranked[1].combined_score);
    }
}
