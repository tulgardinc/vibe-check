use crate::parser::types::ParamInfo;
use tree_sitter::Node;

/// How the generic walker should treat a given AST node.
///
/// # Walker contract
///
/// The generic walker in `chunker.rs` behaves as follows for each role:
///
/// - **`Function`** — Extracts as a chunk. Calls `extract_function_name`,
///   `extract_params`, `extract_return_type`, `function_body`. Does NOT
///   recurse into children (function bodies are walked separately for blocks).
///   If `extract_function_name` returns `None`, the node is silently skipped.
///   Nodes smaller than `MIN_LINES` (3) are filtered out.
///
/// - **`BlockParent`** — The walker looks at this node's direct children for
///   `Block` nodes eligible for extraction. Also implies `is_control_flow`.
///   Example: `if_statement` whose child `statement_block` may be extracted.
///
/// - **`ControlFlow`** — Counts toward block eligibility heuristics (a block
///   must contain at least one control-flow statement to be extracted).
///   Does NOT trigger block child scanning (unlike `BlockParent`).
///
/// - **`Block`** — A braced/indented body that may be extracted as a block
///   chunk if it meets size and complexity thresholds (`MIN_BLOCK_LINES`,
///   at least 2 statements, at least 1 control-flow statement).
///
/// - **`TypeAnnotation`** — Skipped entirely during token collection.
///   Recurse normally during chunk/block extraction. Use this for any
///   type-system syntax that should not contribute to semantic tokens.
///
/// - **`Export`** — Sets `parent_exported = true` for all descendants.
///   The walker recurses into children normally. Use for wrapper nodes
///   like `export_statement`. For non-wrapper visibility (Go capitalization,
///   Rust `pub`), override `is_function_exported` instead.
///
/// - **`Other`** — Recurse into children. No special behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeRole {
    /// A function/method definition — extract as a chunk, don't recurse into body.
    Function,
    /// A control-flow statement (switch, etc.) — counts toward block eligibility.
    ControlFlow,
    /// A control-flow statement whose children may contain extractable blocks.
    /// Implies `is_control_flow() == true`.
    BlockParent,
    /// A braced/indented statement block.
    Block,
    /// A type-system node — skip during token collection.
    TypeAnnotation,
    /// An export/visibility wrapper — propagates `is_exported` to children.
    Export,
    /// Nothing special — recurse into children.
    Other,
}

impl NodeRole {
    /// True for both `ControlFlow` and `BlockParent`.
    pub fn is_control_flow(self) -> bool {
        matches!(self, NodeRole::ControlFlow | NodeRole::BlockParent)
    }

    /// True only for `BlockParent`.
    pub fn is_block_parent(self) -> bool {
        matches!(self, NodeRole::BlockParent)
    }
}

/// Per-language tree-sitter extraction logic.
///
/// Implementations provide the grammar, node classification, and extraction
/// methods that let the generic chunker work across languages.
pub trait LanguageSupport: Send + Sync {
    /// The tree-sitter Language object for this grammar.
    fn language(&self) -> tree_sitter::Language;

    /// Extensions this language handles, without the dot (e.g. `["ts"]`).
    fn file_extensions(&self) -> &'static [&'static str];

    /// Return true if `filename` should be skipped even though its extension matches.
    fn is_excluded_file(&self, _filename: &str) -> bool {
        false
    }

    /// Classify any AST node. This is the single dispatch point —
    /// the generic walker never hardcodes node kind strings.
    fn classify_node(&self, node: Node) -> NodeRole;

    /// Return true if `node` is an identifier token that should be collected
    /// for Jaccard similarity. Called during token extraction on every leaf.
    /// Exclude type-position identifiers here if `TypeAnnotation` classification
    /// does not already cover them.
    fn is_identifier(&self, node: Node) -> bool;

    /// Language keywords to exclude from token sets.
    fn stop_words(&self) -> &'static [&'static str];

    /// Extract the function name from a node classified as `Function`.
    fn extract_function_name(&self, node: Node, source: &[u8]) -> Option<String>;

    /// Extract typed parameters from a function node.
    fn extract_params(&self, node: Node, source: &[u8]) -> Vec<ParamInfo>;

    /// Extract the return type annotation, if any.
    fn extract_return_type(&self, node: Node, source: &[u8]) -> Option<String>;

    /// For wrapper nodes classified as `Function` (e.g. `const f = () => {}`),
    /// return the inner function node for param/return-type extraction.
    /// Default: `None`.
    fn inner_function_node<'a>(&self, _node: Node<'a>) -> Option<Node<'a>> {
        None
    }

    /// Return the body node of a function for block extraction.
    /// Default: unwraps via `inner_function_node`, then tries `child_by_field_name("body")`.
    fn function_body<'a>(&self, node: Node<'a>) -> Option<Node<'a>> {
        let func = self.inner_function_node(node).unwrap_or(node);
        func.child_by_field_name("body")
    }

    /// Determine whether a function is exported/public.
    /// `parent_exported` is true when an ancestor was classified as `Export`
    /// (covers wrapper-based exports like JS/TS `export`).
    /// Override for non-wrapper visibility (e.g. Go capitalisation, Python `_` prefix).
    /// Default: returns `parent_exported`.
    fn is_function_exported(&self, _name: &str, parent_exported: bool) -> bool {
        parent_exported
    }
}
