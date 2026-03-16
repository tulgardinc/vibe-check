export interface Candidate {
  name: string;
  path: string;
  line: number;
  similarity: number;
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
  query_functions: QueryFunction[];
  warnings: string[];
  meta: {
    model: string;
    indexed_functions: number;
    query_functions: number;
    elapsed_ms: number;
  };
}
