export interface ModelInfo {
  name: string;
  dimensions: number;
  tier: '7b' | '137m';
}

export interface EmbeddingResult {
  functionId: string;
  embedding: Float32Array;
}
