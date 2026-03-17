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

# Vibecheck — MCP Skill Guide

You have access to vibecheck via MCP tools. Use them to find and manage duplicated code.

## Step 1: Environment Configuration

Check if a `.env` file exists at the project root. If not, ask the user about their preferences for the variables below — all at once, showing the defaults so they can confirm or override:

| Variable | Purpose | Default |
|----------|---------|---------|
| `VIBECHECK_MODEL` | Embedding model name | Auto-detected (`nomic-embed-code` preferred) |
| `OLLAMA_HOST` | Ollama server URL | `http://localhost:11434` |
| `VIBECHECK_DIMENSIONS` | Embedding dimensions | Model default |
| `VIBECHECK_QUERY_PREFIX` | Query prefix for search | Auto-detected for Nomic models |
| `VIBECHECK_CONTEXT_LENGTH` | Context length in tokens | Model default |
| `VIBECHECK_MAX_INPUT_BYTES` | Max input bytes for truncation | 16,000 |

Only include non-default values. If all defaults are fine, create the `.env` with a comment header listing the options for future reference. If a `.env` already exists, read it and confirm with the user.

## Step 2: Workflow Guide

After the environment is set up, explain the recommended workflow using the MCP tools:

### `vibecheck_index` — Build or update the index
- **What it does:** Parses source files with tree-sitter, generates embeddings via Ollama, stores them in SQLite. Runs in the background — call it again to check progress.
- **When to use:** After large changes — merging a PR, finishing a refactor, pulling new code. Keeps results relevant.
- **Parameters:** `path` (directory), `force` (full re-index), plus Ollama overrides (`model`, `ollamaHost`, `contextLength`, `maxInputBytes`, `queryPrefix`, `dimensions`, `db`).
- **Caveats:**
  - **Indexing time:** Can take minutes on large codebases. Embedding is the bottleneck (~2s per function). Incremental re-indexes are much faster — only changed files are re-embedded. This is a good time to work on other tasks while it runs in the background.
  - **Context length:** Models with short context windows will truncate long functions before embedding, reducing match quality. Tune with `contextLength` and `maxInputBytes`, or use a model with a larger window.
- **Tips:** Use `force: true` to rebuild after changing models. Use `vibecheck_index_stop` to pause — progress is saved, call `vibecheck_index` again to resume.

### `vibecheck_query` — Find similar functions to new code
- **What it does:** Parses and embeds the input, then finds the most similar indexed functions using KNN cosine search + Jaccard re-ranking. Returns JSON with candidates, distances, and similarity scores.
- **When to use:** Targeted checks on files you're actively working on. Best signal-to-noise ratio — use it when writing new code or reviewing a file.
- **Parameters:** `file` (path) or `source` (raw code), `topK` (default 5), `threshold` (default 0.3), plus Ollama overrides.
- **Benefits:** Fast — only embeds the query. Focused results.
- **Drawback:** Only finds duplicates relative to what's indexed. If the index is stale, results may miss recent code. Check `vibecheck_status` first.

### `vibecheck_scan` — Find all similar pairs across the codebase
- **What it does:** Compares every indexed function against every other to find all similar pairs. Returns JSON grouped by similarity tier.
- **When to use:** Periodic audits — before a release, after a large feature, or as a health check. Not for daily use.
- **Parameters:** `topN` (default 50), `threshold` (default 0.25), `db`.
- **Benefits:** Comprehensive — catches duplication you wouldn't think to look for.
- **Drawbacks:**
  - **Slow on large codebases.** O(n) queries where n = indexed functions.
  - **Noisy.** More results means more false positives. Use exclusions to keep signal high.

### `vibecheck_status` — Check index health
- **What it does:** Reports database size, model, dimensions, function/file counts, last indexed time, and exclusion counts.
- **When to use:** Before running query/scan, to check if a re-index is needed.
- **Parameters:** `db`.

### Exclusion tools — Suppress false positives

When results include intentional duplication (test mocks, generated code, structural patterns), exclude them so future results are cleaner:

- **`vibecheck_add_exclusion`** — Exclude a specific function pair. Requires both sides' `queryFunction`, `queryPath`, `querySignatureHash`, `candidateFunction`, `candidatePath`, `candidateSignatureHash`, and a `reason`. Use the `signatureHash` values from query/scan results.
- **`vibecheck_add_file_exclusion`** — Exclude a file or glob pattern entirely (e.g. `pattern: "src/generated/**"`). Requires `pattern` and `reason`.
- **`vibecheck_add_file_pair_exclusion`** — Exclude all comparisons between two files. Requires `fileA`, `fileB`, and `reason`.
- **`vibecheck_add_file_group_exclusion`** — Exclude all pairwise comparisons in a group of files. Requires `files` (array, at least 2) and `reason`. Expands to all pairs automatically.

### Important: gitignore
The `.vibecheck.db` file is a local index and should not be committed. If it's not already in `.gitignore`, add it:
```
.vibecheck.db
```

### Common issues
- **Model mismatch warning:** Re-index with `force: true` after changing models.
- **Ollama not running:** vibecheck auto-starts Ollama, but if it fails, ensure `ollama serve` is running.
- **Poor matches on long functions:** Increase `maxInputBytes` or `contextLength`, or use a model with a larger context window.

### Recommended workflow
1. **Index** after pulling or merging significant changes
2. **Query** the files you're actively editing — catch duplication as you write
3. **Scan** periodically for a full audit
4. **Exclude** false positives to keep results clean over time
