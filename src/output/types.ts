export interface Candidate {
  name: string;
  path: string;
  line: number;
  /** Cosine distance — lower means more similar (0 = identical). */
  distance: number;
  detectionMethod: 'embedding' | 'jscpd' | 'combined';
  source: string;
  signatureHash: string;
  chunkType?: 'function' | 'block';
  context?: string | null;
  jaccardSimilarity?: number;
}

export interface QueryFunction {
  name: string;
  file: string;
  line: number;
  candidates: Candidate[];
  chunkType?: 'function' | 'block';
}

export interface QueryResult {
  queryFunctions: QueryFunction[];
  warnings: string[];
  meta: {
    model: string;
    indexedFunctions: number;
    queryFunctions: number;
    elapsedMs: number;
    indexedBlocks?: number;
  };
}
