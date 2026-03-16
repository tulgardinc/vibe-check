use crate::parser::signature::compute_signature_hash;
use crate::parser::types::{ChunkType, FunctionChunk, ParamInfo, ParsedFile};
use crate::util::hash::sha256;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use tree_sitter::{Node, Parser};

const MIN_LINES: usize = 3;
const MIN_BLOCK_LINES: usize = 6;

fn function_node_types() -> HashSet<&'static str> {
    [
        "function_declaration",
        "generator_function_declaration",
        "method_definition",
        "arrow_function",
        "function_expression",
    ]
    .into_iter()
    .collect()
}

fn control_flow_types() -> HashSet<&'static str> {
    [
        "if_statement",
        "for_statement",
        "for_in_statement",
        "while_statement",
        "do_statement",
        "try_statement",
        "switch_statement",
    ]
    .into_iter()
    .collect()
}

fn block_parent_types() -> HashSet<&'static str> {
    [
        "if_statement",
        "for_statement",
        "for_in_statement",
        "while_statement",
        "do_statement",
        "try_statement",
    ]
    .into_iter()
    .collect()
}

pub fn parse_file(path: &Path) -> Result<ParsedFile, std::io::Error> {
    let source = fs::read_to_string(path)?;
    let file_path = path.to_string_lossy().to_string();
    Ok(parse_source(&source, &file_path))
}

pub fn parse_source(source: &str, file_path: &str) -> ParsedFile {
    let mut parser = Parser::new();
    let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
    parser.set_language(&language).expect("failed to set language");

    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => {
            return ParsedFile {
                file_path: file_path.to_string(),
                chunks: vec![],
                parse_errors: vec!["Failed to parse file".into()],
            };
        }
    };

    let func_types = function_node_types();
    let mut chunks = Vec::new();
    let mut parse_errors = Vec::new();

    walk_node(
        tree.root_node(),
        source.as_bytes(),
        file_path,
        &mut chunks,
        &mut parse_errors,
        false,
        &func_types,
    );

    ParsedFile {
        file_path: file_path.to_string(),
        chunks,
        parse_errors,
    }
}

fn walk_node(
    node: Node,
    source: &[u8],
    file_path: &str,
    chunks: &mut Vec<FunctionChunk>,
    errors: &mut Vec<String>,
    parent_exported: bool,
    func_types: &HashSet<&str>,
) {
    if node.kind() == "ERROR" {
        let text = node.utf8_text(source).unwrap_or("");
        let preview: String = text.chars().take(80).collect();
        errors.push(format!(
            "Parse error at line {}: {}",
            node.start_position().row + 1,
            preview
        ));
    }

    let is_export = node.kind() == "export_statement"
        || node.kind() == "export_default_declaration";

    // Function node
    if func_types.contains(node.kind()) {
        if let Some(name) = extract_function_name(node, source) {
            if let Some(chunk) =
                build_chunk(&name, node, node, source, file_path, parent_exported)
            {
                chunks.push(chunk);
            }
            extract_blocks(node, source, file_path, &name, chunks, func_types);
        }
        return; // don't recurse into function bodies
    }

    // Variable declaration with arrow function or function expression
    if node.kind() == "lexical_declaration" || node.kind() == "variable_declaration" {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "variable_declarator" {
                if let Some(value) = child.child_by_field_name("value") {
                    if func_types.contains(value.kind()) {
                        if let Some(name_node) = child.child_by_field_name("name") {
                            let name = name_node.utf8_text(source).unwrap_or("").to_string();
                            if !name.is_empty() {
                                if let Some(chunk) = build_chunk(
                                    &name,
                                    value,
                                    node,
                                    source,
                                    file_path,
                                    parent_exported,
                                ) {
                                    chunks.push(chunk);
                                }
                                extract_blocks(value, source, file_path, &name, chunks, func_types);
                            }
                        }
                    }
                }
            }
        }
        return;
    }

    // Recurse into children
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        walk_node(
            child,
            source,
            file_path,
            chunks,
            errors,
            is_export || parent_exported,
            func_types,
        );
    }
}

fn build_chunk(
    name: &str,
    func_node: Node,
    span_node: Node,
    source: &[u8],
    file_path: &str,
    is_exported: bool,
) -> Option<FunctionChunk> {
    let start_row = span_node.start_position().row;
    let end_row = span_node.end_position().row;
    let line_count = end_row - start_row + 1;

    if line_count < MIN_LINES {
        return None;
    }

    let params = extract_params(func_node, source);
    let return_type = extract_return_type(func_node, source);
    let start_line = start_row + 1;
    let end_line = end_row + 1;
    let source_text = span_node.utf8_text(source).unwrap_or("").to_string();
    let signature_hash =
        compute_signature_hash(name, &params, return_type.as_deref());

    Some(FunctionChunk {
        id: format!("{file_path}:{name}:{start_line}"),
        file_path: file_path.to_string(),
        function_name: name.to_string(),
        source_text,
        start_line,
        end_line,
        params,
        return_type,
        is_exported,
        signature_hash,
        chunk_type: ChunkType::Function,
        context: None,
    })
}

fn extract_blocks(
    func_node: Node,
    source: &[u8],
    file_path: &str,
    context_name: &str,
    chunks: &mut Vec<FunctionChunk>,
    func_types: &HashSet<&str>,
) {
    if let Some(body) = func_node.child_by_field_name("body") {
        walk_for_blocks(body, source, file_path, context_name, chunks, func_types);
    }
}

fn walk_for_blocks(
    node: Node,
    source: &[u8],
    file_path: &str,
    context_name: &str,
    chunks: &mut Vec<FunctionChunk>,
    func_types: &HashSet<&str>,
) {
    let block_parents = block_parent_types();

    if block_parents.contains(node.kind()) {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "statement_block" && is_eligible_block(child) {
                if let Some(chunk) = build_block_chunk(child, source, file_path, context_name) {
                    chunks.push(chunk);
                }
            }
        }
    }

    // Arrow function callbacks in variable declarations
    if node.kind() == "lexical_declaration" || node.kind() == "variable_declaration" {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "variable_declarator" {
                if let Some(value) = child.child_by_field_name("value") {
                    if value.kind() == "arrow_function" {
                        if let Some(arrow_body) = value.child_by_field_name("body") {
                            if arrow_body.kind() == "statement_block"
                                && is_eligible_block(arrow_body)
                            {
                                if let Some(chunk) =
                                    build_block_chunk(arrow_body, source, file_path, context_name)
                                {
                                    chunks.push(chunk);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Recurse, but skip nested function boundaries
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if !func_types.contains(child.kind()) {
            walk_for_blocks(child, source, file_path, context_name, chunks, func_types);
        }
    }
}

fn is_eligible_block(block: Node) -> bool {
    let line_count = block.end_position().row - block.start_position().row + 1;
    if line_count < MIN_BLOCK_LINES {
        return false;
    }

    let cf_types = control_flow_types();
    let mut statement_count = 0;
    let mut has_control_flow = false;

    let mut cursor = block.walk();
    for child in block.named_children(&mut cursor) {
        statement_count += 1;
        if cf_types.contains(child.kind()) {
            has_control_flow = true;
        }
    }

    has_control_flow && statement_count >= 2
}

fn build_block_chunk(
    block: Node,
    source: &[u8],
    file_path: &str,
    context_name: &str,
) -> Option<FunctionChunk> {
    let start_line = block.start_position().row + 1;
    let end_line = block.end_position().row + 1;
    let source_text = block.utf8_text(source).unwrap_or("").to_string();
    let synthetic_name = format!("<block:{start_line}>");
    let signature_hash = sha256(&source_text)[..8].to_string();

    Some(FunctionChunk {
        id: format!("{file_path}:{synthetic_name}:{start_line}"),
        file_path: file_path.to_string(),
        function_name: synthetic_name,
        source_text,
        start_line,
        end_line,
        params: vec![],
        return_type: None,
        is_exported: false,
        signature_hash,
        chunk_type: ChunkType::Block,
        context: Some(context_name.to_string()),
    })
}

fn extract_function_name(node: Node, source: &[u8]) -> Option<String> {
    if let Some(name_node) = node.child_by_field_name("name") {
        let text = name_node.utf8_text(source).unwrap_or("").to_string();
        if !text.is_empty() {
            return Some(text);
        }
    }
    None
}

fn extract_params(node: Node, source: &[u8]) -> Vec<ParamInfo> {
    let params_node = match node.child_by_field_name("parameters") {
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
            .map(|n| strip_type_prefix(n.utf8_text(source).unwrap_or("")))
            .flatten();

        params.push(ParamInfo {
            name: param_name,
            type_: type_annotation,
        });
    }

    params
}

fn extract_return_type(node: Node, source: &[u8]) -> Option<String> {
    node.child_by_field_name("return_type")
        .map(|n| strip_type_prefix(n.utf8_text(source).unwrap_or("")))
        .flatten()
}

fn strip_type_prefix(text: &str) -> Option<String> {
    let trimmed = if text.starts_with(':') {
        text[1..].trim()
    } else {
        text.trim()
    };

    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_function_declaration() {
        let source = r#"
function greet(name: string): string {
    const greeting = "Hello, " + name;
    return greeting;
}
"#;
        let parsed = parse_source(source, "test.ts");
        assert_eq!(parsed.chunks.len(), 1);
        assert_eq!(parsed.chunks[0].function_name, "greet");
        assert_eq!(parsed.chunks[0].params.len(), 1);
        assert_eq!(parsed.chunks[0].params[0].name, "name");
        assert_eq!(
            parsed.chunks[0].params[0].type_.as_deref(),
            Some("string")
        );
        assert_eq!(parsed.chunks[0].return_type.as_deref(), Some("string"));
        assert_eq!(parsed.chunks[0].chunk_type, ChunkType::Function);
    }

    #[test]
    fn extracts_arrow_function_in_const() {
        let source = r#"
const add = (a: number, b: number): number => {
    const sum = a + b;
    return sum;
}
"#;
        let parsed = parse_source(source, "test.ts");
        assert_eq!(parsed.chunks.len(), 1);
        assert_eq!(parsed.chunks[0].function_name, "add");
        assert_eq!(parsed.chunks[0].params.len(), 2);
    }

    #[test]
    fn skips_tiny_functions() {
        let source = r#"
function tiny(x: number) { return x; }
"#;
        let parsed = parse_source(source, "test.ts");
        assert_eq!(parsed.chunks.len(), 0);
    }

    #[test]
    fn detects_exported_functions() {
        let source = r#"
export function myExport(x: number): number {
    const result = x * 2;
    return result;
}
"#;
        let parsed = parse_source(source, "test.ts");
        assert_eq!(parsed.chunks.len(), 1);
        assert!(parsed.chunks[0].is_exported);
    }

    #[test]
    fn deterministic_id() {
        let source = r#"
function foo(x: number): void {
    console.log(x);
    return;
}
"#;
        let parsed = parse_source(source, "src/app.ts");
        assert_eq!(parsed.chunks[0].id, "src/app.ts:foo:2");
    }

    #[test]
    fn extracts_class_methods() {
        let source = r#"
class MyClass {
    myMethod(x: number): string {
        const result = String(x);
        return result;
    }
}
"#;
        let parsed = parse_source(source, "test.ts");
        assert_eq!(parsed.chunks.len(), 1);
        assert_eq!(parsed.chunks[0].function_name, "myMethod");
    }

    #[test]
    fn signature_hash_is_8_hex() {
        let source = r#"
function testFunc(a: string, b: number): boolean {
    const check = a.length > b;
    return check;
}
"#;
        let parsed = parse_source(source, "test.ts");
        assert_eq!(parsed.chunks[0].signature_hash.len(), 8);
        assert!(parsed.chunks[0]
            .signature_hash
            .chars()
            .all(|c| c.is_ascii_hexdigit()));
    }
}
