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
