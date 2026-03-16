export interface ModelInfo {
  name: string;
  dimensions: number;
  tier: '7b' | '137m';
}

export interface EmbeddingResult {
  functionId: string;
  embedding: Float32Array;
}

/** Provider-agnostic embedding interface used by pipelines. */
export interface Embedder {
  readonly modelName: string;
  readonly dimensions: number;
  readonly tier: string;
  embedBatch(inputs: string[], onProgress?: (done: number, total: number) => void): Promise<Float32Array[]>;
  embedQuery(input: string): Promise<Float32Array>;
}
