# Requirements: Git Integration

## Functional Requirements

### FR-G: `vibec git` command

FR-1: `vibec git` diffs the working tree against HEAD, capturing both staged and unstaged changes in a single combined diff

FR-2: For each file present in the diff, `vibec git` parses the current working-tree version using tree-sitter to extract all function chunks

FR-3: `vibec git` identifies which extracted functions are added or modified by comparing the current working-tree function set against the HEAD version of each changed file; functions present in HEAD but not in the current version are classified as removed

FR-4: `vibec git` embeds only added/modified functions and runs KNN queries against the current index — it does not re-embed unchanged functions

FR-5: `vibec git` excludes from results any candidate whose (file_path, function_name) pair matches a function classified as removed in the current diff

FR-6: `vibec git` outputs results in the same format as `vibec query`: grouped by query function with candidates and similarity scores

FR-7: When the working tree has no changes relative to HEAD, `vibec git` outputs zero results and exits with code 0

FR-8: `vibec git` run outside a git repository exits with a non-zero code and prints: "Not a git repository. These commands require git."

FR-9: Binary files present in the diff are silently skipped

FR-10: Renamed files (via `git mv`) are handled correctly without special-case logic: old path functions are treated as removed (filtered), new path functions are treated as added (queried)

### FR-C: `vibec commit <hash>` command

FR-11: `vibec commit <hash>` computes the diff for the specified commit against its first parent (`<hash>^1..<hash>`)

FR-12: For each file in the diff, `vibec commit <hash>` retrieves post-commit file contents via `git show <hash>:<file>` and pre-commit contents via `git show <hash>^1:<file>`

FR-13: `vibec commit <hash>` identifies added/modified functions using the same comparison logic as FR-3, applied to the post- vs. pre-commit versions

FR-14: `vibec commit <hash>` embeds only added/modified functions and queries them against the current index

FR-15: `vibec commit <hash>` applies the same removed-function candidate filter as FR-5

FR-16: `vibec commit <hash>` outputs results in the same format as `vibec query`

FR-17: For merge commits, `vibec commit <hash>` diffs against the first parent only

FR-18: `vibec commit <hash>` works for any reachable commit regardless of current branch

FR-19: `vibec commit <hash>` run outside a git repository exits with a non-zero code and prints: "Not a git repository. These commands require git."

### FR-S: Index staleness warning

FR-20: `vibec index` stores the current HEAD commit hash in `index_meta` under the key `head_commit`

FR-21: On `vibec git`, `vibec commit`, `vibec query`, and `vibec scan`, the stored `head_commit` value is compared to the actual current HEAD

FR-22: When the stored and actual HEAD hashes differ, a warning is emitted: "Index was built at commit <stored_hash> but HEAD is <current_hash>. Results may be incomplete. Run `vibec index` to update."

FR-23: The staleness warning does not prevent the command from running or change the exit code

FR-24: If `head_commit` is absent from `index_meta` (index predates this feature), no staleness warning is emitted

### FR-E: Shared embedding cache

FR-25: On the first `vibec index` run in a git repository, a shared cache database is created under `git rev-parse --git-common-dir`

FR-26: The shared cache stores: `content_hash` (TEXT PRIMARY KEY, SHA-256) and `embedding` (BLOB)

FR-27: The shared cache records at creation: model name, embedding dimensions, and max_input_bytes

FR-28: During `vibec index`, for each parsed function the embedder checks the shared cache by content hash; a cache hit supplies the embedding without calling Ollama

FR-29: On a cache miss, the embedding is obtained from Ollama and written to the shared cache

FR-30: If model name or dimensions differ from those in the shared cache, the command errors: "Cache was created with model <X>, dimensions <Y>. Run with `--force` to rebuild."

FR-31: When `--force` is passed with mismatched cache settings, the shared cache is rebuilt from scratch

FR-32: The per-worktree `.vibecheck.db` remains authoritative for which functions exist; the cache is a lookup accelerator only

FR-33: In non-git projects, no shared cache is created

FR-34: In worktrees, the cache resolves via `git rev-parse --git-common-dir` to the main `.git/` directory

### FR-P: `vibec cache prune` command

FR-35: `vibec cache prune` removes entries from the shared cache and reports entries removed and bytes freed

FR-36: `vibec cache prune` outside a git repository errors

FR-37: `vibec cache prune` when no shared cache exists errors

### FR-T: `vibec status` cache reporting

FR-38: `vibec status` includes cache entry count and disk size when a shared cache exists

FR-39: `vibec status` shows cache fields as absent/zero when no shared cache exists

### FR-B: Backward compatibility

FR-40: All existing commands behave identically in non-git projects

FR-41: All existing commands function in git projects without a shared cache present

---

## Non-Functional Requirements

NFR-1: Concurrent `vibec index` runs sharing the same cache must not corrupt it; use WAL mode and retry on SQLITE_BUSY

NFR-2: Cache hit rate >80% for a branch with <20% changed files in a fresh worktree

NFR-3: Git is invoked as subprocess (`git` on PATH); no libgit2 or git Rust library dependency

NFR-4: Removed-function filter operates in O(n) via HashSet on (file_path, function_name)

---

## Constraints

C-1: `head_commit` is stored in existing `index_meta` key-value table; no new DB table

C-2: Shared cache is a separate SQLite file, not embedded in `.vibecheck.db`

C-3: `vibec git` and `vibec commit` reuse the existing query pipeline's KNN + re-ranking + exclusion logic

C-4: Output format matches `vibec query` (`QueryResult` JSON and human format)

C-5: Git subprocess calls use `git` on PATH; no bundled binary

C-6: New errors extend `VibecheckError`; a `Git` variant is acceptable

C-7: `find_project_root` worktree detection must not be broken

C-8: Shared cache connection uses same WAL + foreign key + busy-timeout config as `open_database`

C-9: Shared cache uses same checkpoint-then-close pattern as `close_database`

---

## Acceptance Criteria

### vibec git

AC-1: Modified file with two functions (one changed, one unchanged) — only changed function is queried
AC-2: Deleted file — no queries for its functions, no candidates pointing to them
AC-3: Clean working tree — zero query functions, exit code 0
AC-4: Index at current HEAD — no staleness warning
AC-5: File rename (`git mv`) — old path functions filtered, new path functions queried
AC-6: Non-git directory — non-zero exit, "Not a git repository" message
AC-7: Binary file in diff — silently skipped, other results unaffected

### vibec commit

AC-8: Commit adding one function — exactly that function queried
AC-9: Merge commit — diff against first parent only
AC-10: Commit from another branch — completes without error
AC-11: Non-git directory — non-zero exit, "Not a git repository" message

### Staleness warning

AC-12: Index at commit A, HEAD at commit B — warning appears on git/commit/query/scan
AC-13: Index at HEAD — no warning
AC-14: No `head_commit` in index_meta — no warning
AC-15: Warning doesn't change exit code

### Shared embedding cache

AC-16: Mismatched model — error with "Cache was created with model..."
AC-17: Mismatched model + `--force` — cache rebuilt, exit 0
AC-18: Fresh worktree with 80% shared content — >80% cache hit rate
AC-19: Concurrent index runs — no corruption
AC-20: Non-git project — no cache created
AC-21: Git worktree — cache in main `.git/` via `--git-common-dir`

### Status and cache prune

AC-22: Status shows cache entry count and disk size
AC-23: Non-git project status — no cache fields or zero
AC-24: Cache prune reports entries removed and bytes freed
AC-25: Cache prune outside git repo — error

### Backward compatibility

AC-26: Non-git project — all commands behave identically to pre-feature
AC-27: Git project without cache file — query works without error

---

## Open Questions (must resolve before implementation)

1. **Cache prune semantics**: Delete entries not in current worktree's DB, or not in any worktree's DB?
2. **Cache file path**: `.git/vibecheck-cache.db` vs. `.git/vibecheck/cache.db`
3. **Diff command**: Confirm `git diff HEAD` captures both staged and unstaged
