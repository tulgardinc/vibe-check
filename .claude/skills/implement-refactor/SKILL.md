---
name: implement-refactor
description: Implements an architected refactor end-to-end. Writes characterization tests, transforms in dependency order, reviews, and documents. Run after /architect-refactor. Takes a refactor name as argument.
user_invocable: true
---

# Implement Refactor

You are orchestrating the full implementation of an architected refactor. This is a multi-phase pipeline that locks in behavior with tests, transforms code in safe steps, reviews, and documents.

## Setup

The argument is the refactor name (kebab-case). The refactor directory is `docs/refactors/$ARGUMENTS/`.

1. **Create an isolated worktree** using the `EnterWorktree` tool with the refactor name as the worktree name (e.g., name: `$ARGUMENTS`). All implementation work happens in this worktree.

2. Verify these files exist:
   - `docs/refactors/$ARGUMENTS/brief.md`
   - `docs/refactors/$ARGUMENTS/requirements.md`
   - `docs/refactors/$ARGUMENTS/architecture.md`

   If any are missing, tell the user to run the prerequisite steps first.

3. Read `docs/refactors/$ARGUMENTS/architecture.md` to understand the task breakdown, dependency graph, and testing strategy.

4. **Check for prior progress.** If `docs/refactors/$ARGUMENTS/progress.md` exists:
   - Read it to determine which phases and waves are already complete.
   - Resume from the first incomplete phase — skip all completed phases entirely.
   - Report to user: "Resuming from <phase>. Previously completed: <list>."

5. If no `progress.md` exists, create it:
   ```markdown
   # Progress: $ARGUMENTS

   ## Completed Steps
   ```

## Phase 1: Characterization Tests

If the architecture specifies pre-refactor characterization tests:

Spawn the **test-writer** agent:
- Tell it to read:
  - `docs/refactors/$ARGUMENTS/architecture.md`
  - `docs/refactors/$ARGUMENTS/requirements.md`
- Tell it this is a **refactor** — tests must lock in CURRENT behavior, not test new behavior
- Tell it to write characterization tests as specified in the architecture's testing strategy
- Tell it to run tests and confirm they **pass** (these test existing behavior — they must be green)
- Tell it to append decisions to `docs/refactors/$ARGUMENTS/decisions.md`

Wait for completion. Report to user: "Characterization tests written. X tests, all passing (behavior locked). Starting transformation."

Update `progress.md` — append: `- Phase 1: Characterization tests (X tests, all passing)`

If no characterization tests are specified, skip to Phase 2.

## Phase 2: Transformation

Read the task dependency graph from `architecture.md`. Execute tasks in dependency order:

### For each wave of parallelizable tasks:

Identify tasks whose dependencies are all satisfied. For each task in the wave, spawn an **implementor** agent:
- Tell it which task (ID + description) to implement
- Tell it this is a **refactor** — it must preserve behavior, not add features
- Tell it to read:
  - `docs/refactors/$ARGUMENTS/architecture.md` (focus on its task + contracts)
  - `docs/refactors/$ARGUMENTS/requirements.md`
  - Relevant test files
- Tell it to append any deviations to `docs/refactors/$ARGUMENTS/decisions.md`
- Tell it the key rule: **all existing tests must keep passing after every step**

**Spawn parallel tasks simultaneously** using multiple Agent tool calls in one message.

After each wave completes:
1. Run `npm test` to check ALL tests still pass (characterization + existing)
2. Run `npx tsc --noEmit` to verify type correctness
3. Stage all changes and create a checkpoint commit: `wip: $ARGUMENTS — wave N (T1, T2)`
4. Update `progress.md` — append: `- Phase 2 Wave N: T1, T2 (tests: pass/fail, types: pass/fail)`
5. Report to user: "Wave N complete. All tests passing: yes/no. Type-check: pass/fail."
6. If tests fail:
   - If this is a behavioral regression (characterization test broke), this is **critical** — investigate immediately
   - If this is a type error from an incomplete migration, continue to the next wave if the architecture accounts for it
   - If a task agent reported a BLOCKER:
     - Read the blocker from `decisions.md`
     - Report it to the user: "Blocker encountered: <description>. Breaking early. See decisions.md for details."
     - **Stop transformation and skip to Phase 4 (Documentation).**

Continue waves until all tasks are complete and all tests pass.

If tests still fail after all tasks are done, make ONE attempt to fix remaining failures by spawning an **implementor** agent focused on the failing tests. If they still fail, document the failures and continue to review.

## Phase 3: Review Cycle (max 3 iterations)

### Iteration loop:

**Step A** — Spawn the **reviewer** agent:
- Tell it to read:
  - `docs/refactors/$ARGUMENTS/architecture.md`
  - `docs/refactors/$ARGUMENTS/requirements.md`
- Tell it this is a **refactor review** — focus on:
  - Behavioral preservation — did any behavior change unintentionally?
  - Dead code — was all old code actually removed?
  - Incomplete migration — are there callers still using old patterns?
  - Code quality — does the new code meet the target state described in the architecture?
  - Duplication — run vibecheck scan to detect duplicated or near-identical code
- Tell it to review all files changed since the refactor started (use git diff)
- Tell it to write findings to `docs/refactors/$ARGUMENTS/review-notes.md`

**Step B** — Read `review-notes.md`. Check the summary:
- If "ready to ship" or no Critical/Important issues → exit loop, go to Phase 4
- If Critical or Important issues exist AND iteration < 3:
  - Report to user: "Review iteration N: X critical, Y important issues found. Fixing..."
  - Spawn the **fixer** agent:
    - Tell it to read `docs/refactors/$ARGUMENTS/review-notes.md`
    - Tell it to read `docs/refactors/$ARGUMENTS/architecture.md`
    - Tell it which iteration this is (1, 2, or 3)
    - Tell it to append fix details to `docs/refactors/$ARGUMENTS/decisions.md`
  - After fixer completes, stage and commit fixes: `wip: $ARGUMENTS — review fixes (iteration N)`
  - Loop back to Step A
- If iteration = 3:
  - Report: "Final review iteration. Remaining issues documented as tech debt."
  - Exit loop

After the review cycle completes, update `progress.md` — append: `- Phase 3: Review (N iterations, outcome: <ready/tech-debt>)`

## Phase 4: Documentation

Spawn the **documenter** agent:
- Tell it to read all files in `docs/refactors/$ARGUMENTS/`
- Tell it this is a **refactor** summary, not a feature summary. The output structure should be:

```markdown
# Refactor Summary: <name>

## What Changed
<1-3 paragraph description of what was refactored and why>

## Before/After
<concise comparison of the old vs new structure>

## Implementation Overview
- **Files created**: <count>
- **Files modified**: <count>
- **Files deleted**: <count>
- **Tests added**: <count>

### Key Changes
- <change>: <what it improved>

## Key Decisions
<from decisions.md — the important ones with rationale>

## Deviations from Plan
<what changed from the original architecture and why>

## Known Issues & Tech Debt
<from review-notes.md — anything deferred>

## Behavioral Verification
<how we verified behavior was preserved — test results, characterization tests>

## Developer Notes
<anything someone touching this code later should know>
```

- Tell it to write to `docs/refactors/$ARGUMENTS/summary.md`

After completion, update `progress.md` — append: `- Phase 4: Documentation`

## Phase 5: Update Refactor Index

After documentation is complete, update `docs/refactors/INDEX.md`:

1. Read `docs/refactors/$ARGUMENTS/summary.md` to identify which systems were affected.
2. Map affected systems to their memory names (e.g., turn pipeline → `turn-pipeline`, item/inventory → `item-inventory`). Use the system names from `MEMORY.md`.
3. If `docs/refactors/INDEX.md` doesn't exist, create it:
   ```markdown
   # Refactor Index

   Maps completed refactors to the systems they affect. System names match the memory index in `MEMORY.md`.

   When working on a system, check this index to find refactors that recently modified it — then read the refactor's `summary.md` and `decisions.md` for rationale.

   | Refactor | Date | Systems | Description |
   |----------|------|---------|-------------|
   ```
4. Append a new row:
   ```
   | [$ARGUMENTS]($ARGUMENTS/) | <today's date> | <comma-separated system names> | <one-line description> |
   ```

After completion, update `progress.md` — append: `- Phase 5: Index`

## Phase 6: Commit

After the refactor index is updated:

1. Stage all remaining changes (review fixes, docs, index — transformation code was already committed in checkpoint commits).
2. Create a final commit with the message:
   ```
   refactor: $ARGUMENTS — <one-line description from summary>
   ```
   Use the "What Changed" section of `summary.md` to write a concise description.
3. Update `progress.md` — append: `- Phase 6: Commit`

## Completion

After the commit:
1. Read `docs/refactors/$ARGUMENTS/summary.md`
2. Present a final summary to the user:
   - What changed
   - Behavioral verification status
   - Any known issues or tech debt
   - Any blockers encountered
3. Remind the user that implementation was done in a worktree. They can review with `git diff` and merge when ready.
