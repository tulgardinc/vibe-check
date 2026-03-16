# codeuse — Architecture Reference

This document is a complete technical reference for the codeuse system. It covers every module, data structure, algorithm, external dependency, and exposed interface. It is intended to serve as the basis for reimplementing the system in another language (e.g. Rust) without needing to read the TypeScript source.

---

## What codeuse does

codeuse is a semantic code deduplication tool. It indexes functions and logic blocks from a codebase using tree-sitter parsing and vector embeddings, stores them in SQLite with vector search, and queries for similar existing code when new code is written.

The primary use case is LLM-assisted development: when an LLM writes new code, codeuse surfaces existing functions and inline logic blocks that do the same thing, so the LLM (or human) can reuse them instead of creating duplicates.

It runs entirely locally with no cloud dependencies. The embedding model runs via Ollama on the user's machine.

---

## Exposed interfaces

codeuse has two entry points that expose the same four core pipelines.

### CLI (`src/index.ts` → `bin/codeuse.js`)

Commander-based CLI with four subcommands. Each command module in `src/commands/` is a thin wrapper that parses CLI flags and calls the corresponding pipeline function.

| Command | Pipeline function | Purpose |
|---------|------------------|---------|
| `codeuse index [path]` | `runIndex(options)` | Build or update the semantic index |
| `codeuse query <file>` | `runQuery(options)` | Find similar chunks for code in a file |
| `codeuse scan` | `runScan(options)` | Find all similar pairs across the index |
| `codeuse status` | `runStatus(options)` | Report index health and statistics |

CLI flags: `--force` (re-index), `--top-k N`, `--threshold N`, `--json`, `--verbose`, `--db <path>`, `--stdin`.

Output goes to stdout. JSON when `--json` is set or stdout is not a TTY. Human-readable otherwise. Progress and warnings go to stderr.

### MCP server (`src/mcp-server.ts` → `bin/codeuse-mcp.js`)

Stdio-based MCP server using `@modelcontextprotocol/sdk`. Exposes five tools:

| Tool | Parameters | Pipeline |
|------|-----------|----------|
| `codeuse_query` | `file?: string, source?: string, topK?: number, threshold?: number` | `runQuery` |
| `codeuse_index` | `path?: string, force?: boolean` | `runIndex` |
| `codeuse_scan` | `topN?: number, threshold?: number` | `runScan` |
| `codeuse_status` | (none) | `runStatus` |
| `codeuse_add_exclusion` | `queryFunction, queryPath, querySignatureHash, candidateFunction, candidatePath, candidateSignatureHash, reason` | Writes to `.codereuse-ignore.json` |

All tools return `{ content: [{ type: "text", text: "..." }] }`. Query and scan return JSON-serialized results. Index and status return human-readable strings. Errors return `{ isError: true }`.

---

## Data flow

### Index time

```
TypeScript files on disk
    → findTypeScriptFiles(): discover .ts files, exclude node_modules/dist/etc.
    → parseFile(): tree-sitter AST → FunctionChunk[] (functions + blocks)
    → upsertFunctions(): write chunks to SQLite functions table
    → embedBatch(): Ollama generates Float32Array embeddings
    → updateEmbedding(): store embedding BLOBs in SQLite
```

### Query time

```
Input source code (file path or raw string)
    → parseSource(): tree-sitter AST → FunctionChunk[] (functions + blocks)
    → embedBatch(): embed all query chunks
    → for each chunk:
        → queryKNN(): sqlite-vec cosine distance, top-K×3 over-fetch
        → filter self-matches (by chunk ID)
        → rerankCandidates(): Jaccard token re-ranking (α=0.7)
        → applyExclusions(): filter against .codereuse-ignore.json
        → slice to top-K
    → return QueryResult
```

### Scan time

```
    → getAllFunctions(): load all stored functions with embeddings
    → pre-tokenize all sources for Jaccard (tokenCache)
    → for each function:
        → queryKNN(): find 6 nearest neighbors
        → for each neighbor (skip self, deduplicate unordered pairs):
            → compute Jaccard similarity from tokenCache
            → combined score = 0.7 × embedding_distance + 0.3 × (1 - jaccard)
        → filter excluded pairs
    → sort all pairs by combined score, cap at topN
    → return ScanResult
```

---

## Modules

### Parser (`src/parser/`)

**Purpose:** Extract function-level and block-level chunks from TypeScript source.

**Files:**
- `chunker.ts` — main parser logic
- `signature.ts` — signature hash computation
- `types.ts` — `FunctionChunk`, `ParamInfo`, `ParsedFile` types

**How parsing works:**

A single module-level `tree-sitter` `Parser` instance is configured with the TypeScript grammar. The `parseSource(source, filePath)` function parses the source into an AST, then walks it with `walkNode()`.

**Function extraction:** The walker recognizes these AST node types as functions:
- `function_declaration`
- `generator_function_declaration`
- `method_definition`
- `arrow_function`
- `function_expression`

For `lexical_declaration` / `variable_declaration`, it checks if the value is an `arrow_function` or `function_expression` (handles `const foo = () => {}`).

Functions shorter than 3 lines (`MIN_LINES`) are skipped. The walker does not recurse into function bodies for nested function extraction — it stops at the first function boundary.

Exported functions are detected by checking if the parent node is `export_statement` or `export_default_declaration`.

**Block extraction:** After extracting a function chunk, `extractBlocks()` walks the function's body looking for eligible logic blocks. A block is eligible if:
- It is a `statement_block` child of a control flow node (`if_statement`, `for_statement`, `for_in_statement`, `while_statement`, `do_statement`, `try_statement`)
- OR it is the body of an `arrow_function` assigned to a variable inside the function (inline callback)
- AND it is ≥ 6 lines (`MIN_BLOCK_LINES`)
- AND it contains ≥ 1 control flow node
- AND it contains ≥ 2 statements

The block walker recurses into children but stops at nested function boundaries (does not enter nested functions).

**Chunk identity:**

| Field | Functions | Blocks |
|-------|-----------|--------|
| `id` | `filePath:functionName:startLine` | `filePath:<block:startLine>:startLine` |
| `functionName` | Actual name | `<block:startLine>` (synthetic) |
| `signatureHash` | `sha256(name + params + returnType).slice(0, 8)` | `sha256(sourceText).slice(0, 8)` |
| `params` | Extracted from AST | `[]` |
| `returnType` | Extracted from AST | `null` |
| `chunkType` | `'function'` | `'block'` |
| `context` | `null` | Containing function's name |

**Parameter extraction:** Walks the `parameters` field of the function node. Recognizes `required_parameter`, `optional_parameter`, `rest_parameter`. Extracts name from `pattern` or `name` field, type from `type` field. Type annotation prefix (`:`) is stripped.

**Known limitation:** The tree-sitter Node.js bindings have a 32KB input buffer limit. Files larger than 32,768 bytes cause `Parser.parse()` to throw "Invalid argument". The index pipeline catches this and skips the file with a warning. A Rust implementation using tree-sitter's native API would not have this limitation.

### Embedder (`src/embedder/`)

**Purpose:** Generate vector embeddings for source code text via Ollama.

**Files:**
- `ollama-client.ts` — Ollama health check, model detection, auto-start, preflight
- `embed.ts` — `OllamaEmbedder` class implementing the `Embedder` interface
- `types.ts` — `Embedder`, `ModelInfo`, `EmbeddingResult` interfaces

**Embedder interface:**
```
interface Embedder {
  modelName: string
  dimensions: number
  tier: string
  embedBatch(inputs: string[], onProgress?): Promise<Float32Array[]>
  embedQuery(input: string): Promise<Float32Array>
}
```

`embedBatch` processes inputs in batches of 32 (`BATCH_SIZE`), calling `client.embed({ model, input: batch })` for each batch. Returns `Float32Array[]`.

`embedQuery` prepends `"search_query: "` to the input before embedding (Nomic model convention for asymmetric search).

**Model detection:** `detectModel()` queries Ollama's model list and selects the first match from this priority order:
1. `nomic-embed-code` (7B tier)
2. `nomic-embed-code:137m` (137M tier)
3. `nomic-embed-text` (7B tier)
4. `nomic-embed-text:*` variant (137M tier)

Dimensions are detected by embedding the string `"test"` and measuring the output length.

**Auto-start:** If Ollama is not reachable at `http://localhost:11434`, the client spawns `ollama serve` as a detached process and polls for up to 5 seconds.

**Preflight:** `preflight(client)` runs health check → auto-start → model detection → returns `{ ok, embedder, message }` or `{ ok: false, message }`. Used by both index and query pipelines.

**Model compatibility:** The active model name is stored in `index_meta`. At query time, if the stored model differs from the detected model, a warning is emitted. Vectors from different models are incompatible — switching models requires `--force` re-index.

### Store (`src/store/`)

**Purpose:** SQLite database for chunks, embeddings, file tracking, and metadata.

**Files:**
- `db.ts` — database open, schema creation, migrations, metadata KV store
- `index-store.ts` — function CRUD, KNN search
- `file-tracker.ts` — incremental indexing via content hash comparison
- `types.ts` — `StoredFunction`, `FileRecord` interfaces

**Schema (3 tables):**

```sql
CREATE TABLE index_meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE tracked_files (
  file_path TEXT PRIMARY KEY,
  content_hash TEXT NOT NULL,
  mtime_ms INTEGER NOT NULL,
  indexed_at TEXT NOT NULL
);

CREATE TABLE functions (
  id TEXT PRIMARY KEY,
  file_path TEXT NOT NULL,
  function_name TEXT NOT NULL,
  source_text TEXT NOT NULL,
  start_line INTEGER NOT NULL,
  end_line INTEGER NOT NULL,
  params_json TEXT NOT NULL,
  return_type TEXT,
  is_exported INTEGER NOT NULL,
  signature_hash TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  embedding BLOB,
  chunk_type TEXT NOT NULL DEFAULT 'function',
  context TEXT,
  FOREIGN KEY (file_path) REFERENCES tracked_files(file_path) ON DELETE CASCADE
);

CREATE INDEX idx_functions_file ON functions(file_path);
CREATE INDEX idx_functions_sig ON functions(signature_hash);
```

The `chunk_type` and `context` columns are added via `ALTER TABLE` migration if missing (handles DBs created before block-level chunking). The migration runs every time the database is opened.

**Database setup:** `openDatabase(dbPath)` creates a `better-sqlite3` Database instance, enables WAL mode and foreign keys, loads the `sqlite-vec` extension, and runs schema creation + migrations.

**Key operations:**

- `upsertFunctions(db, chunks)` — `INSERT OR REPLACE` in a transaction. Computes `content_hash` from `sourceText` at insert time. Embedding is set to `NULL` (filled in later by the embed step).
- `deleteFunctionsForFile(db, filePath)` — deletes all functions for a file. Used when a file is modified (delete old → insert new).
- `queryKNN(db, queryEmbedding, topK, threshold)` — cosine distance via `vec_distance_cosine(embedding, ?)`, ordered ascending, filtered by `distance <= threshold`. Returns `StoredFunction & { distance }`.
- `getFunctionsWithoutEmbeddings(db)` — `WHERE embedding IS NULL`. Used after upsert to find chunks that need embedding.
- `updateEmbedding(db, functionId, embedding)` — stores `Float32Array` as `Buffer.from(embedding.buffer)`.

**Incremental indexing (`file-tracker.ts`):**

`computeChangedFiles(db, filePaths)` compares the current file list against `tracked_files`:
- Files not in `tracked_files` → `added`
- Files where `mtimeMs` differs → check `contentHash` → if different, `modified`
- Files in `tracked_files` but not in current file list → `deleted`
- Otherwise → `unchanged`

Fast path: if mtime matches, skip content hash computation.

**Metadata KV (`index_meta`):**

Stores: `model_name`, `model_dimensions`, `last_indexed_at`, `created_at`. Used for model mismatch detection.

### Ranking (`src/ranking/`)

**Purpose:** Jaccard token similarity for re-ranking KNN candidates.

**File:** `jaccard.ts`

**Tokenizer (`tokenizeCode`):**
1. Strip type annotations via regex: `:\s*[A-Z][\w<>,\s|&\[\]]*` before `;,)=\n{`
2. Strip `as Type` casts
3. Strip generic brackets `<Type>`
4. Extract tokens matching `[a-zA-Z_$][\w$]*` (identifiers) and `[+\-*/%=<>!&|^~?:]+` (operators)
5. Lowercase all tokens
6. Filter stop words (language keywords: `const`, `let`, `function`, `return`, `if`, `else`, `for`, `while`, `try`, `catch`, `class`, `import`, `export`, `true`, `false`, `null`, `undefined`, etc.)
7. Filter tokens with length ≤ 1
8. Return as `Set<string>`

**Jaccard similarity:** Standard set intersection / union. Optimized to iterate over the smaller set.

**Combined scoring:** `combinedScore = α × embeddingDistance + (1 - α) × (1 - jaccardSimilarity)` where `α = 0.7`. Lower score = more similar. This weights embeddings at 70% and token overlap at 30%.

**Re-ranking (`rerankCandidates`):** Takes query source text and an array of candidates with their embedding distances. Tokenizes the query, computes Jaccard against each candidate, computes combined score, sorts by combined score ascending.

### Prefilter (`src/prefilter/`)

**Purpose:** Fast token-based clone detection as a pre-pass before embeddings.

**Files:**
- `jscpd-runner.ts` — runs jscpd for Type 1-2 clone detection
- `merge.ts` — merges prefilter matches with embedding candidates
- `types.ts` — `PrefilterMatch` type

**Note:** The prefilter is implemented but not currently wired into the query pipeline. The query pipeline uses embedding + Jaccard only. The merge logic exists for future integration. The jscpd runner wraps the `jscpd` npm package with `detectClones({ path, silent: true, minLines: 5, minTokens: 50, format: ['typescript'] })`.

### Ignore / Exclusions (`src/ignore/`)

**Purpose:** Persist false positive pairs so they don't recur in results.

**Files:**
- `ignore-file.ts` — load, save, add exclusion, check exclusion, apply exclusions
- `stale-detector.ts` — detect exclusions referencing changed/deleted functions
- `types.ts` — `Exclusion`, `ExclusionSide`, `IgnoreFile`, `StaleWarning`

**File format (`.codereuse-ignore.json`):**
```json
{
  "version": 1,
  "exclusions": [{
    "reason": "string",
    "added": "YYYY-MM-DD",
    "pair": {
      "a": { "path": "string", "function": "string", "signatureHash": "8hexchars" },
      "b": { "path": "string", "function": "string", "signatureHash": "8hexchars" }
    }
  }]
}
```

**Exclusion matching:** Pair-level, direction-independent. `isExcluded(ignoreFile, hashA, hashB)` checks both orderings. `applyExclusions` filters a candidate array against all exclusions for a given query hash.

**Stale detection:** For each exclusion, looks up both sides by `signatureHash` in the functions table. If either side is not found, emits a `StaleWarning` with the reason. Stale warnings are included in query results.

**Deduplication:** `addExclusion` checks both orderings before appending. Returns the same IgnoreFile if the pair already exists.

### Output (`src/output/`)

**Purpose:** Type definitions and formatting for query and scan results.

**Files:**
- `types.ts` — `Candidate`, `QueryFunction`, `QueryResult`
- `scan-types.ts` — `ScanMatchEntry`, `ScanMatch`, `ScanResult`
- `formatter.ts` — `formatJson`, `formatHuman` for query results
- `scan-formatter.ts` — `formatScanJson`, `formatScanHuman` for scan results

**QueryResult structure:**
```
{
  queryFunctions: [{
    name: string,
    file: string,
    line: number,
    chunkType?: 'function' | 'block',
    candidates: [{
      name: string,
      path: string,
      line: number,
      distance: number,          // combined score (lower = more similar)
      detectionMethod: 'embedding' | 'jscpd' | 'combined',
      source: string,            // full source text of the matched chunk
      signatureHash: string,
      chunkType?: 'function' | 'block',
      context?: string | null,   // containing function name for blocks
      jaccardSimilarity?: number
    }]
  }],
  warnings: string[],
  meta: {
    model: string,
    indexedFunctions: number,
    queryFunctions: number,
    elapsedMs: number,
    indexedBlocks?: number
  }
}
```

**ScanResult structure:**
```
{
  matches: [{
    a: { name, path, line, signatureHash, chunkType?, context? },
    b: { name, path, line, signatureHash, chunkType?, context? },
    distance: number,           // combined score
    similarity: 'identical' | 'nearly identical' | 'very similar' | 'similar' | 'weak',
    jaccardSimilarity?: number
  }],
  meta: {
    model: string,
    chunksScanned: number,
    pairsFound: number,
    elapsedMs: number
  }
}
```

**Similarity tiers (for scan):**
- `identical`: distance ≤ 0.01
- `nearly identical`: distance ≤ 0.05
- `very similar`: distance ≤ 0.12
- `similar`: distance ≤ 0.20
- `weak`: distance > 0.20

**Human format examples:**

Query:
```
runQuery (src/core/query-pipeline.ts:22)
  runIndex (src/core/index-pipeline.ts:30) — similarity: 0.74 [embedding, jaccard: 0.29]
  <block:88> in buildSummary (src/components/Report.tsx:88) — similarity: 0.84 [embedding, jaccard: 0.41]
```

Scan:
```
── VERY SIMILAR (3) ─────────────────────────────────────
  functionA (src/utils/a.ts:10)
  functionB (src/utils/b.ts:25)
  92% similar (distance: 0.0823, jaccard: 0.58)
```

### Core Pipelines (`src/core/`)

**Purpose:** Orchestrate the stages. Each pipeline is a single `async function` that combines parser, embedder, store, and output modules.

**Files:**
- `index-pipeline.ts` — `runIndex(options): Promise<IndexResult>`
- `query-pipeline.ts` — `runQuery(options): Promise<QueryResult>`
- `scan-pipeline.ts` — `runScan(options): ScanResult` (synchronous — no embedding needed)
- `status-pipeline.ts` — `runStatus(options): StatusResult` (synchronous)

**Index pipeline flow:**
1. Find TypeScript files (early return if 0 found)
2. Preflight Ollama (health + model detection)
3. Open database
4. Check model mismatch (error if different model and not `--force`)
5. Compute changed files (or treat all as added if `--force`)
6. Delete functions for removed files
7. For each added/modified file: upsert tracked file → parse → upsert functions (skip on parse failure)
8. Embed all functions without embeddings (batch of 32)
9. Update metadata

**Query pipeline flow:**
1. Parse input source into chunks (early return if 0 chunks)
2. Check DB exists (error if not — don't create empty DB)
3. Preflight Ollama
4. Check model mismatch (warn, don't error)
5. Load exclusions, detect stale exclusions
6. Batch-embed all query chunks
7. For each chunk: KNN over-fetch (top-K × 3) → filter self → Jaccard re-rank → apply exclusions → slice to top-K

**Scan pipeline flow:**
1. Check DB exists (error if not)
2. Load all functions with embeddings
3. Pre-tokenize all sources into a `Map<id, Set<string>>`
4. For each function: KNN top 6 → deduplicate unordered pairs → Jaccard re-score → filter exclusions
5. Sort all pairs by combined score, cap at topN

### Utilities (`src/util/`)

- `hash.ts` — `sha256(input)` and `contentHash(source)` using Node `crypto`
- `config.ts` — `findProjectRoot` (walks up looking for `.git` or `package.json`), `resolveDbPath`, `findTypeScriptFiles` (recursive readdir, filters by `.ts`, excludes `node_modules`, `dist`, `.git`, `coverage`, `.next`, `build`, `experiment`, skips `.d.ts`, `.test.ts`, `.spec.ts`)
- `logger.ts` — colored stderr logging with levels: `quiet`, `normal`, `verbose`. Uses ANSI codes when stderr is a TTY and `NO_COLOR` is not set.

---

## External dependencies

| Dependency | What it does | Rust equivalent |
|-----------|-------------|-----------------|
| `tree-sitter` + `tree-sitter-typescript` | AST parsing | `tree-sitter` crate (native, no 32KB limit) |
| `better-sqlite3` | SQLite driver | `rusqlite` |
| `sqlite-vec` | Vector similarity search extension | `sqlite-vec` C extension loaded via rusqlite |
| `ollama` (npm) | Ollama HTTP client | HTTP client (reqwest) calling Ollama REST API directly |
| `commander` | CLI argument parsing | `clap` |
| `@modelcontextprotocol/sdk` | MCP server | `mcp-server` Rust crate or raw JSON-RPC over stdio |
| `jscpd` | Token-based clone detection (prefilter) | Implement directly or drop — prefilter is not wired in |
| `zod` | Schema validation for MCP params | `serde` + MCP SDK handles this |

---

## File discovery

`findTypeScriptFiles(rootDir, excludes)` does a recursive `readdir` and collects files matching:
- Ends with `.ts`
- Does NOT end with `.d.ts`, `.test.ts`, `.spec.ts`
- No path segment matches an exclude entry

Default excludes: `node_modules`, `dist`, `.git`, `coverage`, `.next`, `build`, `experiment`.

A Rust implementation should consider respecting `.gitignore` instead of a hardcoded exclude list.

---

## Hashing

All hashing uses SHA-256.

- **Content hash:** `sha256(sourceText)` — full hex string. Used for incremental indexing (detect file changes) and stored on every function row.
- **Signature hash:** `sha256(normalizedName + "(" + normalizedParamTypes + "):" + normalizedReturnType).slice(0, 8)` — 8 hex chars. Used for exclusion matching. Normalization: lowercase, strip whitespace. Params joined by `,`, types default to `any`, return defaults to `void`.
- **Block signature hash:** `sha256(sourceText).slice(0, 8)` — content-based since blocks have no named signature.

---

## Configuration and state files

| File | Location | Purpose | Committed to git |
|------|----------|---------|-----------------|
| `.codeuse.db` | Project root | SQLite database with index | No (gitignored, regeneratable) |
| `.codereuse-ignore.json` | Project root | False positive exclusions | Yes |

---

## Known limitations

- **Tree-sitter Node.js 32KB limit:** Files over 32,768 bytes fail to parse. The native tree-sitter C/Rust library has no such limit.
- **TypeScript only:** The parser uses tree-sitter-typescript. The grammar is a superset of JavaScript so `.js` files would parse, but file discovery filters to `.ts` only. Other languages would need their own grammars — the chunking logic (find functions, find blocks) is structurally similar across languages.
- **Prefilter not wired in:** jscpd prefilter and merge logic exist but are not called from the query pipeline. Only embedding + Jaccard is active.
- **Single-threaded embedding:** Ollama calls are sequential batches of 32. Parallelism is limited by Ollama's own concurrency.
- **No .gitignore support:** File discovery uses a hardcoded exclude list, not .gitignore parsing.
