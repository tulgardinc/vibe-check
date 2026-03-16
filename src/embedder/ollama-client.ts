import { Ollama } from 'ollama';
import type { ModelInfo } from './types.js';

export function createClient(host?: string): Ollama {
  return new Ollama({ host: host ?? 'http://localhost:11434' });
}

export async function checkHealth(client: Ollama): Promise<boolean> {
  try {
    await client.list();
    return true;
  } catch {
    return false;
  }
}

export async function detectModel(client: Ollama): Promise<ModelInfo> {
  const response = await client.list();
  const names = response.models.map((m) => m.name);

  // Preference order: nomic-embed-code 7B > nomic-embed-code 137M > nomic-embed-text
  const candidates: Array<{ match: (n: string) => boolean; resolve: (n: string) => string; tier: ModelInfo['tier'] }> = [
    { match: (n) => n === 'nomic-embed-code:latest' || n === 'nomic-embed-code', resolve: () => 'nomic-embed-code', tier: '7b' },
    { match: (n) => n.startsWith('nomic-embed-code:137m'), resolve: (n) => n, tier: '137m' },
    { match: (n) => n === 'nomic-embed-text:latest' || n === 'nomic-embed-text', resolve: () => 'nomic-embed-text', tier: '7b' },
    { match: (n) => n.startsWith('nomic-embed-text:'), resolve: (n) => n, tier: '137m' },
  ];

  for (const candidate of candidates) {
    const found = names.find(candidate.match);
    if (found) {
      const modelName = candidate.resolve(found);
      const dimensions = await detectDimensions(client, modelName);
      return { name: modelName, dimensions, tier: candidate.tier };
    }
  }

  throw new Error(
    'No nomic embedding model found in Ollama. Run: ollama pull nomic-embed-text',
  );
}

export async function detectDimensions(
  client: Ollama,
  modelName: string,
): Promise<number> {
  const response = await client.embed({ model: modelName, input: 'test' });
  return response.embeddings[0].length;
}
