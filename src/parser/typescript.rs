use crate::parser::language::{LanguageSupport, NodeRole};
use crate::parser::types::ParamInfo;
use std::collections::HashSet;
use std::sync::LazyLock;
use tree_sitter::Node;

static STOP_WORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "const", "let", "var", "function", "return", "if", "else", "for", "while", "do",
        "switch", "case", "break", "continue", "try", "catch", "finally", "throw", "new",
        "this", "typeof", "instanceof", "void", "delete", "in", "of", "import", "export",
        "from", "default", "async", "await", "class", "extends", "implements", "interface",
        "type", "enum", "true", "false", "null", "undefined",
    ]
    .into_iter()
    .collect()
});

pub struct TypeScriptSupport;

impl LanguageSupport for TypeScriptSupport {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    }

    fn file_extensions(&self) -> &'static [&'static str] {
        &["ts"]
    }

    fn is_excluded_file(&self, filename: &str) -> bool {
        filename.ends_with(".d.ts")
            || filename.ends_with(".test.ts")
            || filename.ends_with(".spec.ts")
    }

    fn classify_node(&self, node: Node) -> NodeRole {
        match node.kind() {
            // Direct function definitions
            "function_declaration"
            | "generator_function_declaration"
            | "method_definition"
            | "arrow_function"
            | "function_expression" => NodeRole::Function,

            // Variable declarations wrapping a function value
            "lexical_declaration" | "variable_declaration"
                if contains_function_value(node) =>
            {
                NodeRole::Function
            }

            // Control flow — only block-eligible parents
            "if_statement" | "for_statement" | "for_in_statement" | "while_statement"
            | "do_statement" | "try_statement" => NodeRole::BlockParent,

            // Control flow — not a block parent
            "switch_statement" => NodeRole::ControlFlow,

            // Statement block
            "statement_block" => NodeRole::Block,

            // Type-system nodes (skipped during token collection)
            "type_annotation" | "type_arguments" | "type_parameters" | "as_expression"
            | "satisfies_expression" | "return_type" => NodeRole::TypeAnnotation,

            // Export wrappers
            "export_statement" | "export_default_declaration" => NodeRole::Export,

            _ => NodeRole::Other,
        }
    }

    fn stop_words(&self) -> &HashSet<&'static str> {
        &STOP_WORDS
    }

    fn extract_function_name(&self, node: Node, source: &[u8]) -> Option<String> {
        // Variable-wrapped function: dig into declarator for the name
        if node.kind() == "lexical_declaration" || node.kind() == "variable_declaration" {
            return extract_variable_function_name(node, source);
        }

        // Direct function: use the name field
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
            if kind != "required_parameter"
                && kind != "optional_parameter"
                && kind != "rest_parameter"
            {
                continue;
            }

            let raw_name = child
                .child_by_field_name("pattern")
                .or_else(|| child.child_by_field_name("name"))
                .map(|n| n.utf8_text(source).unwrap_or("").to_string())
                .unwrap_or_else(|| child.utf8_text(source).unwrap_or("").to_string());

            let param_name = if kind == "rest_parameter" {
                format!("...{raw_name}")
            } else {
                raw_name
            };

            let type_annotation = child
                .child_by_field_name("type")
                .and_then(|n| strip_type_prefix(n.utf8_text(source).unwrap_or("")));

            params.push(ParamInfo {
                name: param_name,
                type_: type_annotation,
            });
        }

        params
    }

    fn extract_return_type(&self, node: Node, source: &[u8]) -> Option<String> {
        let func_node = self.inner_function_node(node).unwrap_or(node);
        func_node
            .child_by_field_name("return_type")
            .and_then(|n| strip_type_prefix(n.utf8_text(source).unwrap_or("")))
    }

    fn inner_function_node<'a>(&self, node: Node<'a>) -> Option<Node<'a>> {
        if node.kind() != "lexical_declaration" && node.kind() != "variable_declaration" {
            return None;
        }
        find_variable_function_value(node)
    }
}

/// Check if a variable declaration contains a function value (without needing source bytes).
fn contains_function_value(node: Node) -> bool {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "variable_declarator" {
            if let Some(value) = child.child_by_field_name("value") {
                let kind = value.kind();
                if kind == "arrow_function" || kind == "function_expression" {
                    return true;
                }
            }
        }
    }
    false
}

/// Find the function node inside a variable declaration.
fn find_variable_function_value(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "variable_declarator" {
            if let Some(value) = child.child_by_field_name("value") {
                let kind = value.kind();
                if kind == "arrow_function" || kind == "function_expression" {
                    return Some(value);
                }
            }
        }
    }
    None
}

/// Extract the variable name from a declaration like `const foo = () => {}`.
fn extract_variable_function_name(node: Node, source: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "variable_declarator" {
            if let Some(value) = child.child_by_field_name("value") {
                let kind = value.kind();
                if kind == "arrow_function" || kind == "function_expression" {
                    if let Some(name_node) = child.child_by_field_name("name") {
                        let text = name_node.utf8_text(source).unwrap_or("");
                        if !text.is_empty() {
                            return Some(text.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

fn strip_type_prefix(text: &str) -> Option<String> {
    let trimmed = text.strip_prefix(':').unwrap_or(text).trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
