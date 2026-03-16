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

  // Prefer 7B (default tag), fall back to 137M
  const has7b = names.some(
    (n) => n === 'nomic-embed-code:latest' || n === 'nomic-embed-code',
  );
  const has137m = names.some((n) => n.startsWith('nomic-embed-code:137m'));

  if (has7b) {
    const modelName = 'nomic-embed-code';
    const dimensions = await detectDimensions(client, modelName);
    return { name: modelName, dimensions, tier: '7b' };
  }

  if (has137m) {
    const modelName = names.find((n) => n.startsWith('nomic-embed-code:137m'))!;
    const dimensions = await detectDimensions(client, modelName);
    return { name: modelName, dimensions, tier: '137m' };
  }

  throw new Error(
    'No nomic-embed-code model found in Ollama. Run: ollama pull nomic-embed-code',
  );
}

export async function detectDimensions(
  client: Ollama,
  modelName: string,
): Promise<number> {
  const response = await client.embed({ model: modelName, input: 'test' });
  return response.embeddings[0].length;
}
