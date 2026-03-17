use crate::parser::language::{LanguageSupport, NodeRole};
use crate::parser::registry;
use crate::parser::signature::compute_signature_hash;
use crate::parser::types::{ChunkType, FunctionChunk, ParamInfo, ParsedFile};
use crate::util::hash::sha256;
use std::cell::RefCell;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use tree_sitter::{Node, Parser};

const MIN_LINES: usize = 3;
const MIN_BLOCK_LINES: usize = 6;

thread_local! {
    static PARSER: RefCell<Parser> = RefCell::new(Parser::new());
}

pub fn parse_file(path: &Path) -> Result<ParsedFile, std::io::Error> {
    let source = fs::read_to_string(path)?;
    let file_path = path.to_string_lossy().to_string();
    Ok(parse_source(&source, &file_path))
}

pub fn parse_source(source: &str, file_path: &str) -> ParsedFile {
    match registry::language_for_file(file_path) {
        Some(lang) => parse_source_with_lang(source, file_path, lang),
        None => ParsedFile {
            file_path: file_path.to_string(),
            source: source.to_string(),
            chunks: vec![],
            parse_errors: vec![format!("No language support for {file_path}")],
        },
    }
}

fn parse_source_with_lang(
    source: &str,
    file_path: &str,
    lang: &dyn LanguageSupport,
) -> ParsedFile {
    PARSER.with_borrow_mut(|parser| {
        parser
            .set_language(&lang.language())
            .expect("failed to set language");

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

        let mut chunks = Vec::new();
        let mut parse_errors = Vec::new();

        walk_node(
            tree.root_node(),
            source.as_bytes(),
            file_path,
            &mut chunks,
            &mut parse_errors,
            false,
            lang,
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
    lang: &dyn LanguageSupport,
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

    let role = lang.classify_node(node);
    let is_export = role == NodeRole::Export;

    if role == NodeRole::Function {
        if let Some(name) = lang.extract_function_name(node, source) {
            let exported = lang.is_function_exported(&name, parent_exported);
            if let Some(chunk) =
                build_chunk(&name, node, source, file_path, exported, lang)
            {
                chunks.push(chunk);
            }
            if let Some(body) = lang.function_body(node) {
                walk_for_blocks(body, source, file_path, &name, chunks, lang);
            }
        }
        return; // don't recurse into function bodies
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
            lang,
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
fn collect_tokens(node: Node, source: &[u8], lang: &dyn LanguageSupport) -> HashSet<String> {
    let stops: HashSet<&str> = lang.stop_words().iter().copied().collect();
    let mut tokens = HashSet::new();
    collect_tokens_recursive(node, source, &mut tokens, lang, &stops);
    tokens
}

fn collect_tokens_recursive(
    node: Node,
    source: &[u8],
    tokens: &mut HashSet<String>,
    lang: &dyn LanguageSupport,
    stops: &HashSet<&str>,
) {
    // Skip type system nodes entirely
    if lang.classify_node(node) == NodeRole::TypeAnnotation {
        return;
    }

    if lang.is_identifier(node) {
        let text = node.utf8_text(source).unwrap_or("");
        let lower = text.to_lowercase();
        if lower.len() > 1 && !stops.contains(lower.as_str()) {
            tokens.insert(lower);
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_tokens_recursive(child, source, tokens, lang, stops);
    }
}

/// Construct a deterministic chunk ID from file path, name, and start line.
fn make_chunk_id(file_path: &str, name: &str, start_line: usize) -> String {
    format!("{file_path}:{name}:{start_line}")
}

fn build_chunk(
    name: &str,
    span_node: Node,
    source: &[u8],
    file_path: &str,
    is_exported: bool,
    lang: &dyn LanguageSupport,
) -> Option<FunctionChunk> {
    let start_row = span_node.start_position().row;
    let end_row = span_node.end_position().row;
    let line_count = end_row - start_row + 1;

    if line_count < MIN_LINES {
        return None;
    }

    let params = lang.extract_params(span_node, source);
    let return_type = lang.extract_return_type(span_node, source);
    let start_line = start_row + 1;
    let end_line = end_row + 1;
    let source_text = span_node.utf8_text(source).unwrap_or("").to_string();
    let signature_hash = compute_signature_hash(name, &params, return_type.as_deref());
    let signature =
        format_signature(&params, return_type.as_deref(), ChunkType::Function, line_count);
    let tokens = collect_tokens(span_node, source, lang);

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

fn walk_for_blocks(
    node: Node,
    source: &[u8],
    file_path: &str,
    context_name: &str,
    chunks: &mut Vec<FunctionChunk>,
    lang: &dyn LanguageSupport,
) {
    let role = lang.classify_node(node);

    if role.is_block_parent() {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if lang.classify_node(child) == NodeRole::Block
                && is_eligible_block(child, lang)
                && let Some(chunk) = build_block_chunk(child, source, file_path, context_name, lang)
            {
                chunks.push(chunk);
            }
        }
    }

    // Recurse, but skip nested function boundaries.
    // For nested functions (e.g. arrow callbacks in variable declarations),
    // check if the function body itself is an eligible block.
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        let child_role = lang.classify_node(child);
        if child_role == NodeRole::Function {
            if let Some(body) = lang.function_body(child) {
                if lang.classify_node(body) == NodeRole::Block
                    && is_eligible_block(body, lang)
                    && let Some(chunk) =
                        build_block_chunk(body, source, file_path, context_name, lang)
                {
                    chunks.push(chunk);
                }
            }
        } else {
            walk_for_blocks(child, source, file_path, context_name, chunks, lang);
        }
    }
}

fn is_eligible_block(block: Node, lang: &dyn LanguageSupport) -> bool {
    let line_count = block.end_position().row - block.start_position().row + 1;
    if line_count < MIN_BLOCK_LINES {
        return false;
    }

    let mut statement_count = 0;
    let mut has_control_flow = false;

    let mut cursor = block.walk();
    for child in block.named_children(&mut cursor) {
        statement_count += 1;
        if lang.classify_node(child).is_control_flow() {
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
    lang: &dyn LanguageSupport,
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
    let tokens = collect_tokens(block, source, lang);

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
