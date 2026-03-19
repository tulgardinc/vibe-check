# Review Notes: git-integration

## Automated Check Results
- TypeCheck: PASS (`cargo check` clean)
- Lint: PASS with warnings (3 clippy warnings in new code, all `collapsible_if` or `manual_div_ceil` -- stylistic, not correctness)
- Tests: PASS (102 unit tests + 9 integration tests, 111 total, 0 failures)
- Vibecheck: not run (requires Ollama for embedding; deferred to manual review below)

## Critical Issues

None found.

## Important Issues

- [ ] `src/core/git_pipeline.rs:40-170 + 175-311` -- **Substantial code duplication between `run_git_query` and `run_commit_query`**. These two functions are ~130 lines each and share roughly 80% of their logic: git repo validation, DB open, embedder resolution, model mismatch check, staleness check, extension filtering, chunk comparison loop structure, empty-result early return, `query_chunks` call, removed-function post-filter, result assembly. They differ only in (a) how diff entries are obtained (`diff_working_tree` vs `diff_commit`) and (b) how file contents are read (`read_working_tree_file`/`show_file("HEAD",...)` vs `show_file(hash,...)`/`show_file(parent,...)` ). Suggested fix: extract a shared `run_diff_query` function parameterized by a closure or trait that provides the diff entries and file reading strategy, then have both public functions delegate to it.

- [ ] `src/core/git_pipeline.rs:156` -- **Unnecessary heap allocation in HashSet lookup**. `all_removed.contains(&(c.path.clone(), c.name.clone()))` allocates two new `String`s for every candidate checked. Since `all_removed` is a `HashSet<(String, String)>`, each `.contains()` call clones the candidate's path and name only to compare them. Suggested fix: change `all_removed` to store `(String, String)` but use `.contains()` with a borrowed tuple via the `Borrow` trait, or build a `HashSet<(&str, &str)>` from `all_removed` before the filter loop. The same issue exists at line 296.

- [ ] `src/core/index_pipeline.rs:314-329` -- **Cache writes inside index DB transaction**. Cache `insert_embedding` calls happen within the `conn.transaction()` scope. If the transaction rolls back (e.g., a later `update_embedding` fails), the cache retains embeddings that the index DB does not have. While D16 acknowledges the cache is a "lookup accelerator" and this is not data-corrupting, it means the cache can grow with orphaned entries that only `prune` can clean up. The architecture says the cache is accelerator-only (FR-32), so this is acceptable but worth documenting. Suggested fix: move cache writes after `tx.commit()` succeeds, or accept this as-is with a code comment noting the intentional tradeoff.

- [ ] `src/util/git.rs:325-329` -- **Staleness warning message deviates from FR-22 specification**. The requirement says: "Index was built at commit <stored_hash> but HEAD is <current_hash>. Results may be incomplete. Run `vibec index` to update." The implementation says: "Index may be stale: it was built at commit {short_hash}, but HEAD is now {short_hash}. Consider re-running `vibec index`." The wording and hash truncation are different. Suggested fix: if the wording is intentional, document the deviation in `decisions.md`. Otherwise, match the spec wording.

## Minor Issues

- [ ] `src/core/index_pipeline.rs:295` -- Clippy: manual `div_ceil` reimplementation. Use `total_misses.div_ceil(INDEX_EMBED_BATCH_SIZE)` instead.

- [ ] `src/core/index_pipeline.rs:346-350` -- Clippy: collapsible `if let Some(cc) = cache_conn { if let Err(e) = ...` can be collapsed to `if let Some(cc) = cache_conn && let Err(e) = ...`.

- [ ] `src/core/index_pipeline.rs:373-377` -- Clippy: collapsible `if is_git_repo(...) { if let Ok(head) = ...` can be collapsed.

- [ ] `src/main.rs:668-673` -- Redundant git repo check. `run_cache_prune_cmd` checks `is_git_repo`, then `resolve_cache_path` calls `get_git_common_dir` (which also fails for non-git), and then `prune_cache` checks `is_git_repo` again. Not harmful, but could be simplified to a single check.

- [ ] `src/core/git_pipeline.rs:324-363` -- `compare_chunks` silently handles duplicate function names (within the same file) by keeping only the last one in the `HashMap`. This is documented as intentional in D6, but a debug-level log when collisions occur would help users diagnose unexpected behavior.

- [ ] `src/util/git.rs:148-170` -- `diff_working_tree` will fail with an error in a repo with no commits (empty repo where HEAD doesn't exist). `git diff HEAD` returns exit code 128 in this case. This is an edge case that probably never matters in practice but could be handled gracefully by returning an empty diff.

- [ ] `tests/integration.rs` -- No integration tests for `run_git_query` or `run_commit_query`. The unit tests for `compare_chunks` are good, but there are no tests for the full git pipeline flow (creating a git repo with commits and running the pipeline). This was noted in the architecture as "may be deferred" for T5, but it would significantly increase confidence.

## Architecture Conformance

- The implementation matches the architecture closely across all 9 tasks (T1-T9).
- All contracts from `architecture.md` are implemented: `GitQueryOptions`, `CommitQueryOptions`, `QueryChunkOptions`, `FileDiff`, `CacheStats`, `PruneResult`, `DiffEntry`, `DiffStatus`, and all public functions.
- The `query_chunks` extraction (T3) was done correctly -- existing `run_query` delegates to it.
- Cache schema matches the spec: `cache_meta` and `embeddings` tables with the documented columns.
- Module structure follows the architecture: `util/git.rs`, `store/cache.rs`, `core/git_pipeline.rs`.
- D7 (MCP deferred) is respected: no MCP changes.
- D8 (no new dependencies) is respected: git via `std::process::Command`.
- One deviation: staleness warning wording differs from FR-22 (see Important Issues above). This is documented in D14 for the scan pipeline case but not for the general wording change.

## Positive Observations

- **Correct `Command::new("git").args(...)` usage throughout**: All git subprocess calls use argument arrays rather than shell string interpolation, eliminating any command injection risk. This is the right pattern.
- **Graceful degradation**: Non-git projects skip all git features cleanly. Cache failures are logged and skipped, never fatal. `check_staleness` returns `None` when not in a git repo or when `head_commit` is absent.
- **Thorough test coverage for the cache layer**: 10 unit tests covering open, close, insert, lookup, settings mismatch, force rebuild, stats, and prune (including edge cases like empty cache, no worktree DB, all referenced, non-git repo).
- **Clean error handling**: All git subprocess calls map stderr to `VibecheckError::Git` with descriptive messages. Cache operations catch and log errors without crashing.
- **Rename handling via decomposition (FR-10)**: Renames in `parse_diff_output` are correctly decomposed into Delete (old path) + Add (new path), which naturally flows through the pipeline without any special-case logic.
- **Initial commit support in `diff_commit`**: Falls back to `git diff-tree --root` when `<hash>^1` fails, handling repos with initial commits correctly.
- **`diff-tree` first-line hash correctly skipped**: `parse_diff_output` skips lines with fewer than 2 tab-separated parts, which naturally handles the commit hash line that `diff-tree --root` prepends.
- **Test quality**: Both unit tests (parser-level `compare_chunks` tests covering all cases) and integration tests (cache hit/miss verification with `CountingMockEmbedder`) are well-designed and test real behavior, not just types.

## Summary

The implementation is solid and ready to ship with minor cleanup. There are no correctness bugs, no security vulnerabilities, and no broken contracts. The code follows established project patterns (pipeline pattern, error handling, DB lifecycle). The main actionable feedback is the code duplication between `run_git_query` and `run_commit_query`, which is a maintainability concern but not a blocker. The clippy warnings are all stylistic. Test coverage is good for the cache and comparison logic; git pipeline integration tests would be valuable but were explicitly deferred in the architecture. Overall assessment: **ready to ship** (with the understanding that the duplication in `git_pipeline.rs` should be addressed in a follow-up).
