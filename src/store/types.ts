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
}

export interface FileRecord {
  filePath: string;
  contentHash: string;
  mtimeMs: number;
  indexedAt: string;
}

export interface IndexMeta {
  modelName: string | null;
  modelDimensions: number | null;
  createdAt: string | null;
  lastIndexedAt: string | null;
}
