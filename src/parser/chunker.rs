use crate::parser::signature::compute_signature_hash;
use crate::parser::types::{ChunkType, FunctionChunk, ParamInfo, ParsedFile};
use crate::util::hash::sha256;
use std::cell::RefCell;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;
use tree_sitter::{Node, Parser};

const MIN_LINES: usize = 3;
const MIN_BLOCK_LINES: usize = 6;

static FUNCTION_NODE_TYPES: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "function_declaration",
        "generator_function_declaration",
        "method_definition",
        "arrow_function",
        "function_expression",
    ]
    .into_iter()
    .collect()
});

static CONTROL_FLOW_TYPES: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
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
});

static BLOCK_PARENT_TYPES: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
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
});

static TYPE_NODE_TYPES: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "type_annotation",
        "type_arguments",
        "type_parameters",
        "as_expression",
        "satisfies_expression",
        "return_type",
    ]
    .into_iter()
    .collect()
});

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

thread_local! {
    static PARSER: RefCell<Parser> = RefCell::new({
        let mut parser = Parser::new();
        let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        parser.set_language(&language).expect("failed to set language");
        parser
    });
}

pub fn parse_file(path: &Path) -> Result<ParsedFile, std::io::Error> {
    let source = fs::read_to_string(path)?;
    let file_path = path.to_string_lossy().to_string();
    Ok(parse_source(&source, &file_path))
}

pub fn parse_source(source: &str, file_path: &str) -> ParsedFile {
    PARSER.with_borrow_mut(|parser| {
        let tree = match parser.parse(source, None) {
            Some(t) => t,
            None => {
                return ParsedFile {
                    file_path: file_path.to_string(),
                    source: source.to_string(),
                    chunks: vec![],
                    parse_errors: vec!["Failed to parse file".into()],
                };
            }
        };

        let func_types = &*FUNCTION_NODE_TYPES;
        let mut chunks = Vec::new();
        let mut parse_errors = Vec::new();

        walk_node(
            tree.root_node(),
            source.as_bytes(),
            file_path,
            &mut chunks,
            &mut parse_errors,
            false,
            func_types,
        );

        ParsedFile {
            file_path: file_path.to_string(),
            source: source.to_string(),
            chunks,
            parse_errors,
        }
    })
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
            if child.kind() == "variable_declarator"
                && let Some(value) = child.child_by_field_name("value")
                && func_types.contains(value.kind())
                && let Some(name_node) = child.child_by_field_name("name")
            {
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

fn format_signature(
    params: &[ParamInfo],
    return_type: Option<&str>,
    chunk_type: ChunkType,
    line_count: usize,
) -> String {
    if chunk_type == ChunkType::Block {
        return format!("<block> ({line_count} lines)");
    }
    let params_str: Vec<String> = params
        .iter()
        .map(|p| match &p.type_ {
            Some(t) => format!("{}: {t}", p.name),
            None => p.name.clone(),
        })
        .collect();
    match return_type {
        Some(rt) => format!("({}) => {rt}", params_str.join(", ")),
        None => format!("({})", params_str.join(", ")),
    }
}

/// Collect identifier tokens from the AST, skipping type annotations.
fn collect_tokens(node: Node, source: &[u8]) -> HashSet<String> {
    let type_nodes = &*TYPE_NODE_TYPES;
    let stops = &*STOP_WORDS;
    let mut tokens = HashSet::new();
    collect_tokens_recursive(node, source, &mut tokens, type_nodes, stops);
    tokens
}

fn collect_tokens_recursive(
    node: Node,
    source: &[u8],
    tokens: &mut HashSet<String>,
    type_nodes: &HashSet<&str>,
    stops: &HashSet<&str>,
) {
    // Skip type system nodes entirely
    if type_nodes.contains(node.kind()) {
        return;
    }

    if node.kind() == "identifier" || node.kind() == "property_identifier" {
        let text = node.utf8_text(source).unwrap_or("");
        let lower = text.to_lowercase();
        if lower.len() > 1 && !stops.contains(lower.as_str()) {
            tokens.insert(lower);
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_tokens_recursive(child, source, tokens, type_nodes, stops);
    }
}

/// Construct a deterministic chunk ID from file path, name, and start line.
fn make_chunk_id(file_path: &str, name: &str, start_line: usize) -> String {
    format!("{file_path}:{name}:{start_line}")
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
    let signature_hash = compute_signature_hash(name, &params, return_type.as_deref());
    let signature = format_signature(&params, return_type.as_deref(), ChunkType::Function, line_count);
    let tokens = collect_tokens(span_node, source);

    Some(FunctionChunk {
        id: make_chunk_id(file_path, name, start_line),
        file_path: file_path.to_string(),
        function_name: name.to_string(),
        source_text,
        start_line,
        end_line,
        params,
        return_type,
        is_exported,
        signature_hash,
        signature,
        tokens,
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
    let block_parents = &*BLOCK_PARENT_TYPES;

    if block_parents.contains(node.kind()) {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "statement_block"
                && is_eligible_block(child)
                && let Some(chunk) = build_block_chunk(child, source, file_path, context_name)
            {
                chunks.push(chunk);
            }
        }
    }

    // Arrow function callbacks in variable declarations
    if node.kind() == "lexical_declaration" || node.kind() == "variable_declaration" {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "variable_declarator"
                && let Some(value) = child.child_by_field_name("value")
                && value.kind() == "arrow_function"
                && let Some(arrow_body) = value.child_by_field_name("body")
                && arrow_body.kind() == "statement_block"
                && is_eligible_block(arrow_body)
                && let Some(chunk) =
                    build_block_chunk(arrow_body, source, file_path, context_name)
            {
                chunks.push(chunk);
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

    let cf_types = &*CONTROL_FLOW_TYPES;
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
    let line_count = end_line - start_line + 1;
    let source_text = block.utf8_text(source).unwrap_or("").to_string();
    let synthetic_name = format!("<block:{start_line}>");
    // Block hashes are content-based (unlike function signature hashes which use
    // name + param types + return type). This means block exclusions become stale
    // when the block's source text changes, even for whitespace-only edits.
    let signature_hash = sha256(&source_text)[..16].to_string();
    let signature = format_signature(&[], None, ChunkType::Block, line_count);
    let tokens = collect_tokens(block, source);

    Some(FunctionChunk {
        id: make_chunk_id(file_path, &synthetic_name, start_line),
        file_path: file_path.to_string(),
        function_name: synthetic_name,
        source_text,
        start_line,
        end_line,
        params: vec![],
        return_type: None,
        is_exported: false,
        signature_hash,
        signature,
        tokens,
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
            .and_then(|n| strip_type_prefix(n.utf8_text(source).unwrap_or("")));

        params.push(ParamInfo {
            name: param_name,
            type_: type_annotation,
        });
    }

    params
}

fn extract_return_type(node: Node, source: &[u8]) -> Option<String> {
    node.child_by_field_name("return_type")
        .and_then(|n| strip_type_prefix(n.utf8_text(source).unwrap_or("")))
}

fn strip_type_prefix(text: &str) -> Option<String> {
    let trimmed = text.strip_prefix(':').unwrap_or(text).trim();

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
        assert_eq!(parsed.chunks[0].signature, "(name: string) => string");
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
    fn signature_hash_is_16_hex() {
        let source = r#"
function testFunc(a: string, b: number): boolean {
    const check = a.length > b;
    return check;
}
"#;
        let parsed = parse_source(source, "test.ts");
        assert_eq!(parsed.chunks[0].signature_hash.len(), 16);
        assert!(parsed.chunks[0]
            .signature_hash
            .chars()
            .all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn tokens_extracted_from_ast() {
        let source = r#"
function process(items: string[], count: number): boolean {
    const filtered = items.filter(x => x.length > count);
    return filtered.length > 0;
}
"#;
        let parsed = parse_source(source, "test.ts");
        let tokens = &parsed.chunks[0].tokens;
        // Identifiers from value positions should be present
        assert!(tokens.contains("items"));
        assert!(tokens.contains("filtered"));
        assert!(tokens.contains("count"));
        assert!(tokens.contains("length"));
        // Stop words should be absent
        assert!(!tokens.contains("const"));
        assert!(!tokens.contains("return"));
        assert!(!tokens.contains("function"));
    }

    #[test]
    fn parsed_file_contains_source() {
        let source = "function foo() { return 1; }";
        let parsed = parse_source(source, "test.ts");
        assert_eq!(parsed.source, source);
    }
}
