use crate::parser::types::ParamInfo;
use std::collections::HashSet;
use tree_sitter::Node;

/// How the generic walker should treat a given AST node.
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

    /// Language keywords to exclude from token sets.
    fn stop_words(&self) -> &HashSet<&'static str>;

    /// Extract the function name from a node classified as `Function`.
    fn extract_function_name(&self, node: Node, source: &[u8]) -> Option<String>;

    /// Extract typed parameters from a function node.
    fn extract_params(&self, node: Node, source: &[u8]) -> Vec<ParamInfo>;

    /// Extract the return type annotation, if any.
    fn extract_return_type(&self, node: Node, source: &[u8]) -> Option<String>;

    /// For wrapper nodes classified as `Function` (e.g. `const f = () => {}`),
    /// return the inner function node for param/return-type extraction and
    /// block extraction on the correct body. Default: `None`.
    fn inner_function_node<'a>(&self, _node: Node<'a>) -> Option<Node<'a>> {
        None
    }
}
