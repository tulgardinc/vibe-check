import { createRequire } from 'node:module';
import fs from 'node:fs';
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { z } from 'zod';
import { runQuery } from './core/query-pipeline.js';
import { runIndex } from './core/index-pipeline.js';
import { runScan } from './core/scan-pipeline.js';
import { runStatus } from './core/status-pipeline.js';
import { loadIgnoreFile, addExclusion, saveIgnoreFile } from './ignore/ignore-file.js';
import { findProjectRoot } from './util/config.js';

const require = createRequire(import.meta.url);
const { version } = require('../package.json') as { version: string };

const server = new McpServer({
  name: 'codeuse',
  version,
});

function mcpError(e: unknown): { content: Array<{ type: 'text'; text: string }>; isError: true } {
  return {
    content: [{ type: 'text', text: `Error: ${e instanceof Error ? e.message : String(e)}` }],
    isError: true,
  };
}

// Tool 1: Query for similar functions
server.tool(
  'codeuse_query',
  `Find existing functions and logic blocks in the codebase that are semantically similar to the given TypeScript code. Uses embedding similarity combined with Jaccard token overlap to rank matches. Results include named functions and inline blocks (chunkType "block") — blocks indicate similar logic nested inside another function. Prefer passing "file" over "source" when the code is already on disk to save tokens. Use codeuse_add_exclusion to suppress false positives.`,
  {
    file: z.string().optional().describe('Path to a TypeScript file to check (preferred over source — saves tokens)'),
    source: z.string().optional().describe('TypeScript source code to check (use when code is not yet on disk)'),
    topK: z.number().optional().default(5).describe('Number of candidate matches per function (default: 5)'),
    threshold: z.number().optional().default(0.3).describe('Cosine distance threshold — lower means more similar (default: 0.3)'),
  },
  async ({ file, source, topK, threshold }) => {
    try {
      let resolvedSource: string;
      let fileName: string | undefined;
      if (file) {
        resolvedSource = fs.readFileSync(file, 'utf-8');
        fileName = file;
      } else if (source) {
        resolvedSource = source;
      } else {
        return mcpError(new Error('Provide either "file" or "source"'));
      }
      const result = await runQuery({ source: resolvedSource, fileName, topK, threshold });
      return {
        content: [{ type: 'text', text: JSON.stringify(result, null, 2) }],
      };
    } catch (e) {
      return mcpError(e);
    }
  },
);

// Tool 2: Index the codebase
server.tool(
  'codeuse_index',
  'Build or update the semantic index of TypeScript functions and logic blocks. Parses source with tree-sitter, generates embeddings via local Ollama, stores in SQLite. Incremental by default — only re-processes files that changed since the last run.',
  {
    path: z.string().optional().describe('Directory to index (defaults to project root)'),
    force: z.boolean().optional().default(false).describe('Force full re-index, ignoring incremental state'),
  },
  async ({ path, force }) => {
    try {
      const result = await runIndex({
        path,
        force,
        onProgress: (msg) => process.stderr.write(`${msg}\n`),
      });
      return {
        content: [{
          type: 'text',
          text: `Indexing complete. ${result.functionsIndexed} functions from ${result.filesScanned} files ` +
            `(${result.added} added, ${result.modified} modified, ${result.deleted} deleted). ` +
            `Model: ${result.model} (${result.tier}, ${result.dimensions}d).`,
        }],
      };
    } catch (e) {
      return mcpError(e);
    }
  },
);

// Tool 3: Index status
server.tool(
  'codeuse_status',
  'Check whether the codeuse index exists and is healthy. Reports: embedding model, indexed chunk count (functions + blocks), tracked files, exclusions, and staleness. If no index exists, run codeuse_index first.',
  {},
  async () => {
    try {
      const result = runStatus();
      if (!result.exists) {
        return {
          content: [{ type: 'text', text: 'No index found. Run codeuse_index to create one.' }],
        };
      }
      return {
        content: [{
          type: 'text',
          text: [
            `Database: ${result.dbPath} (${result.sizeMb} MB)`,
            `Model: ${result.model}`,
            `Dimensions: ${result.dimensions}`,
            `Indexed functions: ${result.indexedFunctions}${result.unembedded > 0 ? ` (${result.unembedded} awaiting embedding)` : ''}`,
            `Tracked files: ${result.trackedFiles}`,
            `Last indexed: ${result.lastIndexed}`,
            `Exclusions: ${result.exclusions}${result.staleExclusions > 0 ? ` (${result.staleExclusions} stale)` : ''}`,
          ].join('\n'),
        }],
      };
    } catch (e) {
      return mcpError(e);
    }
  },
);

// Tool 4: Scan entire codebase for similar function pairs
server.tool(
  'codeuse_scan',
  'Compare all indexed functions and logic blocks against each other to find duplicate or similar pairs across the codebase. Returns deduplicated pairs ranked by combined embedding + Jaccard similarity. Pairs involving "block" chunks indicate similar logic buried inline that could be extracted.',
  {
    topN: z.number().optional().default(50).describe('Maximum number of pairs to return (default: 50)'),
    threshold: z.number().optional().default(0.25).describe('Cosine distance threshold — lower means stricter (default: 0.25)'),
  },
  async ({ topN, threshold }) => {
    try {
      const result = runScan({ topN, threshold });
      return {
        content: [{ type: 'text', text: JSON.stringify(result, null, 2) }],
      };
    } catch (e) {
      return mcpError(e);
    }
  },
);

// Tool 5: Add a false-positive exclusion
server.tool(
  'codeuse_add_exclusion',
  'Permanently suppress a pair of functions/blocks from appearing as matches in query and scan results. Stores the exclusion in .codereuse-ignore.json using signature hashes. Use the signatureHash values from query or scan output.',
  {
    queryFunction: z.string().describe('Name of the query function'),
    queryPath: z.string().describe('File path of the query function'),
    querySignatureHash: z.string().describe('Signature hash of the query function (from query results)'),
    candidateFunction: z.string().describe('Name of the candidate function to exclude'),
    candidatePath: z.string().describe('File path of the candidate function'),
    candidateSignatureHash: z.string().describe('Signature hash of the candidate function (from query results)'),
    reason: z.string().describe('Why these functions are not duplicates'),
  },
  async ({ queryFunction, queryPath, querySignatureHash, candidateFunction, candidatePath, candidateSignatureHash, reason }) => {
    try {
      const projectRoot = findProjectRoot(process.cwd());
      let ignoreFile = loadIgnoreFile(projectRoot);

      ignoreFile = addExclusion(ignoreFile, {
        reason,
        added: new Date().toISOString().split('T')[0],
        pair: {
          a: { path: queryPath, function: queryFunction, signatureHash: querySignatureHash },
          b: { path: candidatePath, function: candidateFunction, signatureHash: candidateSignatureHash },
        },
      });

      saveIgnoreFile(projectRoot, ignoreFile);

      return {
        content: [{
          type: 'text',
          text: `Exclusion added: ${queryFunction} ↔ ${candidateFunction} ("${reason}"). Total exclusions: ${ignoreFile.exclusions.length}.`,
        }],
      };
    } catch (e) {
      return mcpError(e);
    }
  },
);

// Start the server
async function main() {
  const transport = new StdioServerTransport();
  await server.connect(transport);
  process.stderr.write('codeuse MCP server running on stdio\n');
}

main().catch((e) => {
  process.stderr.write(`Fatal: ${e instanceof Error ? e.message : String(e)}\n`);
  process.exit(1);
});
