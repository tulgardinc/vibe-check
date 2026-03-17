use crate::parser::language::{LanguageSupport, NodeRole};
use crate::parser::typescript::{
    contains_function_value, extract_variable_function_name, find_variable_function_value,
    strip_type_prefix,
};
use crate::parser::types::ParamInfo;
use tree_sitter::Node;

static STOP_WORDS: &[&str] = &[
    "const", "let", "var", "function", "return", "if", "else", "for", "while", "do",
    "switch", "case", "break", "continue", "try", "catch", "finally", "throw", "new",
    "this", "typeof", "instanceof", "void", "delete", "in", "of", "import", "export",
    "from", "default", "async", "await", "class", "extends", "implements", "interface",
    "type", "enum", "true", "false", "null", "undefined",
];

pub struct TsxSupport;

impl LanguageSupport for TsxSupport {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_typescript::LANGUAGE_TSX.into()
    }

    fn file_extensions(&self) -> &'static [&'static str] {
        &["tsx"]
    }

    fn is_excluded_file(&self, filename: &str) -> bool {
        filename.ends_with(".d.tsx")
            || filename.ends_with(".test.tsx")
            || filename.ends_with(".spec.tsx")
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

            "type_annotation" | "type_arguments" | "type_parameters" | "as_expression"
            | "satisfies_expression" | "return_type" => NodeRole::TypeAnnotation,

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
