---
name: vibecheck
description: >
  Semantic code deduplication scanner. Finds duplicated or near-identical
  functions across the codebase using vector embeddings and Jaccard similarity.

  Auto-invoke when: refactoring code, extracting shared utilities, auditing
  code quality, reviewing for tech debt, preparing a commit or PR, or when
  the user mentions duplication, dedup, or similar code.

  Do NOT invoke for: writing new features from scratch, debugging runtime
  errors, configuration changes, or documentation-only edits.
---

# Vibecheck Setup & Workflow Guide

You are helping the user set up and understand vibecheck — a semantic code deduplication tool.

## Step 1: Environment Configuration

Check if a `.env` file exists at the project root. If it does not, walk the user through creating one by asking about their preferences for each variable below. Ask all questions upfront in a single message, providing the defaults so they can just confirm or override:

| Variable | Purpose | Default |
|----------|---------|---------|
| `VIBECHECK_MODEL` | Embedding model name | Auto-detected (`nomic-embed-code` preferred) |
| `OLLAMA_HOST` | Ollama server URL | `http://localhost:11434` |
| `VIBECHECK_DIMENSIONS` | Embedding dimensions | Model default |
| `VIBECHECK_QUERY_PREFIX` | Query prefix for search | Auto-detected for Nomic models |
| `VIBECHECK_CONTEXT_LENGTH` | Context length in tokens | Model default |
| `VIBECHECK_MAX_INPUT_BYTES` | Max input bytes for truncation | 16,000 |

Only include variables in the `.env` where the user wants a non-default value. If they're happy with all defaults, still create the `.env` with a comment header explaining the available options for future reference.

If a `.env` already exists, read it and confirm the current settings with the user, offering to update anything.

## Step 2: Workflow Guide

After the `.env` is handled, explain the recommended vibecheck workflow — both CLI and MCP. Cover each tool with its benefits, drawbacks, and when to use it:

### `vibec index` / `vibecheck_index`
- **What it does:** Parses all supported source files with tree-sitter, generates embeddings via Ollama, and stores them in a local SQLite database.
- **When to use:** After large changes — merging a big PR, finishing a refactor, pulling in new code. Keeps the index fresh so query/scan results stay relevant.
- **Drawbacks:**
  - **Indexing time:** Can take a long time on large codebases. Embedding is the bottleneck — each function/block requires an Ollama round-trip. On a first index of a large project, expect minutes to tens of minutes. Incremental re-indexes are much faster since only changed files are re-embedded.
  - **Context length:** If your embedding model has a short context window, long functions will be truncated before embedding, which can reduce match quality for large functions. The `VIBECHECK_CONTEXT_LENGTH` and `VIBECHECK_MAX_INPUT_BYTES` variables control this. If you're seeing poor matches on long functions, consider a model with a larger context window or increasing these limits if your model supports it.
  - **Memory/CPU:** Ollama embedding models vary in resource usage. Lighter models are faster but may produce lower-quality embeddings.
- **Tips:** Use `--force` to rebuild from scratch if you change models or suspect index corruption. Use `vibecheck_index_stop` (MCP) to pause a long-running index — progress is saved and you can resume later.

### `vibec query <file>` / `vibecheck_query`
- **What it does:** Parses and embeds the given file (or source code), then finds the most similar functions already in the index using KNN cosine search + Jaccard re-ranking.
- **When to use:** For targeted checks on the specific files you're working on. This is the highest signal-to-noise ratio tool — use it when writing new code or reviewing a specific file.
- **Benefits:** Fast (only embeds the query file), focused results, great for catching duplication as you write.
- **Drawbacks:** Only finds duplicates relative to what's already indexed. If the index is stale, results may miss recent code.
- **Tips:** Adjust `--threshold` (lower = stricter) and `--top-k` to tune results. The MCP version accepts raw `source` code too, so you can check snippets without saving to a file.

### `vibec scan` / `vibecheck_scan`
- **What it does:** Compares every indexed function against every other indexed function to find all similar pairs across the entire codebase.
- **When to use:** Periodic audits — before a release, after a large feature lands, or as a codebase health check. Not for daily use.
- **Benefits:** Comprehensive — catches duplication you wouldn't think to look for. Great for finding systemic patterns.
- **Drawbacks:**
  - **Slow on large codebases:** O(n) KNN queries where n = number of indexed functions. A codebase with 500+ functions will take a while.
  - **Noisy results:** More results means more false positives. Use exclusions aggressively to keep signal high.
- **Tips:** Use `--threshold` to filter weak matches. Use `--top-n` to limit output. Add exclusions for intentional duplication (test mocks, generated code, etc.) via `vibecheck_add_exclusion`, `vibecheck_add_file_exclusion`, `vibecheck_add_file_pair_exclusion`, or `vibecheck_add_file_group_exclusion`.

### `vibec status` / `vibecheck_status`
- **What it does:** Shows index health — number of files, functions, embedding model, and whether the index is up to date.
- **When to use:** To check if you need to re-index before running query/scan.

### Common issues
- **Model mismatch warning:** If you change your embedding model after indexing, vibecheck will warn you. Re-index with `--force` to rebuild with the new model.
- **Ollama not running:** vibecheck will attempt to auto-start Ollama, but if it fails, make sure `ollama serve` is running.
- **Poor match quality on long functions:** Increase `VIBECHECK_MAX_INPUT_BYTES` or `VIBECHECK_CONTEXT_LENGTH`, or use a model with a larger context window.

### Recommended daily workflow
1. **Index** after pulling or merging significant changes
2. **Query** the files you're actively editing — catch duplication as you write
3. **Scan** periodically for a full audit
4. **Exclude** false positives to keep results clean over time
