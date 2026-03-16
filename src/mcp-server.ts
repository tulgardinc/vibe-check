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
  'Find existing functions in the codebase that are semantically similar to TypeScript code. Pass either a file path or source code. Prefer file path when checking code you already wrote to disk — it avoids sending the full source in the tool call.',
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
  'Index TypeScript functions in the codebase for semantic search. Run this once to build the index, or again after code changes to update it incrementally.',
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
  'Show the health and statistics of the codeuse index: model, function count, tracked files, exclusions.',
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
  'Scan all indexed functions against each other to find duplicate or similar function pairs across the codebase. Returns deduplicated pairs ranked by similarity.',
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
  'Mark two functions as NOT duplicates so they are no longer suggested as matches. Use this when a query result is a false positive.',
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
