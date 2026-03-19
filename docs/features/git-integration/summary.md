# Summary: git-integration

## What Was Built

Git-aware duplicate detection for vibecheck. The feature adds two query commands (`vibec git`, `vibec commit`) that check only changed code for duplicates rather than requiring a full file query. It adds a shared embedding cache at `.git/vibecheck-cache.db` that is shared across all worktrees of a repository, eliminating redundant Ollama calls when switching branches. It also adds index staleness warnings on any command that queries the index, `vibec status` cache reporting, and `vibec cache prune` for manual cache cleanup.

## Files Changed

**New files (5):**
- `src/util/git.rs` — Git subprocess wrappers: `is_git_repo`, `get_head_commit`, `get_git_common_dir`, `diff_working_tree`, `diff_commit`, `show_file`, `read_working_tree_file`, `check_staleness`
- `src/store/cache.rs` — Shared embedding cache: open/close, lookup, insert, stats, prune (discovers all worktrees via `git worktree list --porcelain`)
- `src/core/git_pipeline.rs` — `run_git_query`, `run_commit_query`, shared private `run_diff_query`, `compare_chunks`
- `docs/features/git-integration/` — feature documentation (spec, requirements, architecture, decisions, review-notes, progress)
- `tests/integration.rs` — 9 integration tests (cache hit/miss with `CountingMockEmbedder`)

**Modified files (13):**
- `src/core/query_pipeline.rs` — Extracted `query_chunks()` and `QueryChunkOptions` for reuse by git pipeline
- `src/core/index_pipeline.rs` — Cache integration in embedding loop; `head_commit` stored in `index_meta` after index
- `src/core/scan_pipeline.rs` — Staleness warning via `logger::warn`
- `src/core/status_pipeline.rs` — Cache fields added to `StatusResult`
- `src/core/mod.rs` — `pub mod git_pipeline`
- `src/store/mod.rs` — `pub mod cache`
- `src/util/mod.rs` — `pub mod git`
- `src/util/config.rs` — `resolve_cache_path()` via `git rev-parse --git-common-dir`
- `src/error.rs` — `Git(String)` variant
- `src/main.rs` — `Git`, `Commit`, `Cache Prune` subcommands
- `src/output/types.rs` — `cache_exists`, `cache_path`, `cache_entry_count`, `cache_size_bytes` on `StatusResult`
- `src/output/formatter.rs` — Cache fields in status human output

**Test count:** 102 unit tests + 9 integration tests = 111 total, all passing.

## Key Decisions

1. **Cache location**: `.git/vibecheck-cache.db` directly in the git common dir (resolved via `git rev-parse --git-common-dir`). Simple and discoverable; the common dir is the same for all worktrees.

2. **Cache prune semantics**: Prune deletes entries not referenced by ANY worktree's DB (not just the current one). Pruning based on a single worktree would evict embeddings other worktrees still need.

3. **Diff command**: `git diff HEAD` for `vibec git`, capturing both staged and unstaged changes in one command.

4. **Reuse query logic via extraction**: `query_chunks()` was extracted from `query_pipeline.rs` and called by both `run_query` and the git pipeline. One code path means bug fixes apply everywhere.

5. **Git pipeline returns `QueryResult`**: Same type as `vibec query`. Existing formatters and any tooling built on the output format work unchanged.

6. **Function comparison by name + content hash**: `compare_chunks` matches by `function_name` within a file; same name + different SHA-256 of source text = modified; name disappeared = removed; new name = added. Simpler than AST comparison, sufficient for the use case. Duplicate function names in the same file: last one wins silently.

7. **No new Cargo dependencies**: Git interaction uses `std::process::Command`. Consistent with the zero-cloud-dependency design.

8. **MCP server deferred**: `vibec git` and `vibec commit` are CLI-only in this iteration. The pipeline functions are structured to be exposed as MCP tools later.

9. **Scan pipeline staleness via logger::warn**: `ScanResult` has no `warnings` field; adding one would be a cross-cutting output change. Logging to stderr is sufficient for a CLI command.

10. **Cache writes inside index DB transaction**: Cache `insert_embedding` calls happen within the `conn.transaction()` scope. If the transaction rolls back, the cache retains orphaned entries. Accepted -- the cache is a lookup accelerator and `prune` handles cleanup.

## Test Coverage

- **Cache layer**: 10 unit tests covering open, close, insert, lookup, settings mismatch, force rebuild, stats, and prune (including empty cache, no worktree DB, all-referenced, non-git repo cases).
- **`compare_chunks`**: Unit tests covering all four classifications: added, modified, removed, unchanged.
- **Git utils**: Unit tests for output parsing logic in `git.rs`.
- **Integration**: 9 tests using `CountingMockEmbedder` to verify cache hit/miss rates and embedding call counts.
- **Not covered**: Full end-to-end `run_git_query` and `run_commit_query` pipeline tests (no tests create an actual git repo with commits and run the pipeline). Deferred per the original architecture note for T5. The `compare_chunks` unit tests and cache integration tests provide meaningful coverage of the two most complex subsystems.

## Known Issues & Tech Debt

- **No integration tests for git pipeline**: `run_git_query` and `run_commit_query` have no tests that exercise the full flow against a real git repo. A `tempfile` git repo fixture would significantly increase confidence.
- **`diff_working_tree` fails in empty repos**: `git diff HEAD` exits with code 128 in a repo with no commits. Edge case, but currently surfaces as an error rather than an empty diff.
- **`compare_chunks` silent collision**: Duplicate function names in the same file silently keep the last entry. A debug-level log on collision would help diagnose unexpected behavior.
- **Redundant git repo checks in prune flow**: `run_cache_prune_cmd` checks `is_git_repo`, then `resolve_cache_path` calls `get_git_common_dir` (also fails for non-git), and then `prune_cache` checks again. Not harmful, minor cleanup opportunity.
- **Pre-existing clippy warnings**: Collapsible `if` chains in `parser/chunker.rs` and `parser/typescript.rs` are pre-existing and not part of this feature.

## Deviations from Spec

- **Staleness warning wording** (RF-4 fixed): The initial implementation used truncated 8-char hashes and different phrasing. Fixed in review iteration 1 to match FR-22 exactly: "Index was built at commit \<stored_hash\> but HEAD is \<current_hash\>. Results may be incomplete. Run `vibec index` to update."
- **`run_diff_query` extraction** (RF-1): Architecture described `run_git_query` and `run_commit_query` as separate full implementations. In practice, the ~80% shared logic was extracted into a private `run_diff_query` function parameterized by closures for diff-entry retrieval and file reading. This is a better design than what the architecture specified.
- **Scan pipeline staleness**: Architecture suggested adding staleness warnings to `scan_pipeline.rs` via a warnings vec on `ScanResult`. Implemented via `logger::warn` instead to avoid modifying `ScanResult` and all its downstream formatters (D14).

## Developer Notes

- The cache is an **accelerator only** — it is never the source of truth for which functions exist. The per-worktree `.vibecheck.db` is authoritative. The cache can have orphaned entries (from transaction rollbacks or deleted functions); `vibec cache prune` cleans them up.
- All git subprocess calls use `Command::new("git").args([...])` with argument arrays, never shell string interpolation. Maintain this pattern to avoid command injection.
- `check_staleness` returns `None` when not in a git repo or when `head_commit` is absent from `index_meta` -- it is always safe to call.
- Cache and database connections both follow the same lifecycle: `open_*` -> work -> `close_*` (WAL checkpoint before close). The WAL checkpoint on close is important for concurrent worktree access; see the fix in commit 27772af.
- `compare_chunks` operates on `FunctionChunk` structs before they touch the DB and computes SHA-256 of `source_text` directly (D12). It does not use the `content_hash` from the DB, because chunks at this stage have not been upserted yet.
