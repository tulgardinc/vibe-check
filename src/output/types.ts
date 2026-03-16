export interface Candidate {
  name: string;
  path: string;
  line: number;
  /** Cosine distance — lower means more similar (0 = identical). */
  distance: number;
  detectionMethod: 'embedding' | 'jscpd' | 'combined';
  source: string;
  signatureHash: string;
}

export interface QueryFunction {
  name: string;
  file: string;
  line: number;
  candidates: Candidate[];
}

export interface QueryResult {
  queryFunctions: QueryFunction[];
  warnings: string[];
  meta: {
    model: string;
    indexedFunctions: number;
    queryFunctions: number;
    elapsedMs: number;
  };
}
