use crate::parser::language::{LanguageSupport, NodeRole};
use crate::parser::types::ParamInfo;
use tree_sitter::Node;

static STOP_WORDS: &[&str] = &[
    "fn", "let", "mut", "const", "static", "pub", "crate", "super", "self", "mod", "use",
    "struct", "enum", "trait", "impl", "type", "where", "if", "else", "match", "for", "while",
    "loop", "break", "continue", "return", "in", "as", "ref", "move", "async", "await", "unsafe",
    "extern", "true", "false", "some", "none", "ok", "err",
];

pub struct RustSupport;

impl LanguageSupport for RustSupport {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_rust::LANGUAGE.into()
    }

    fn file_extensions(&self) -> &'static [&'static str] {
        &["rs"]
    }

    fn is_excluded_file(&self, filename: &str) -> bool {
        filename == "build.rs"
    }

    fn classify_node(&self, node: Node) -> NodeRole {
        match node.kind() {
            "function_item" | "function_signature_item" | "closure_expression" => NodeRole::Function,

            "if_expression" | "for_expression" | "while_expression" | "loop_expression"
            | "match_expression" => NodeRole::BlockParent,

            "block" => NodeRole::Block,

            "type_parameters" | "type_arguments" | "where_clause" => NodeRole::TypeAnnotation,

            _ => NodeRole::Other,
        }
    }

    fn is_identifier(&self, node: Node) -> bool {
        matches!(
            node.kind(),
            "identifier" | "field_identifier" | "shorthand_field_identifier"
        )
    }

    fn stop_words(&self) -> &'static [&'static str] {
        STOP_WORDS
    }

    fn extract_function_name(&self, node: Node, source: &[u8]) -> Option<String> {
        match node.kind() {
            "function_item" | "function_signature_item" => {
                let name_node = node.child_by_field_name("name")?;
                let text = name_node.utf8_text(source).unwrap_or("");
                if text.is_empty() {
                    None
                } else {
                    Some(text.to_string())
                }
            }
            _ => None,
        }
    }

    fn extract_params(&self, node: Node, source: &[u8]) -> Vec<ParamInfo> {
        let params_node = match node.child_by_field_name("parameters") {
            Some(n) => n,
            None => return vec![],
        };

        let mut params = Vec::new();
        let mut cursor = params_node.walk();

        for child in params_node.named_children(&mut cursor) {
            match child.kind() {
                "parameter" => {
                    let name = child
                        .child_by_field_name("pattern")
                        .map(|n| n.utf8_text(source).unwrap_or("").to_string())
                        .unwrap_or_default();

                    let type_ = child
                        .child_by_field_name("type")
                        .map(|n| n.utf8_text(source).unwrap_or("").to_string())
                        .filter(|s| !s.is_empty());

                    if !name.is_empty() {
                        params.push(ParamInfo { name, type_ });
                    }
                }
                "self_parameter" => {
                    let text = child.utf8_text(source).unwrap_or("self").to_string();
                    params.push(ParamInfo {
                        name: text,
                        type_: None,
                    });
                }
                _ => {}
            }
        }

        params
    }

    fn extract_return_type(&self, node: Node, source: &[u8]) -> Option<String> {
        let type_node = node.child_by_field_name("return_type")?;
        let text = type_node.utf8_text(source).unwrap_or("");
        if text.is_empty() {
            None
        } else {
            Some(text.to_string())
        }
    }

    fn is_function_exported(&self, _name: &str, _parent_exported: bool) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    /// Parse source and call the callback with each function node found.
    fn with_functions(source: &str, mut callback: impl FnMut(&str, Node, &[u8])) {
        let lang = RustSupport;
        let mut parser = Parser::new();
        parser.set_language(&lang.language()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let bytes = source.as_bytes();

        fn walk<'a>(
            node: Node<'a>,
            lang: &RustSupport,
            source: &[u8],
            callback: &mut impl FnMut(&str, Node, &[u8]),
        ) {
            if lang.classify_node(node) == NodeRole::Function {
                if let Some(name) = lang.extract_function_name(node, source) {
                    callback(&name, node, source);
                }
                return;
            }
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                walk(child, lang, source, callback);
            }
        }

        walk(tree.root_node(), &lang, bytes, &mut callback);
    }

    #[test]
    fn extracts_function_declaration() {
        let source = r#"
fn greet(name: &str) -> String {
    let greeting = format!("Hello, {}", name);
    greeting
}
"#;
        let mut found = false;
        with_functions(source, |name, node, src| {
            assert_eq!(name, "greet");

            let lang = RustSupport;
            let params = lang.extract_params(node, src);
            assert_eq!(params.len(), 1);
            assert_eq!(params[0].name, "name");
            assert_eq!(params[0].type_.as_deref(), Some("&str"));

            let ret = lang.extract_return_type(node, src);
            assert_eq!(ret.as_deref(), Some("String"));
            found = true;
        });
        assert!(found, "should find greet function");
    }

    #[test]
    fn extracts_impl_method() {
        let source = r#"
struct MyStruct;

impl MyStruct {
    fn process(&self, x: i32) -> bool {
        let result = x > 0;
        result
    }
}
"#;
        let mut found = false;
        with_functions(source, |name, node, src| {
            if name == "process" {
                let lang = RustSupport;
                let params = lang.extract_params(node, src);
                assert_eq!(params.len(), 2);
                assert_eq!(params[0].name, "&self");
                assert_eq!(params[0].type_, None);
                assert_eq!(params[1].name, "x");
                assert_eq!(params[1].type_.as_deref(), Some("i32"));

                let ret = lang.extract_return_type(node, src);
                assert_eq!(ret.as_deref(), Some("bool"));
                found = true;
            }
        });
        assert!(found, "should find process method");
    }

    #[test]
    fn skips_tiny_functions() {
        let source = "fn tiny(x: i32) -> i32 { x + 1 }";
        let mut count = 0;
        with_functions(source, |_, node, _| {
            let lines = node.end_position().row - node.start_position().row + 1;
            if lines >= 3 {
                count += 1;
            }
        });
        assert_eq!(count, 0, "single-line function should be filtered");
    }

    #[test]
    fn skips_closures() {
        let source = r#"
fn main() {
    let f = |x: i32| {
        x + 1
    };
    println!("{}", f(1));
}
"#;
        let mut names: Vec<String> = Vec::new();
        with_functions(source, |name, _, _| {
            names.push(name.to_string());
        });
        // Only `main` should be found — the closure has no name
        assert_eq!(names, vec!["main"]);
    }

    #[test]
    fn extracts_generic_return_type() {
        let source = r#"
fn try_parse(input: &str) -> Result<String, Error> {
    let parsed = input.to_string();
    Ok(parsed)
}
"#;
        with_functions(source, |name, node, src| {
            assert_eq!(name, "try_parse");
            let lang = RustSupport;
            let ret = lang.extract_return_type(node, src);
            assert_eq!(ret.as_deref(), Some("Result<String, Error>"));
        });
    }

    #[test]
    fn deterministic_id_format() {
        let source = r#"
fn foo(x: i32) -> i32 {
    let y = x * 2;
    y
}
"#;
        with_functions(source, |name, node, _| {
            let start_line = node.start_position().row + 1;
            let id = format!("src/lib.rs:{name}:{start_line}");
            assert_eq!(id, "src/lib.rs:foo:2");
        });
    }

    #[test]
    fn excludes_build_rs() {
        let lang = RustSupport;
        assert!(lang.is_excluded_file("build.rs"));
        assert!(!lang.is_excluded_file("main.rs"));
        assert!(!lang.is_excluded_file("lib.rs"));
    }

    #[test]
    fn tokens_exclude_type_identifiers() {
        let source = r#"
fn process(items: Vec<String>, count: usize) -> bool {
    let filtered = items.len();
    filtered > count
}
"#;
        let lang = RustSupport;
        let mut parser = Parser::new();
        parser.set_language(&lang.language()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let bytes = source.as_bytes();

        // Walk the tree and verify is_identifier classification by node kind
        let mut has_type_id = false;
        let mut value_ids = Vec::new();
        fn check(node: Node, lang: &RustSupport, source: &[u8], has_type: &mut bool, vals: &mut Vec<String>) {
            if node.kind() == "type_identifier" {
                *has_type = true;
                assert!(!lang.is_identifier(node), "type_identifier should NOT pass is_identifier");
            }
            if lang.is_identifier(node) {
                vals.push(node.utf8_text(source).unwrap_or("").to_string());
            }
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                check(child, lang, source, has_type, vals);
            }
        }
        check(tree.root_node(), &lang, bytes, &mut has_type_id, &mut value_ids);

        assert!(has_type_id, "source should contain type_identifier nodes");
        assert!(value_ids.contains(&"items".to_string()));
        assert!(value_ids.contains(&"filtered".to_string()));
        assert!(value_ids.contains(&"count".to_string()));
        // Type names should NOT appear in value identifiers
        assert!(!value_ids.contains(&"Vec".to_string()));
        assert!(!value_ids.contains(&"String".to_string()));
    }

    #[test]
    fn signature_hash_format() {
        use crate::parser::signature::compute_signature_hash;

        let params = vec![
            ParamInfo {
                name: "a".to_string(),
                type_: Some("i32".to_string()),
            },
            ParamInfo {
                name: "b".to_string(),
                type_: Some("i32".to_string()),
            },
        ];
        let hash = compute_signature_hash("add", &params, Some("i32"));
        assert_eq!(hash.len(), 16);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn classify_node_roles() {
        let source = r#"
fn example() {
    if true {
        for i in 0..10 {
            println!("{}", i);
        }
    }
}
"#;
        let lang = RustSupport;
        let mut parser = Parser::new();
        parser.set_language(&lang.language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        fn find_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
            if node.kind() == kind {
                return Some(node);
            }
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if let Some(found) = find_kind(child, kind) {
                    return Some(found);
                }
            }
            None
        }

        let root = tree.root_node();

        let fn_node = find_kind(root, "function_item").unwrap();
        assert_eq!(lang.classify_node(fn_node), NodeRole::Function);

        let if_node = find_kind(root, "if_expression").unwrap();
        assert_eq!(lang.classify_node(if_node), NodeRole::BlockParent);

        let for_node = find_kind(root, "for_expression").unwrap();
        assert_eq!(lang.classify_node(for_node), NodeRole::BlockParent);

        let block_node = find_kind(root, "block").unwrap();
        assert_eq!(lang.classify_node(block_node), NodeRole::Block);
    }
}
