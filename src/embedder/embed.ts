import type { Ollama } from 'ollama';
import type { FunctionChunk } from '../parser/types.js';
import type { EmbeddingResult } from './types.js';

const BATCH_SIZE = 32;

export async function embedChunks(
  client: Ollama,
  modelName: string,
  chunks: FunctionChunk[],
  onProgress?: (done: number, total: number) => void,
): Promise<EmbeddingResult[]> {
  const results: EmbeddingResult[] = [];

  for (let i = 0; i < chunks.length; i += BATCH_SIZE) {
    const batch = chunks.slice(i, i + BATCH_SIZE);
    const inputs = batch.map((c) => c.sourceText);

    const response = await client.embed({ model: modelName, input: inputs });

    for (let j = 0; j < batch.length; j++) {
      results.push({
        functionId: batch[j].id,
        embedding: new Float32Array(response.embeddings[j]),
      });
    }

    onProgress?.(Math.min(i + BATCH_SIZE, chunks.length), chunks.length);
  }

  return results;
}

export async function embedQuery(
  client: Ollama,
  modelName: string,
  source: string,
): Promise<Float32Array> {
  const response = await client.embed({
    model: modelName,
    input: `search_query: ${source}`,
  });
  return new Float32Array(response.embeddings[0]);
}
