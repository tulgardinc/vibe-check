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
}

export interface ParsedFile {
  filePath: string;
  chunks: FunctionChunk[];
  parseErrors: string[];
}
