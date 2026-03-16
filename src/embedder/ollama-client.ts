import { spawn } from 'node:child_process';
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

  const available = names.length > 0
    ? `\n  Models currently in Ollama: ${names.join(', ')}`
    : '\n  No models are currently installed in Ollama.';

  throw new Error(
    `No compatible embedding model found in Ollama.${available}\n\n` +
    `  codeuse needs a Nomic embedding model. Choose one:\n\n` +
    `    ollama pull nomic-embed-text      (recommended, 274 MB, works on CPU and GPU)\n` +
    `    ollama pull nomic-embed-code      (code-specific, if available)\n\n` +
    `  The model runs locally with no API keys or cloud dependencies.\n` +
    `  After pulling, re-run this command.`,
  );
}

export async function detectDimensions(
  client: Ollama,
  modelName: string,
): Promise<number> {
  const response = await client.embed({ model: modelName, input: 'test' });
  return response.embeddings[0].length;
}

async function tryStartOllama(): Promise<boolean> {
  try {
    const child = spawn('ollama', ['serve'], {
      detached: true,
      stdio: 'ignore',
    });
    child.unref();

    process.stderr.write('Ollama not running — starting it automatically...\n');

    // Wait up to 5 seconds for it to come up
    for (let i = 0; i < 10; i++) {
      await new Promise((r) => setTimeout(r, 500));
      try {
        const test = new Ollama();
        await test.list();
        process.stderr.write('Ollama started.\n');
        return true;
      } catch {
        // Not ready yet
      }
    }
    return false;
  } catch {
    // ollama binary not found
    return false;
  }
}

/**
 * Run a full preflight check and return a human-readable diagnostic.
 * Used by the CLI to give first-time users clear setup instructions.
 */
export async function preflight(client: Ollama): Promise<{ ok: boolean; message: string }> {
  let healthy = await checkHealth(client);

  if (!healthy) {
    // Try to start Ollama if the binary exists
    const started = await tryStartOllama();
    if (started) {
      healthy = await checkHealth(client);
    }
  }

  if (!healthy) {
    return {
      ok: false,
      message:
        `Cannot connect to Ollama at http://localhost:11434.\n\n` +
        `  Ollama is a local model runner that codeuse uses for embeddings.\n` +
        `  It runs entirely on your machine — no cloud, no API keys.\n\n` +
        `  To set up:\n` +
        `    1. Install Ollama:  https://ollama.com/download\n` +
        `    2. Start it:        ollama serve\n` +
        `    3. Pull a model:    ollama pull nomic-embed-text\n` +
        `    4. Re-run:          codeuse index\n`,
    };
  }

  try {
    const model = await detectModel(client);
    return {
      ok: true,
      message: `Ollama is running. Using ${model.name} (${model.tier}, ${model.dimensions}d).`,
    };
  } catch (e) {
    return {
      ok: false,
      message: e instanceof Error ? e.message : String(e),
    };
  }
}
