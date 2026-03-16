import fs from 'node:fs/promises';
import Parser from 'tree-sitter';
// @ts-expect-error no type declarations for tree-sitter-typescript
import TypeScript from 'tree-sitter-typescript/bindings/node/typescript.js';
import { computeSignatureHash } from './signature.js';
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

const MIN_LINES = 3;

export async function parseFile(filePath: string): Promise<ParsedFile> {
  const source = await fs.readFile(filePath, 'utf-8');
  return parseSource(source, filePath);
}

export function parseSource(source: string, filePath: string): ParsedFile {
  const tree = parser.parse(source);
  const chunks: FunctionChunk[] = [];
  const parseErrors: string[] = [];

  walkNode(tree.rootNode, filePath, source, chunks, parseErrors, false);

  return { filePath, chunks, parseErrors };
}

function walkNode(
  node: Parser.SyntaxNode,
  filePath: string,
  source: string,
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
    const chunk = extractChunk(node, filePath, source, parentExported);
    if (chunk) {
      chunks.push(chunk);
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
          const nameNode = declarator.childForFieldName('name');
          const name = nameNode?.text ?? null;
          if (name) {
            const chunk = extractChunkFromAssignment(
              name,
              value,
              node,
              filePath,
              source,
              parentExported,
            );
            if (chunk) {
              chunks.push(chunk);
            }
          }
        }
      }
    }
    return;
  }

  for (const child of node.namedChildren) {
    walkNode(child, filePath, source, chunks, errors, isExportStatement || parentExported);
  }
}

function extractChunk(
  node: Parser.SyntaxNode,
  filePath: string,
  source: string,
  isExported: boolean,
): FunctionChunk | null {
  const name = extractFunctionName(node);
  if (!name) return null; // Skip anonymous functions

  const lineCount = node.endPosition.row - node.startPosition.row + 1;
  if (lineCount < MIN_LINES) return null;

  const params = extractParams(node);
  const returnType = extractReturnType(node);
  const sourceText = node.text;
  const startLine = node.startPosition.row + 1;
  const endLine = node.endPosition.row + 1;
  const signatureHash = computeSignatureHash(name, params, returnType);

  return {
    id: `${filePath}:${name}:${startLine}`,
    filePath,
    functionName: name,
    sourceText,
    startLine,
    endLine,
    params,
    returnType,
    isExported,
    signatureHash,
  };
}

function extractChunkFromAssignment(
  name: string,
  valueNode: Parser.SyntaxNode,
  declarationNode: Parser.SyntaxNode,
  filePath: string,
  source: string,
  isExported: boolean,
): FunctionChunk | null {
  const lineCount =
    declarationNode.endPosition.row - declarationNode.startPosition.row + 1;
  if (lineCount < MIN_LINES) return null;

  const params = extractParams(valueNode);
  const returnType = extractReturnType(valueNode);
  const sourceText = declarationNode.text;
  const startLine = declarationNode.startPosition.row + 1;
  const endLine = declarationNode.endPosition.row + 1;
  const signatureHash = computeSignatureHash(name, params, returnType);

  return {
    id: `${filePath}:${name}:${startLine}`,
    filePath,
    functionName: name,
    sourceText,
    startLine,
    endLine,
    params,
    returnType,
    isExported,
    signatureHash,
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
      child.type === 'optional_parameter'
    ) {
      const paramName = child.childForFieldName('pattern')?.text
        ?? child.childForFieldName('name')?.text
        ?? child.text;
      const typeAnnotation = child.childForFieldName('type');
      params.push({
        name: paramName,
        type: stripTypePrefix(typeAnnotation?.text ?? null),
      });
    } else if (child.type === 'rest_parameter') {
      const paramName = child.childForFieldName('pattern')?.text
        ?? child.childForFieldName('name')?.text
        ?? child.text;
      const typeAnnotation = child.childForFieldName('type');
      params.push({
        name: `...${paramName}`,
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
