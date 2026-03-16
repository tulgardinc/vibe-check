# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

`codeuse` (repo name: vibe-check) is a semantic code deduplication tool for TypeScript. It indexes functions from a codebase using tree-sitter parsing and code embeddings (Ollama), stores them in SQLite with vector search, then queries for similar existing functions when new code is written. Designed to run locally with zero cloud dependencies.

## Commands

```bash
npm run build          # TypeScript compilation (tsc)
npm run dev            # Watch mode compilation
npm test               # Run all tests (vitest run)
npm run test:watch     # Watch mode tests
npx vitest run test/unit/chunker.test.ts  # Run a single test file
```

**Prerequisites:** Node >= 20, Ollama running locally with `nomic-embed-code` model.

**CLI usage:**
```bash
bin/codeuse.js index [path]    # Build/update function index
bin/codeuse.js query <file>    # Find similar functions in index
bin/codeuse.js status          # Show index health
```

## Architecture

Three-stage pipeline with shared core modules:

**Entry points:** `src/index.ts` (CLI via Commander) and `src/mcp-server.ts` (MCP server for Claude integration with 4 tools).

**Core pipelines** (`src/core/`): `index-pipeline.ts`, `query-pipeline.ts`, `status-pipeline.ts` — orchestrate the stages. CLI commands (`src/commands/`) are thin wrappers around these pipelines.

**Data flow:**
1. **Parser** (`src/parser/chunker.ts`) — tree-sitter extracts function-level chunks from TypeScript ASTs. Signature hashing in `signature.ts`.
2. **Embedder** (`src/embedder/`) — Ollama client with auto-start, model detection (7B GPU preferred, 137M CPU fallback). Models produce incompatible vector spaces; switching requires re-index.
3. **Store** (`src/store/`) — SQLite + sqlite-vec. `db.ts` has schema (tables: `index_meta`, `tracked_files`, `functions` with embedding BLOBs). `index-store.ts` handles upsert and KNN cosine search. `file-tracker.ts` does incremental indexing via content hash comparison.
4. **Ignore** (`src/ignore/`) — `.codereuse-ignore.json` stores pair-level false positive exclusions with signature hashes. Stale detection warns when referenced functions change.
5. **Prefilter** (`src/prefilter/`) — jscpd token-based clone detection as fast path before embeddings.
6. **Output** (`src/output/formatter.ts`) — JSON and human-readable formatting.

## Key Design Decisions

- ESM-only (`"type": "module"` in package.json, Node16 module resolution)
- Function IDs are `filePath:functionName:startLine` — deterministic and greppable
- Foreign keys enforce `functions.file_path` references `tracked_files.file_path` — delete tracked files cascades to functions
- Embedding model name is stored in `index_meta`; mismatch at query time triggers warning
- The `.codeuse.db` file is gitignored and regeneratable

## Detailed Architecture Docs

See `PROJECT.md` for comprehensive documentation including clone type coverage, performance characteristics, false positive system design, and LLM output contract.
