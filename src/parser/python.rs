use crate::parser::language::{LanguageSupport, NodeRole};
use crate::parser::types::ParamInfo;
use tree_sitter::Node;

static STOP_WORDS: &[&str] = &[
    "def", "class", "return", "if", "elif", "else", "for", "while", "break", "continue", "pass",
    "import", "from", "as", "with", "try", "except", "finally", "raise", "yield", "lambda",
    "and", "or", "not", "is", "in", "True", "False", "None", "self", "cls", "async", "await",
    "global", "nonlocal", "assert", "del",
];

pub struct PythonSupport;

impl LanguageSupport for PythonSupport {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_python::LANGUAGE.into()
    }

    fn file_extensions(&self) -> &'static [&'static str] {
        &["py"]
    }

    fn is_excluded_file(&self, filename: &str) -> bool {
        filename.starts_with("test_")
            || filename.ends_with("_test.py")
            || filename.ends_with("_tests.py")
            || filename == "conftest.py"
            || filename == "setup.py"
    }

    fn classify_node(&self, node: Node) -> NodeRole {
        match node.kind() {
            "function_definition" => NodeRole::Function,

            "if_statement" | "for_statement" | "while_statement" | "try_statement"
            | "with_statement" => NodeRole::BlockParent,

            "match_statement" => NodeRole::ControlFlow,

            "block" => NodeRole::Block,

            // decorated_definition wraps function/class — recurse through it
            // No TypeAnnotation needed — Python type hints are simple enough
            // that filtering identifiers via is_identifier is sufficient.

            _ => NodeRole::Other,
        }
    }

    fn is_identifier(&self, node: Node) -> bool {
        node.kind() == "identifier"
    }

    fn stop_words(&self) -> &'static [&'static str] {
        STOP_WORDS
    }

    fn extract_function_name(&self, node: Node, source: &[u8]) -> Option<String> {
        let name_node = node.child_by_field_name("name")?;
        let text = name_node.utf8_text(source).unwrap_or("");
        if text.is_empty() {
            None
        } else {
            Some(text.to_string())
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
                "identifier" => {
                    let name = child.utf8_text(source).unwrap_or("").to_string();
                    if !name.is_empty() {
                        params.push(ParamInfo {
                            name,
                            type_: None,
                        });
                    }
                }

                "typed_parameter" => {
                    let name = child
                        .named_child(0)
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

                "default_parameter" => {
                    let name = child
                        .child_by_field_name("name")
                        .map(|n| n.utf8_text(source).unwrap_or("").to_string())
                        .unwrap_or_default();

                    if !name.is_empty() {
                        params.push(ParamInfo {
                            name,
                            type_: None,
                        });
                    }
                }

                "typed_default_parameter" => {
                    let name = child
                        .child_by_field_name("name")
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

                "list_splat_pattern" => {
                    let inner = child
                        .named_child(0)
                        .map(|n| n.utf8_text(source).unwrap_or("").to_string())
                        .unwrap_or_default();
                    if !inner.is_empty() {
                        params.push(ParamInfo {
                            name: format!("*{inner}"),
                            type_: None,
                        });
                    }
                }

                "dictionary_splat_pattern" => {
                    let inner = child
                        .named_child(0)
                        .map(|n| n.utf8_text(source).unwrap_or("").to_string())
                        .unwrap_or_default();
                    if !inner.is_empty() {
                        params.push(ParamInfo {
                            name: format!("**{inner}"),
                            type_: None,
                        });
                    }
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

    fn is_function_exported(&self, name: &str, _parent_exported: bool) -> bool {
        !name.starts_with('_')
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    fn with_functions(source: &str, mut callback: impl FnMut(&str, Node, &[u8])) {
        let lang = PythonSupport;
        let mut parser = Parser::new();
        parser.set_language(&lang.language()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let bytes = source.as_bytes();

        fn walk<'a>(
            node: Node<'a>,
            lang: &PythonSupport,
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
def greet(name: str) -> str:
    greeting = f"Hello, {name}"
    return greeting
"#;
        let mut found = false;
        with_functions(source, |name, node, src| {
            assert_eq!(name, "greet");

            let lang = PythonSupport;
            let params = lang.extract_params(node, src);
            assert_eq!(params.len(), 1);
            assert_eq!(params[0].name, "name");
            assert_eq!(params[0].type_.as_deref(), Some("str"));

            let ret = lang.extract_return_type(node, src);
            assert_eq!(ret.as_deref(), Some("str"));
            found = true;
        });
        assert!(found, "should find greet function");
    }

    #[test]
    fn extracts_method() {
        let source = r#"
class MyClass:
    def process(self, x: int) -> bool:
        result = x > 0
        return result
"#;
        let mut found = false;
        with_functions(source, |name, node, src| {
            if name == "process" {
                let lang = PythonSupport;
                let params = lang.extract_params(node, src);
                assert_eq!(params.len(), 2);
                assert_eq!(params[0].name, "self");
                assert_eq!(params[0].type_, None);
                assert_eq!(params[1].name, "x");
                assert_eq!(params[1].type_.as_deref(), Some("int"));

                let ret = lang.extract_return_type(node, src);
                assert_eq!(ret.as_deref(), Some("bool"));
                found = true;
            }
        });
        assert!(found, "should find process method");
    }

    #[test]
    fn extracts_default_params() {
        let source = r#"
def connect(host: str, port: int = 8080, timeout=30):
    return create_connection(host, port, timeout)
"#;
        let mut found = false;
        with_functions(source, |name, node, src| {
            if name == "connect" {
                let lang = PythonSupport;
                let params = lang.extract_params(node, src);
                assert_eq!(params.len(), 3);
                assert_eq!(params[0].name, "host");
                assert_eq!(params[0].type_.as_deref(), Some("str"));
                assert_eq!(params[1].name, "port");
                assert_eq!(params[2].name, "timeout");
                assert_eq!(params[2].type_, None);
                found = true;
            }
        });
        assert!(found, "should find connect function");
    }

    #[test]
    fn extracts_splat_params() {
        let source = r#"
def variadic(*args, **kwargs):
    for arg in args:
        print(arg)
"#;
        let mut found = false;
        with_functions(source, |name, node, src| {
            if name == "variadic" {
                let lang = PythonSupport;
                let params = lang.extract_params(node, src);
                assert_eq!(params.len(), 2);
                assert_eq!(params[0].name, "*args");
                assert_eq!(params[1].name, "**kwargs");
                found = true;
            }
        });
        assert!(found, "should find variadic function");
    }

    #[test]
    fn extracts_decorated_function() {
        let source = r#"
@staticmethod
def helper(x: int) -> int:
    result = x * 2
    return result
"#;
        let mut found = false;
        with_functions(source, |name, _, _| {
            if name == "helper" {
                found = true;
            }
        });
        assert!(found, "should find decorated function");
    }

    #[test]
    fn skips_tiny_functions() {
        let source = "def tiny(x): return x + 1";
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
    fn private_function_not_exported() {
        let lang = PythonSupport;
        assert!(!lang.is_function_exported("_helper", false));
        assert!(!lang.is_function_exported("__init__", false));
        assert!(lang.is_function_exported("process", false));
    }

    #[test]
    fn excluded_files() {
        let lang = PythonSupport;
        assert!(lang.is_excluded_file("test_utils.py"));
        assert!(lang.is_excluded_file("utils_test.py"));
        assert!(lang.is_excluded_file("conftest.py"));
        assert!(lang.is_excluded_file("setup.py"));
        assert!(!lang.is_excluded_file("utils.py"));
        assert!(!lang.is_excluded_file("main.py"));
    }

    #[test]
    fn classify_node_roles() {
        let source = r#"
def example():
    if True:
        for i in range(10):
            print(i)
"#;
        let lang = PythonSupport;
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

        let fn_node = find_kind(root, "function_definition").unwrap();
        assert_eq!(lang.classify_node(fn_node), NodeRole::Function);

        let if_node = find_kind(root, "if_statement").unwrap();
        assert_eq!(lang.classify_node(if_node), NodeRole::BlockParent);

        let for_node = find_kind(root, "for_statement").unwrap();
        assert_eq!(lang.classify_node(for_node), NodeRole::BlockParent);

        let block_node = find_kind(root, "block").unwrap();
        assert_eq!(lang.classify_node(block_node), NodeRole::Block);
    }
}
