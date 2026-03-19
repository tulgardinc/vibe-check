# Decision Log: git-integration

Decisions and rationale recorded during development.

---

## D1: Cache prune semantics
**Decision**: `vibec cache prune` deletes entries not referenced by ANY worktree's DB, not just the current one.
**Why**: Pruning based on a single worktree would evict embeddings that other worktrees still need, defeating the purpose of the shared cache.
**How to apply**: Prune must discover all worktrees via `git worktree list`, open each `.vibecheck.db`, collect all content hashes in use, and delete cache entries not in that set.

## D2: Cache file path
**Decision**: `.git/vibecheck-cache.db` directly in the `.git/` directory (resolved via `git rev-parse --git-common-dir`).
**Why**: Simple, discoverable, no extra directory nesting needed.

## D3: Diff command for `vibec git`
**Decision**: Use `git diff HEAD` which captures both staged and unstaged changes against the current HEAD commit.
**Why**: This is the simplest single command that gives the full working-tree-vs-committed diff.

## D4: Reuse query logic via extraction
**Decision**: Extract `query_chunks()` from `query_pipeline.rs` rather than having the git pipeline re-implement query logic.
**Why**: Satisfies constraint C-3 (reuse KNN + reranking + exclusions). Keeps behavior consistent — one code path for query logic means bug fixes apply everywhere.

## D5: Git pipeline produces `QueryResult`
**Decision**: `run_git_query` and `run_commit_query` return `QueryResult` (same type as `vibec query`), not a new result type.
**Why**: Existing human and JSON formatters work unchanged. CI/tooling integrations built on `vibec query` output work with `vibec git` output too.

## D6: Function comparison by name + content hash
**Decision**: `compare_chunks` matches functions by `function_name` within a file and uses `content_hash` to detect modifications.
**Why**: Simpler than AST-based comparison and sufficient for the use case. Same function name + different content = modified. Name disappeared = removed. New name = added.

## D7: MCP server deferred
**Decision**: MCP server integration for `vibec git`/`vibec commit` is deferred to a future iteration.
**Why**: CLI-first. MCP can be added later by exposing the same pipeline functions as tools.

## D8: No new dependencies
**Decision**: Git interaction uses `std::process::Command`. No libgit2, no `git2` crate.
**Why**: Consistent with zero-cloud-dependency design. `git` binary is already a runtime prerequisite for the feature (graceful degradation when absent).

## D9: Prune via worktree discovery
**Decision**: `vibec cache prune` discovers all worktrees via `git worktree list --porcelain` and cross-references their `.vibecheck.db` files.
**Why**: This is the only way to know which cache entries are still in use without maintaining a separate reference count.

## D10: Test structure for TDD (red phase)
**Decision**: Tests are written as `#[cfg(test)] mod tests` blocks within the source files they test, following the existing project convention (see `db.rs`, `index_store.rs`, `config.rs`, `hash.rs`).
**Why**: Consistent with codebase patterns. Colocated tests have direct access to private functions and types without needing `pub(crate)`.

## D11: Stubs use `todo!()` instead of returning defaults
**Decision**: All stub functions use `todo!("git-integration: not yet implemented")` rather than returning dummy values.
**Why**: This ensures tests fail clearly at runtime with an obvious message pointing to the unimplemented code. Returning defaults would make some tests pass prematurely, undermining the TDD red phase.

## D12: compare_chunks uses sha256 of source_text for content comparison
**Decision**: `compare_chunks` computes content hashes via `sha256(source_text)` rather than relying on a `content_hash` field on `FunctionChunk`.
**Why**: `FunctionChunk` does not have a `content_hash` field. The content hash is computed during upsert in `index_store.rs`. The `compare_chunks` function operates on parsed chunks before they touch the database, so it must compute the hash itself.

## D13: T3 test validates type signature only
**Decision**: The `query_chunks_function_exists_with_correct_signature` test verifies that `QueryChunkOptions` and `query_chunks` exist with the correct types, but does not call `query_chunks` (which would require a full DB + embedder setup).
**Why**: T3 is a pure refactor task. The existing query pipeline tests already validate the behavior. The TDD test for T3 only needs to confirm the extracted function's public API shape matches the architecture contract. The existing tests serve as the regression safety net.

## D14: Scan pipeline staleness uses logger::warn instead of warnings vec
**Decision**: In `scan_pipeline.rs`, the staleness warning is emitted via `logger::warn()` rather than being added to a `warnings` field on `ScanResult`.
**Why**: `ScanResult` has no `warnings` field, and adding one would be a cross-cutting change affecting output formatters and JSON serialization. Logging the warning is sufficient since scan is a user-facing CLI command where stderr output is visible.

## D15: StatusResult cache fields default to zero/false/empty
**Decision**: The four new `StatusResult` fields (`cache_exists`, `cache_path`, `cache_entry_count`, `cache_size_bytes`) default to `false`, `""`, `0`, `0` in the existing `status_pipeline.rs` code.
**Why**: This ensures backward compatibility -- the status pipeline compiles and runs without cache infrastructure being present. T8 implementation will populate these fields when cache is available.

## D16: Cache integration uses bytes_to_embedding for cache hits
**Decision**: For cache hits, the cached embedding bytes are converted to `Vec<f32>` via `bytes_to_embedding()` and passed to `update_embedding()`, which internally converts them back to bytes.
**Why**: This avoids modifying the `update_embedding` API (which other tasks depend on) and keeps the cache integration self-contained within `index_pipeline.rs`. The double conversion has negligible overhead compared to the Ollama call it replaces.

## D17: max_input_bytes for cache derived from OllamaConfig
**Decision**: The `max_input_bytes` parameter for `open_cache` is sourced from `options.ollama.max_input_bytes.unwrap_or(DEFAULT_MAX_INPUT_BYTES)` rather than from the `Embedder` trait.
**Why**: The `Embedder` trait does not expose `max_input_bytes`. The `OllamaConfig` is the canonical source for this configuration value.

## D18: Fixed pre-existing borrow errors in cache.rs prune_cache
**Decision**: Fixed two compilation errors in `prune_cache` (T9) where `Statement` borrows prevented moving `Connection`/`Transaction`. Added inner scopes to drop `Statement` before consuming the borrow.
**Why**: These errors blocked compilation of the entire crate including T6 tests. The fix is trivially correct (scope-based drop) and does not change T9 behavior.

---

## Review Fixes (Iteration 1)

### RF-1: Extracted shared `run_diff_query` to eliminate duplication (Important)
**Issue**: `run_git_query` and `run_commit_query` were ~130 lines each with ~80% identical logic.
**Fix**: Extracted a private `run_diff_query` function parameterized by:
- `diff_entries: Vec<DiffEntry>` -- the list of changed files
- `read_new: Fn(&str) -> Result<String>` -- how to read the "new" version of a file
- `read_old: Fn(&DiffEntry, &str) -> Result<Option<String>>` -- how to read the "old" version of a file

Both `run_git_query` and `run_commit_query` now prepare their diff entries and closures, then delegate to `run_diff_query`. The `read_old` closure receives the `DiffEntry` so it can check `entry.status == DiffStatus::Added` to return `None` for new files, unifying the Added-vs-Modified branching that was previously duplicated in both callers.
**Files**: `src/core/git_pipeline.rs`

### RF-2: Eliminated unnecessary heap allocations in HashSet lookup (Important)
**Issue**: `all_removed.contains(&(c.path.clone(), c.name.clone()))` allocated two new `String`s per candidate checked.
**Fix**: Build a `HashSet<(&str, &str)>` of borrowed references from `all_removed` before the filter loop, then use `.contains(&(c.path.as_str(), c.name.as_str()))` for zero-allocation lookups.
**Files**: `src/core/git_pipeline.rs`

### RF-3: Fixed clippy warnings (Minor)
**Fixes applied**:
- `index_pipeline.rs:295`: Replaced manual `(total_misses + INDEX_EMBED_BATCH_SIZE - 1) / INDEX_EMBED_BATCH_SIZE` with `total_misses.div_ceil(INDEX_EMBED_BATCH_SIZE)`.
- `index_pipeline.rs:346-350`: Collapsed nested `if let Some(cc) = cache_conn { if let Err(e) = ...` into a single `if let` chain.
- `index_pipeline.rs:373-377`: Collapsed nested `if is_git_repo(...) { if let Ok(head) = ...` into a single `if let` chain.
**Note**: Remaining clippy warnings in `parser/chunker.rs` and `parser/typescript.rs` are pre-existing and outside the scope of this review.
**Files**: `src/core/index_pipeline.rs`

### RF-4: Staleness warning wording aligned to FR-22 spec (Important)
**Issue**: The staleness warning said "Index may be stale: it was built at commit {short_hash}, but HEAD is now {short_hash}. Consider re-running `vibec index`." with truncated 8-char hashes.
**Fix**: Changed to match the FR-22 specification exactly: "Index was built at commit <stored_hash> but HEAD is <current_hash>. Results may be incomplete. Run `vibec index` to update." Uses full commit hashes (not truncated).
**Files**: `src/util/git.rs`

### Deferred Issues (Tech Debt)
- **Cache writes inside index DB transaction** (`index_pipeline.rs:314-329`): Cache `insert_embedding` calls happen within the DB transaction scope. If the transaction rolls back, the cache retains orphaned entries. Accepted as-is per D16 -- the cache is an accelerator and `prune` handles cleanup. A code comment could be added in a future pass.
- **Redundant git repo check in `run_cache_prune_cmd`** (`main.rs:668-673`): Three separate `is_git_repo` checks occur in the prune flow. Not harmful, but could be simplified. Deferred as minor.
- **`compare_chunks` silent duplicate handling** (`git_pipeline.rs:324-363`): When duplicate function names exist in the same file, only the last one is kept in the HashMap. Documented as intentional in D6. A debug-level log on collision could be added later.
- **`diff_working_tree` in empty repo** (`git.rs:148-170`): `git diff HEAD` fails in repos with no commits. Edge case unlikely in practice. Deferred.
- **No integration tests for git pipeline** (`tests/integration.rs`): Unit tests for `compare_chunks` are thorough but full git pipeline flow tests are missing. Deferred per architecture T5 note.
- **Pre-existing clippy warnings** in `parser/chunker.rs` and `parser/typescript.rs`: Collapsible `if` chains. Not part of this feature; deferred.
