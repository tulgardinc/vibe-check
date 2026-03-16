export interface ParamInfo {
  name: string;
  type: string | null;
}

export interface FunctionChunk {
  /** Deterministic ID: filePath:functionName:startLine */
  id: string;
  filePath: string;
  functionName: string;
  sourceText: string;
  startLine: number;
  endLine: number;
  params: ParamInfo[];
  returnType: string | null;
  isExported: boolean;
  signatureHash: string;
  chunkType: 'function' | 'block';
  /** For blocks: containing function/scope name. Null for top-level blocks and functions. */
  context: string | null;
}

export interface ParsedFile {
  filePath: string;
  chunks: FunctionChunk[];
  parseErrors: string[];
}
