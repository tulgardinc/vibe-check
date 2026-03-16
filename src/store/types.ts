export interface StoredFunction {
  id: string;
  filePath: string;
  functionName: string;
  sourceText: string;
  startLine: number;
  endLine: number;
  paramsJson: string;
  returnType: string | null;
  isExported: boolean;
  signatureHash: string;
  contentHash: string;
  embedding: Buffer | null;
  chunkType: 'function' | 'block';
  context: string | null;
}

export interface FileRecord {
  filePath: string;
  contentHash: string;
  mtimeMs: number;
  indexedAt: string;
}
