import type { Ollama } from 'ollama';
import type { Embedder } from './types.js';

const BATCH_SIZE = 32;

/** Ollama-backed implementation of the Embedder interface. */
export class OllamaEmbedder implements Embedder {
  constructor(
    private client: Ollama,
    public readonly modelName: string,
    public readonly dimensions: number,
    public readonly tier: string,
  ) {}

  async embedBatch(
    inputs: string[],
    onProgress?: (done: number, total: number) => void,
  ): Promise<Float32Array[]> {
    const results: Float32Array[] = [];

    for (let i = 0; i < inputs.length; i += BATCH_SIZE) {
      const batch = inputs.slice(i, i + BATCH_SIZE);
      const response = await this.client.embed({ model: this.modelName, input: batch });

      for (const embedding of response.embeddings) {
        results.push(new Float32Array(embedding));
      }

      onProgress?.(Math.min(i + BATCH_SIZE, inputs.length), inputs.length);
    }

    return results;
  }

  async embedQuery(input: string): Promise<Float32Array> {
    const response = await this.client.embed({
      model: this.modelName,
      input: `search_query: ${input}`,
    });
    return new Float32Array(response.embeddings[0]);
  }
}
