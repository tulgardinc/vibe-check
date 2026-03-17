# CLAUDE.md

## Project

`vibecheck` (repo name: vibe-check) is a semantic code deduplication tool. It indexes functions from a codebase using tree-sitter parsing and code embeddings (Ollama), stores them in SQLite with vector search, then queries for similar existing functions when new code is written. Designed to run locally with zero cloud dependencies.

## Commands

```bash
cargo build --release    # Build release binaries
cargo test               # Run all tests
cargo run -- index       # Run CLI via cargo
```

**Prerequisites:** Rust toolchain, Ollama running locally with an embedding model (auto-detected; prefers `nomic-embed-code`).

**CLI usage (after build):**
```bash
vibec index [path]       # Build/update function index
vibec query <file>       # Find similar functions in index
vibec scan               # Find all similar pairs across the codebase
vibec status             # Show index health
```

**MCP server:** `vibecheck-mcp` binary exposes tools for Claude integration.

## Architecture

Three-stage pipeline in `src/`:

**Entry points:** `main.rs` (CLI via clap) and `mcp.rs` (MCP server).

**Core pipelines** (`core/`): `index_pipeline.rs`, `query_pipeline.rs`, `scan_pipeline.rs`, `status_pipeline.rs`.

**Data flow:**
1. **Parser** (`parser/`) — tree-sitter extracts function-level chunks via `LanguageSupport` trait. `registry.rs` maps file extensions to languages, `typescript.rs` is the first implementation. Signature hashing in `signature.rs`.
2. **Embedder** (`embedder/`) — Ollama HTTP client with auto-start, model detection.
3. **Store** (`store/`) — SQLite + sqlite-vec. `db.rs` has schema, `index_store.rs` handles upsert and KNN cosine search, `file_tracker.rs` does incremental indexing.
4. **Ignore** (`ignore/`) — `.vibecheck-ignore.json` stores pair-level false positive exclusions.
5. **Ranking** (`ranking/`) — Jaccard token-based re-ranking of embedding candidates.
6. **Output** (`output/`) — JSON and human-readable formatting.

## Key Design Decisions

- Function IDs are `filePath:functionName:startLine` — deterministic and greppable
- Foreign keys enforce `functions.file_path` references `tracked_files.file_path` — delete tracked files cascades to functions
- Embedding model name is stored in `index_meta`; mismatch at query time triggers warning
- The `.vibecheck.db` file is gitignored and regeneratable
