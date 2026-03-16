import fs from 'node:fs/promises';
import Parser from 'tree-sitter';
// @ts-expect-error no type declarations for tree-sitter-typescript
import TypeScript from 'tree-sitter-typescript/bindings/node/typescript.js';
import { computeSignatureHash } from './signature.js';
import { sha256 } from '../util/hash.js';
import type { FunctionChunk, ParamInfo, ParsedFile } from './types.js';

const parser = new Parser();
parser.setLanguage(TypeScript);

const FUNCTION_NODE_TYPES = new Set([
  'function_declaration',
  'generator_function_declaration',
  'method_definition',
  'arrow_function',
  'function_expression',
]);

/** Minimum line count for a function to be indexed. */
const MIN_LINES = 3;

/** Minimum line count for a block to be indexed. */
const MIN_BLOCK_LINES = 6;

/** Control flow node types that indicate non-trivial logic. */
const CONTROL_FLOW_TYPES = new Set([
  'if_statement',
  'for_statement',
  'for_in_statement',
  'while_statement',
  'do_statement',
  'try_statement',
  'switch_statement',
]);

/** Parent node types whose statement_block children are eligible for extraction. */
const BLOCK_PARENT_TYPES = new Set([
  'if_statement',
  'for_statement',
  'for_in_statement',
  'while_statement',
  'do_statement',
  'try_statement',
]);

export async function parseFile(filePath: string): Promise<ParsedFile> {
  const source = await fs.readFile(filePath, 'utf-8');
  return parseSource(source, filePath);
}

export function parseSource(source: string, filePath: string): ParsedFile {
  const tree = parser.parse(source);
  const chunks: FunctionChunk[] = [];
  const parseErrors: string[] = [];

  walkNode(tree.rootNode, filePath, chunks, parseErrors, false);

  return { filePath, chunks, parseErrors };
}

function walkNode(
  node: Parser.SyntaxNode,
  filePath: string,
  chunks: FunctionChunk[],
  errors: string[],
  parentExported: boolean,
): void {
  if (node.type === 'ERROR') {
    errors.push(
      `Parse error at line ${node.startPosition.row + 1}: ${node.text.slice(0, 80)}`,
    );
  }

  // Check if this node is an export statement wrapping a function
  const isExportStatement =
    node.type === 'export_statement' || node.type === 'export_default_declaration';

  if (FUNCTION_NODE_TYPES.has(node.type)) {
    const name = extractFunctionName(node);
    if (name) {
      const chunk = buildChunk(name, node, node, filePath, parentExported);
      if (chunk) chunks.push(chunk);
      // Scan body for eligible blocks
      extractBlocks(node, filePath, name, chunks);
    }
    // Don't recurse into function bodies for nested functions
    return;
  }

  // Handle variable declarations like `const foo = () => {}`
  if (node.type === 'lexical_declaration' || node.type === 'variable_declaration') {
    for (const declarator of node.namedChildren) {
      if (declarator.type === 'variable_declarator') {
        const value = declarator.childForFieldName('value');
        if (
          value &&
          (value.type === 'arrow_function' || value.type === 'function_expression')
        ) {
          const name = declarator.childForFieldName('name')?.text ?? null;
          if (name) {
            const chunk = buildChunk(name, value, node, filePath, parentExported);
            if (chunk) chunks.push(chunk);
            // Scan body for eligible blocks
            extractBlocks(value, filePath, name, chunks);
          }
        }
      }
    }
    return;
  }

  for (const child of node.namedChildren) {
    walkNode(child, filePath, chunks, errors, isExportStatement || parentExported);
  }
}

/**
 * Build a FunctionChunk from a function node.
 * @param funcNode — the arrow_function/function_declaration node (used for params/return type)
 * @param spanNode — the node whose text span defines the chunk boundaries (may differ for assignments)
 */
function buildChunk(
  name: string,
  funcNode: Parser.SyntaxNode,
  spanNode: Parser.SyntaxNode,
  filePath: string,
  isExported: boolean,
): FunctionChunk | null {
  const lineCount = spanNode.endPosition.row - spanNode.startPosition.row + 1;
  if (lineCount < MIN_LINES) return null;

  const params = extractParams(funcNode);
  const returnType = extractReturnType(funcNode);
  const startLine = spanNode.startPosition.row + 1;
  const endLine = spanNode.endPosition.row + 1;

  return {
    id: `${filePath}:${name}:${startLine}`,
    filePath,
    functionName: name,
    sourceText: spanNode.text,
    startLine,
    endLine,
    params,
    returnType,
    isExported,
    signatureHash: computeSignatureHash(name, params, returnType),
    chunkType: 'function',
    context: null,
  };
}

/**
 * Walk a function body to find eligible block-level chunks.
 * A block is eligible if: ≥ MIN_BLOCK_LINES lines, ≥ 1 control flow node, ≥ 2 statements.
 */
function extractBlocks(
  funcNode: Parser.SyntaxNode,
  filePath: string,
  contextName: string,
  chunks: FunctionChunk[],
): void {
  const body = funcNode.childForFieldName('body');
  if (!body) return;

  walkForBlocks(body, filePath, contextName, chunks);
}

function walkForBlocks(
  node: Parser.SyntaxNode,
  filePath: string,
  contextName: string,
  chunks: FunctionChunk[],
): void {
  // Check statement_block children of control flow nodes
  if (BLOCK_PARENT_TYPES.has(node.type)) {
    for (const child of node.namedChildren) {
      if (child.type === 'statement_block' && isEligibleBlock(child)) {
        const blockChunk = buildBlockChunk(child, filePath, contextName);
        if (blockChunk) chunks.push(blockChunk);
      }
    }
  }

  // Check for arrow function callbacks assigned to variables inside function bodies
  if (node.type === 'lexical_declaration' || node.type === 'variable_declaration') {
    for (const declarator of node.namedChildren) {
      if (declarator.type === 'variable_declarator') {
        const value = declarator.childForFieldName('value');
        if (value && value.type === 'arrow_function') {
          const arrowBody = value.childForFieldName('body');
          if (arrowBody && arrowBody.type === 'statement_block' && isEligibleBlock(arrowBody)) {
            const blockChunk = buildBlockChunk(arrowBody, filePath, contextName);
            if (blockChunk) chunks.push(blockChunk);
          }
        }
      }
    }
  }

  // Recurse into children (but not into nested function declarations)
  for (const child of node.namedChildren) {
    if (!FUNCTION_NODE_TYPES.has(child.type)) {
      walkForBlocks(child, filePath, contextName, chunks);
    }
  }
}

function isEligibleBlock(block: Parser.SyntaxNode): boolean {
  const lineCount = block.endPosition.row - block.startPosition.row + 1;
  if (lineCount < MIN_BLOCK_LINES) return false;

  let statementCount = 0;
  let hasControlFlow = false;

  for (const child of block.namedChildren) {
    statementCount++;
    if (CONTROL_FLOW_TYPES.has(child.type)) {
      hasControlFlow = true;
    }
  }

  return hasControlFlow && statementCount >= 2;
}

function buildBlockChunk(
  block: Parser.SyntaxNode,
  filePath: string,
  contextName: string,
): FunctionChunk | null {
  const startLine = block.startPosition.row + 1;
  const endLine = block.endPosition.row + 1;
  const syntheticName = `<block:${startLine}>`;

  return {
    id: `${filePath}:${syntheticName}:${startLine}`,
    filePath,
    functionName: syntheticName,
    sourceText: block.text,
    startLine,
    endLine,
    params: [],
    returnType: null,
    isExported: false,
    signatureHash: sha256(block.text).slice(0, 8),
    chunkType: 'block',
    context: contextName,
  };
}

function extractFunctionName(node: Parser.SyntaxNode): string | null {
  // function_declaration, generator_function_declaration
  const nameNode = node.childForFieldName('name');
  if (nameNode) return nameNode.text;

  // method_definition
  if (node.type === 'method_definition') {
    const methodName = node.childForFieldName('name');
    return methodName?.text ?? null;
  }

  return null;
}

function extractParams(node: Parser.SyntaxNode): ParamInfo[] {
  const paramsNode = node.childForFieldName('parameters');
  if (!paramsNode) return [];

  const params: ParamInfo[] = [];
  for (const child of paramsNode.namedChildren) {
    if (
      child.type === 'required_parameter' ||
      child.type === 'optional_parameter' ||
      child.type === 'rest_parameter'
    ) {
      const rawName = child.childForFieldName('pattern')?.text
        ?? child.childForFieldName('name')?.text
        ?? child.text;
      const paramName = child.type === 'rest_parameter' ? `...${rawName}` : rawName;
      const typeAnnotation = child.childForFieldName('type');
      params.push({
        name: paramName,
        type: stripTypePrefix(typeAnnotation?.text ?? null),
      });
    }
  }
  return params;
}

function extractReturnType(node: Parser.SyntaxNode): string | null {
  const returnType = node.childForFieldName('return_type');
  if (!returnType) return null;
  return stripTypePrefix(returnType.text);
}

function stripTypePrefix(text: string | null): string | null {
  if (text == null) return null;
  const trimmed = text.startsWith(':') ? text.slice(1).trim() : text.trim();
  return trimmed || null;
}
