---
name: implement-feature
description: Implements an architected feature end-to-end. Writes tests, implements in dependency order, reviews, and documents. Run after /architect-feature. Takes a feature name as argument.
user_invocable: true
---

# Implement Feature

You are orchestrating the full implementation of an architected feature. This is a multi-phase pipeline that writes tests, implements code, reviews, and documents.

## Setup

The argument is the feature name (kebab-case). The feature directory is `docs/features/$ARGUMENTS/`.

1. **Create an isolated worktree** using the `EnterWorktree` tool with the feature name as the worktree name (e.g., name: `$ARGUMENTS`). All implementation work happens in this worktree.

2. Verify these files exist:
   - `docs/features/$ARGUMENTS/spec.md`
   - `docs/features/$ARGUMENTS/requirements.md`
   - `docs/features/$ARGUMENTS/architecture.md`

   If any are missing, tell the user to run the prerequisite steps first.

3. Read `docs/features/$ARGUMENTS/architecture.md` to understand the task breakdown and dependency graph.

4. **Check for prior progress.** If `docs/features/$ARGUMENTS/progress.md` exists:
   - Read it to determine which phases and waves are already complete.
   - Resume from the first incomplete phase — skip all completed phases entirely.
   - Report to user: "Resuming from <phase>. Previously completed: <list>."

5. If no `progress.md` exists, create it:
   ```markdown
   # Progress: $ARGUMENTS

   ## Completed Steps
   ```

## Phase 1: Tests (Red)

Spawn the **test-writer** agent:
- Tell it to read:
  - `docs/features/$ARGUMENTS/architecture.md`
  - `docs/features/$ARGUMENTS/requirements.md`
- Tell it to write test files as specified in the architecture's test strategy
- Tell it to append decisions to `docs/features/$ARGUMENTS/decisions.md`
- Tell it to run tests and confirm they fail (red phase)

Wait for completion. Report to user: "Tests written. X tests, all failing (red). Starting implementation."

Update `progress.md` — append: `- Phase 1: Tests (X tests, all failing)`

## Phase 2: Implementation

Read the task dependency graph from `architecture.md`. Execute tasks in dependency order:

### For each wave of parallelizable tasks:

Identify tasks whose dependencies are all satisfied. For each task in the wave, spawn an **implementor** agent:
- Tell it which task (ID + description) to implement
- Tell it to read:
  - `docs/features/$ARGUMENTS/architecture.md` (focus on its task + contracts)
  - `docs/features/$ARGUMENTS/requirements.md`
  - Relevant test files
- Tell it to append any deviations to `docs/features/$ARGUMENTS/decisions.md`

**Spawn parallel tasks simultaneously** using multiple Agent tool calls in one message.

After each wave completes:
1. Run `npm test` to check progress
2. Stage all changes and create a checkpoint commit: `wip: $ARGUMENTS — wave N (T1, T2)`
3. Update `progress.md` — append: `- Phase 2 Wave N: T1, T2 (X/Y tests passing)`
4. Report to user: "Wave N complete. X/Y tests passing."
5. If a task agent reported a BLOCKER:
   - Read the blocker from `decisions.md`
   - Report it to the user: "Blocker encountered: <description>. Breaking early. See decisions.md for details."
   - **Stop implementation and skip to Phase 4 (Documentation).**

Continue waves until all tasks are complete and all tests pass.

If tests still fail after all tasks are done, make ONE attempt to fix remaining failures by spawning an **implementor** agent focused on the failing tests. If they still fail, document the failures and continue to review.

## Phase 3: Review Cycle (max 3 iterations)

### Iteration loop:

**Step A** — Spawn the **reviewer** agent:
- Tell it to read:
  - `docs/features/$ARGUMENTS/architecture.md`
  - `docs/features/$ARGUMENTS/requirements.md`
- Tell it to review all files changed since implementation started (use git diff)
- Tell it to run vibecheck scan for code duplication
- Tell it to write findings to `docs/features/$ARGUMENTS/review-notes.md`

**Step B** — Read `review-notes.md`. Check the summary:
- If "ready to ship" or no Critical/Important issues → exit loop, go to Phase 4
- If Critical or Important issues exist AND iteration < 3:
  - Report to user: "Review iteration N: X critical, Y important issues found. Fixing..."
  - Spawn the **fixer** agent:
    - Tell it to read `docs/features/$ARGUMENTS/review-notes.md`
    - Tell it to read `docs/features/$ARGUMENTS/architecture.md`
    - Tell it which iteration this is (1, 2, or 3)
    - Tell it to append fix details to `docs/features/$ARGUMENTS/decisions.md`
  - After fixer completes, stage and commit fixes: `wip: $ARGUMENTS — review fixes (iteration N)`
  - Loop back to Step A
- If iteration = 3:
  - Report: "Final review iteration. Remaining issues documented as tech debt."
  - Exit loop

After the review cycle completes, update `progress.md` — append: `- Phase 3: Review (N iterations, outcome: <ready/tech-debt>)`

## Phase 4: Documentation

Spawn the **documenter** agent:
- Tell it to read all files in `docs/features/$ARGUMENTS/`
- Tell it to write `docs/features/$ARGUMENTS/summary.md`

After completion, update `progress.md` — append: `- Phase 4: Documentation`

## Phase 5: Update Feature Index

After documentation is complete, update `docs/features/INDEX.md`:

1. Read `docs/features/$ARGUMENTS/summary.md` to identify which systems were affected.
2. Map affected systems to their memory names (e.g., turn pipeline → `turn-pipeline`, item/inventory → `item-inventory`). Use the system names from `MEMORY.md`.
3. Append a new row to the table in `docs/features/INDEX.md`:
   ```
   | [$ARGUMENTS]($ARGUMENTS/) | <today's date> | <comma-separated system names> | <one-line description> |
   ```
4. If `docs/features/INDEX.md` doesn't exist, create it with a header row and the new entry.

After completion, update `progress.md` — append: `- Phase 5: Index`

## Phase 6: Commit

After the feature index is updated:

1. Stage all remaining changes (review fixes, docs, index — implementation code was already committed in checkpoint commits).
2. Create a final commit with the message:
   ```
   feat: $ARGUMENTS — <one-line description from summary>
   ```
   Use the "What Was Built" section of `summary.md` to write a concise description.
3. Update `progress.md` — append: `- Phase 6: Commit`

## Completion

After the commit:
1. Read `docs/features/$ARGUMENTS/summary.md`
2. Present a final summary to the user:
   - What was built
   - Test status
   - Any known issues or tech debt
   - Any blockers encountered
3. Remind the user that implementation was done in a worktree. They can review with `git diff` and merge when ready.
