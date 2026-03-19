# Feature: Git Integration

## Problem Statement

Vibecheck's index is a point-in-time snapshot with no awareness of git state. After branch switches, the index silently goes stale. Users must manually re-index after every checkout. There's no way to check only *changed* code for duplicates — you either query a whole file or scan everything. Cross-worktree usage requires a full re-index per worktree, which is expensive.

This feature makes vibecheck git-aware: it adds commands to query only changed code, warns when the index is stale, and shares embedding computation across worktrees via a content-addressed cache.

## User Story

As a developer working across branches and worktrees, I want to check my changed code for duplicates without re-indexing everything, so that I catch copy-paste before it lands — fast and cheap.

## Existing System Context

- **Index pipeline** (`core/index_pipeline.rs`): Incremental indexing via `file_tracker.rs` (mtime + content hash). Stores model name, dimensions, and signature hash version in `index_meta`. No git state tracked.
- **Query pipeline** (`core/query_pipeline.rs`): Takes a source file, parses all functions, embeds them, runs KNN against the index. Returns `QueryResult` with `QueryFunction` entries and candidates.
- **Database** (`store/db.rs`): Per-project `.vibecheck.db` at project root. `tracked_files` table with cascade delete to `functions`. `index_meta` key-value table.
- **Project root** (`util/config.rs`): `find_project_root` walks up looking for `.git` or `package.json`. In worktrees, `.git` is a file — `.exists()` still returns true, so each worktree resolves to its own root and gets its own `.vibecheck.db`.
- **Embedding** (`embedder/`): Ollama HTTP client. Batch embedding. Model auto-detection. Embedding is the expensive operation (seconds to minutes for a full codebase).

## UX Walkthrough

### `vibec git`

1. User makes changes on a branch (modifies files, adds files, deletes files) and runs `vibec git`.
2. Vibecheck diffs the working tree against HEAD (staged + unstaged combined).
3. For each changed file, parses the **current** (working tree) version to extract all function chunks.
4. Identifies which functions are added or modified vs. the HEAD version by comparing against the old (HEAD) version of each changed file.
5. Embeds only the added/modified functions and queries them against the current index.
6. Filters results: any candidate pointing to a function that was **removed** in the diff is excluded (matched by file_path + function_name).
7. Outputs results in the same format as `vibec query` — grouped by query function, showing candidates with similarity scores.
8. If the index was not built at the current HEAD commit, a warning is shown: "Index was built at commit abc123 but HEAD is def456. Results may be incomplete. Run `vibec index` to update."

### `vibec commit <hash>`

1. User runs `vibec commit abc123` to check whether a past commit introduced duplicates of code that exists today.
2. Vibecheck computes the diff for that commit against its first parent (`<hash>^1..<hash>`).
3. For each changed file, retrieves the post-commit file contents via `git show <hash>:<file>`.
4. Parses to extract function chunks, identifies added/modified functions by comparing against the pre-commit version (`git show <hash>^1:<file>`).
5. Embeds added/modified functions and queries them against the **current** index.
6. Same removed-function filtering as `vibec git`.
7. Same output format as `vibec query`.
8. Same staleness warning (index HEAD vs current HEAD).
9. Merge commits: diffs against first parent, which represents "what did this merge bring into the target branch."

### Index staleness warning

1. During `vibec index`, the current HEAD commit hash is stored in `index_meta` (key: `head_commit`).
2. On `vibec git`, `vibec commit`, `vibec query`, and `vibec scan`, the stored HEAD is compared to the actual current HEAD.
3. If they differ, a warning is emitted. The command still runs.

### Shared embedding cache

1. On first `vibec index` in a git repo, a shared cache is created at `.git/vibecheck-cache.db` (or similar location under `.git/`).
2. The cache stores: `content_hash` (SHA256 of function source text) -> `embedding` (vector bytes).
3. Cache-level settings (model name, dimensions, max_input_bytes) are locked on creation. Subsequent runs with different settings error: "Cache was created with model X, dimensions Y. Run with `--force` to rebuild."
4. During indexing, for each parsed function: compute content hash, check shared cache. Cache hit -> use cached embedding. Cache miss -> call Ollama, store result in cache.
5. The per-worktree `.vibecheck.db` remains the source of truth for which functions currently exist. The shared cache is just an embedding lookup accelerator.
6. In non-git projects, no shared cache is created. Embeddings are stored in the per-project `.vibecheck.db` only.
7. `vibec status` includes cache size information (entry count, disk size).
8. `vibec cache prune` allows manual cache cleanup.

### Graceful degradation without git

- `vibec index`, `query`, `scan`, `status` work exactly as today in non-git projects. No shared cache, no staleness warning.
- `vibec git` and `vibec commit` error with: "Not a git repository. These commands require git."

## Edge Cases & Failure Modes

- **Renamed files (`git mv`)**: Diff shows old path deleted, new path added. All functions in old path are "removed" (filtered from results). All functions in new path are "added" (queried). Matches between old and new are suppressed by the removed-function filter. Works correctly without special handling.
- **Merge commits**: `vibec commit <hash>` diffs against first parent. This shows what the merge brought into the target branch, including conflict resolution code.
- **Commit not in current branch history**: `vibec commit <hash>` works regardless — git can compute the diff for any reachable commit. The query runs against the current index. No special handling needed.
- **No changes in working tree**: `vibec git` with a clean working tree outputs no results (no functions to query). Not an error.
- **Binary files in diff**: Skipped — tree-sitter can't parse them, and they won't match any supported language extension.
- **Large diffs**: Many files changed (e.g., after a rebase). All modified functions are queried. Could be slow if hundreds of functions changed. No special batching needed — the existing batch embedding handles it.
- **Concurrent cache writes**: Two worktrees run `vibec index` simultaneously. SQLite WAL mode handles concurrent reads. Concurrent writes may hit `SQLITE_BUSY` — retry with backoff.
- **Ollama model updated (same name, new weights)**: Cached embeddings become semantically stale. `--force` is the escape hatch. Document: "If you update your Ollama model, re-index with `--force`."
- **Cache growth over time**: Embeddings from deleted functions and old branches accumulate. `vibec cache prune` is the manual cleanup path. `vibec status` shows cache size so users can monitor.
- **Shared cache location in worktrees**: Use `git rev-parse --git-common-dir` to find the main `.git/` directory, not the worktree-specific `.git` file.

## Future Direction

- **Automatic re-indexing**: A git post-checkout hook that runs `vibec index` incrementally after branch switches. The shared cache makes this cheap.
- **PR-level scanning**: `vibec pr` that queries all changes in the current branch vs. the base branch (e.g., main). Natural extension of `vibec git` scoped to a branch diff rather than uncommitted changes.
- **Cache GC strategies**: Automatic eviction of embeddings not referenced by any worktree's DB. Requires discovering all worktrees (`git worktree list`) and cross-referencing their function tables.
- **CI integration**: `vibec commit HEAD` in CI to catch duplicates in every push. The shared cache isn't available in CI, but a remote cache (S3, etc.) could serve the same role.
- **Content-addressed index sharing**: Instead of per-worktree DBs, a single shared index that tracks which functions belong to which branch/commit. More complex but eliminates per-worktree indexing entirely. Deferred in favor of the simpler shared-cache approach.

## Open Questions

- **Cache location**: `.git/vibecheck-cache.db` vs. `.git/vibecheck/cache.db` vs. another location. Should be discoverable via `git rev-parse --git-common-dir`.
- **Cache prune semantics**: Should `vibec cache prune` delete all entries not referenced by the current worktree's DB? Or require no worktree references them at all? The former is simpler but could evict entries another worktree needs.
- **Diff strategy for `vibec git`**: Use `git diff HEAD` (combined staged + unstaged) or run `git diff HEAD` which already captures both? Need to confirm the exact git plumbing command.

## Success Criteria

- SC-1: `vibec git` correctly identifies added/modified functions from uncommitted changes and returns duplicate candidates from the index.
- SC-2: `vibec git` does not return candidates pointing to functions removed in the current diff.
- SC-3: `vibec commit <hash>` correctly extracts added/modified functions from a specific commit and queries them against the current index.
- SC-4: `vibec commit <hash>` handles merge commits by diffing against the first parent.
- SC-5: A staleness warning is shown when the index's stored HEAD commit differs from the current HEAD, on any command that queries the index.
- SC-6: The shared embedding cache reduces re-indexing time in a new worktree by reusing embeddings for unchanged functions (measured: >80% cache hit rate for a branch with <20% changed files).
- SC-7: `vibec index` with mismatched cache settings (different model or dimensions) produces a clear error message.
- SC-8: All existing commands (`index`, `query`, `scan`, `status`) continue to work identically in non-git projects.
- SC-9: `vibec git` and `vibec commit` error clearly when run outside a git repository.
- SC-10: `vibec status` reports cache size (entry count and disk size).
- SC-11: `vibec cache prune` removes entries from the shared cache and reports how much was freed.
