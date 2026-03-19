---
name: architect-refactor
description: Interactive architecture session for a refactor. Explores the codebase, maps dependencies, designs the migration path, and produces a step-by-step plan. Run after /new-refactor. Takes a refactor name as argument.
user_invocable: true
---

# Architect Refactor

You are now in **refactor architecture mode**. Your role is to collaboratively design the implementation plan for a refactor — mapping dependencies, identifying safe transformation steps, and ensuring behavioral preservation.

## Core Principles

### Structural Changes Over Workarounds

**Always design the plan around fixing the structural root cause, even when it means touching the entire codebase.** Never compromise the plan by scoping down to avoid a large blast radius — if the structural problem spans 50 files, the plan must span 50 files. The transformation sequence should be designed to make that large change safe and incremental, not to avoid it. A refactor that leaves the structural problem in place is a failed refactor regardless of how many cosmetic improvements it makes.

### Simplicity Over Complexity

**The target state must be simpler than the current state.** A refactor that replaces one complex system with a differently-complex system has failed. At every design decision — new interfaces, module boundaries, patterns — ask: **"Can we solve this with a simpler system?"**

- Plain functions and data beat class hierarchies and inheritance. Explicit inline logic beats clever indirection.
- Removing code, layers, and concepts is the highest-value refactor. If the system can work with fewer moving parts, that is the target.
- Every abstraction in the target design must justify itself with concrete, present-day callers and a clear simplification. "More flexible" or "more proper" without a specific scenario is not justification.
- Cognitive debt is as costly as tech debt. If a developer needs to hold 5 concepts in their head to understand a flow, and the refactored version needs 3, that's a measurable win — capture it in the plan.
- When choosing between two correct approaches, prefer the one that a new reader can understand by reading top-to-bottom without jumping between files.

## Setup

The argument is the refactor name (kebab-case). The refactor directory is `docs/refactors/$ARGUMENTS/`.

1. Verify `docs/refactors/$ARGUMENTS/brief.md` exists. If not, tell the user to run `/new-refactor` first.
2. Read `docs/refactors/$ARGUMENTS/brief.md` thoroughly.
3. Read `CLAUDE.md` for project conventions, stack details, and rules.
4. Create `docs/refactors/$ARGUMENTS/decisions.md` with a header:
   ```markdown
   # Decision Log: $ARGUMENTS

   Decisions and rationale recorded during refactoring.

   ---
   ```

## Phase 1: Requirements

Spawn the **requirements** agent:
- Tell it to read `docs/refactors/$ARGUMENTS/brief.md` — explain that this is a refactor brief, not a feature spec. It should extract requirements focused on:
  - Behavioral preservation (what must NOT change)
  - Structural improvements (what must change)
  - Constraints (what patterns to follow, what to avoid)
  - Acceptance criteria framed as before/after comparisons
- Tell it to write output to `docs/refactors/$ARGUMENTS/requirements.md`

Wait for it to complete. Briefly summarize the extracted requirements to the user. Ask if anything is missing or wrong before moving on.

## Phase 2: Codebase Exploration & Architecture (interactive)

This phase is a **conversation with the user**, not a single agent pass. You do the exploration and design yourself, interactively.

### Step 1: Map the blast radius

Use Glob, Grep, and Read to comprehensively understand:
- **All callers and consumers** — who calls the code being refactored? Grep for function names, type references, imports
- **All tests** — what test coverage exists? What's tested, what's not?
- **Type dependencies** — what types flow in and out? Who depends on the current interfaces?
- **Side effects** — what mutations, DB writes, events, or external calls happen in the affected code?

Present findings to the user:
- "X has Y callers across Z files — here's the full list"
- "Test coverage: A is well-tested, B has no tests, C only has integration coverage"
- "These types are exported and used by: ..."

### Step 2: Identify the safe transformation sequence

Refactors fail when they try to change everything at once. Design a sequence where:

- **Each step is independently shippable** — the code compiles and tests pass after every step
- **Behavioral changes are impossible to sneak in** — structural changes only, behavior preserved
- **Rollback is trivial** — if a step goes wrong, reverting it doesn't cascade
- **Dependencies are respected** — types and contracts change before implementations
- **The structural fix comes first** — the core model/abstraction change lands early in the sequence; callers migrate to it in subsequent steps. Never defer the structural change to "phase 2" — it is the point of the refactor.

Common refactor patterns to consider:
- **Strangler fig** — new code alongside old, callers migrate incrementally, old code deleted last
- **Extract and inline** — pull logic into a new function/module, then inline the old wrapper
- **Expand and contract** — add the new API, migrate callers, remove the old API
- **Parallel implementation** — build the new version, verify equivalence, swap

When the blast radius is large, use these patterns to make the change safe and incremental — not to avoid the change. A 50-file migration done in 5 safe steps is correct. A 5-file patch that leaves 45 files on the old pattern is not.

Ask the user:
- "I'd suggest this sequence: [steps]. Does this ordering make sense?"
- "Step N has risk because [reason] — want to split it further?"
- "There's a dependency between X and Y that means we can't parallelize here"

### Step 3: Design contracts and interfaces

For the target state:
- **Define the new interfaces** — what do the new types/APIs look like? Prefer the simplest possible surface: plain types over class hierarchies, flat parameters over deep config objects, direct calls over layers of indirection.
- **Compare with current** — show the before/after side by side. The "after" should have fewer concepts, fewer layers, or fewer places a reader needs to look to understand the flow. If it doesn't, challenge whether the design is actually an improvement.
- **Identify migration work** — what callers need to change and how?
- **Assess backwards compatibility** — per CLAUDE.md rules, no backwards compat needed. Plan for clean cuts.
- **Rename what no longer fits** — if the refactor changes what something does or represents, the old name is now a lie. Rename functions, types, files, and variables whose names no longer describe their post-refactor meaning. Misleading names are bugs. This is in scope for the refactor, not a follow-up.
- **Simplicity check** — for every new abstraction, pattern, or layer in the design, ask: "What concrete problem does this solve that a simpler approach doesn't?" If the answer is hypothetical, remove it.

### Step 4: Assess risk and testing gaps

Before finalizing:
- **Missing tests** — what tests need to be added BEFORE refactoring to lock in current behavior?
- **Behavioral equivalence** — how do we verify the refactored code does the same thing?
- **Edge cases** — what unusual code paths exist that might break?
- **Performance** — could the new structure change performance characteristics?

### Step 5: Converge

When you and the user are aligned on the plan, ask: **"Ready for me to write this up?"**

## Save the Architecture

When the user confirms, write `docs/refactors/$ARGUMENTS/architecture.md`:

```markdown
# Architecture: <refactor name>

## Blast Radius Analysis

### Affected Files
| File | Role | Callers | Test Coverage |
|------|------|---------|---------------|
| path | what it does | who uses it | tested/untested |

### Type Dependencies
- <type>: used by <list of files/functions>

### Side Effects in Scope
- <mutation/write/event that must be preserved>

## Before/After Comparison

### Current Structure
<description or diagram of how it works now>

### Target Structure
<description or diagram of how it will work after>

### Contract Changes
\`\`\`typescript
// Before
<old interfaces>

// After
<new interfaces>
\`\`\`

## Transformation Plan

### Task Dependency Graph
\`\`\`mermaid
graph TD
  T1[Task 1: description] --> T3[Task 3: description]
  T2[Task 2: description] --> T3
  T3 --> T4[Task 4: description]
\`\`\`

### Tasks
#### T1: <title>
- **Type**: <characterization-test | extract | migrate | delete | restructure>
- **Files**: <files involved>
- **Depends on**: none
- **Parallel**: yes/no
- **Description**: <what to do>
- **Verification**: <how to confirm this step is correct>

...

## Testing Strategy

### Pre-refactor (characterization tests)
- <tests to add before changing anything, to lock in current behavior>

### Per-step verification
- <how each step is verified — existing tests pass, new tests pass, type-check passes>

### Post-refactor
- <any new tests for the target state that don't exist yet>

## Risk Mitigation
- <risk>: <mitigation strategy>

## Decisions
- <key decisions made during the session with rationale>
```

Also append key architectural decisions to `docs/refactors/$ARGUMENTS/decisions.md`.

Then confirm and remind the user to run `/implement-refactor $ARGUMENTS` when ready.
