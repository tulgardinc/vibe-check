use crate::parser::language::{LanguageSupport, NodeRole};
use crate::parser::typescript::{
    contains_function_value, extract_variable_function_name, find_variable_function_value,
};
use crate::parser::types::ParamInfo;
use tree_sitter::Node;

static STOP_WORDS: &[&str] = &[
    "const", "let", "var", "function", "return", "if", "else", "for", "while", "do",
    "switch", "case", "break", "continue", "try", "catch", "finally", "throw", "new",
    "this", "typeof", "instanceof", "void", "delete", "in", "of", "import", "export",
    "from", "default", "async", "await", "class", "extends", "true", "false", "null",
    "undefined",
];

pub struct JavaScriptSupport;

impl LanguageSupport for JavaScriptSupport {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_javascript::LANGUAGE.into()
    }

    fn file_extensions(&self) -> &'static [&'static str] {
        &["js", "mjs", "cjs", "jsx"]
    }

    fn is_excluded_file(&self, filename: &str) -> bool {
        filename.ends_with(".test.js")
            || filename.ends_with(".spec.js")
            || filename.ends_with(".test.jsx")
            || filename.ends_with(".spec.jsx")
            || filename.ends_with(".test.mjs")
            || filename.ends_with(".spec.mjs")
            || filename.ends_with(".test.cjs")
            || filename.ends_with(".spec.cjs")
    }

    fn classify_node(&self, node: Node) -> NodeRole {
        match node.kind() {
            "function_declaration"
            | "generator_function_declaration"
            | "method_definition"
            | "arrow_function"
            | "function_expression" => NodeRole::Function,

            "lexical_declaration" | "variable_declaration"
                if contains_function_value(node) =>
            {
                NodeRole::Function
            }

            "if_statement" | "for_statement" | "for_in_statement" | "while_statement"
            | "do_statement" | "try_statement" => NodeRole::BlockParent,

            "switch_statement" => NodeRole::ControlFlow,

            "statement_block" => NodeRole::Block,

            "export_statement" | "export_default_declaration" => NodeRole::Export,

            _ => NodeRole::Other,
        }
    }

    fn is_identifier(&self, node: Node) -> bool {
        matches!(node.kind(), "identifier" | "property_identifier")
    }

    fn stop_words(&self) -> &'static [&'static str] {
        STOP_WORDS
    }

    fn extract_function_name(&self, node: Node, source: &[u8]) -> Option<String> {
        if node.kind() == "lexical_declaration" || node.kind() == "variable_declaration" {
            return extract_variable_function_name(node, source);
        }

        let name_node = node.child_by_field_name("name")?;
        let text = name_node.utf8_text(source).unwrap_or("");
        if text.is_empty() {
            None
        } else {
            Some(text.to_string())
        }
    }

    fn extract_params(&self, node: Node, source: &[u8]) -> Vec<ParamInfo> {
        let func_node = self.inner_function_node(node).unwrap_or(node);
        let params_node = match func_node.child_by_field_name("parameters") {
            Some(n) => n,
            None => return vec![],
        };

        let mut params = Vec::new();
        let mut cursor = params_node.walk();

        for child in params_node.named_children(&mut cursor) {
            let kind = child.kind();

            let param_name = match kind {
                "identifier" => child.utf8_text(source).unwrap_or("").to_string(),

                "assignment_pattern" => child
                    .child_by_field_name("left")
                    .map(|n| n.utf8_text(source).unwrap_or("").to_string())
                    .unwrap_or_default(),

                "rest_pattern" => {
                    let inner = child
                        .named_child(0)
                        .map(|n| n.utf8_text(source).unwrap_or("").to_string())
                        .unwrap_or_default();
                    format!("...{inner}")
                }

                "object_pattern" | "array_pattern" => {
                    child.utf8_text(source).unwrap_or("").to_string()
                }

                _ => continue,
            };

            if param_name.is_empty() {
                continue;
            }

            params.push(ParamInfo {
                name: param_name,
                type_: None,
            });
        }

        params
    }

    fn extract_return_type(&self, _node: Node, _source: &[u8]) -> Option<String> {
        None
    }

    fn inner_function_node<'a>(&self, node: Node<'a>) -> Option<Node<'a>> {
        if node.kind() != "lexical_declaration" && node.kind() != "variable_declaration" {
            return None;
        }
        find_variable_function_value(node)
    }
}
