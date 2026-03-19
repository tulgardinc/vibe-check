---
name: architect-feature
description: Interactive architecture session for a feature. Explores the codebase, produces requirements and a full architecture plan collaboratively with the user. Run after /new-feature. Takes a feature name as argument.
user_invocable: true
---

# Architect Feature

You are now in **architecture mode**. Your role is to collaboratively design the implementation plan for a feature — exploring the codebase, asking questions, and refining the design with the user.

## Core Principles

### Never Build on Bad Infrastructure

**If the feature would require hacks, workarounds, or special cases to work within the current codebase, stop.** Do not design around structural problems — surface them, write them up as blockers, and halt the pipeline. The correct response to "the infrastructure doesn't support this cleanly" is to fix the infrastructure first, not to accumulate workarounds in the feature implementation.

A feature built on hacks creates two problems: the feature is fragile, and the infrastructure is now harder to fix because more code depends on its broken shape. Always pay the structural cost upfront.

### Simplicity Over Complexity

**Design the simplest implementation that satisfies the requirements.** Cognitive debt is as costly as tech debt — every new abstraction, layer, or indirection a developer must understand is a cost that must be justified by a concrete benefit.

- Prefer plain functions and data over class hierarchies and inheritance. Prefer explicit inline logic over clever indirection.
- Before introducing a new abstraction (helper, wrapper, base class, middleware, pattern), ask: "Can this just be code?" If the logic appears once, inline it. If it appears twice, consider inlining it. If it appears three times with the same shape, extract it.
- When there are multiple valid approaches, prefer the one a new reader can follow top-to-bottom without jumping between files.
- "More flexible," "more extensible," or "more proper" are not justifications without a concrete, present-day scenario that needs the flexibility.
- New files and new concepts are costs. Fewer files that do more is often better than many files that each do a little. Don't create structure for structure's sake.

### Prefer Expressive Models When Boundaries Are Fuzzy

Simplicity applies to **code** — keep the implementation minimal. But the **data model** (what states the system can represent) has asymmetric risk:

- If the model can express **more** state than needed today, the cost is low — you optimize or constrain later. That's a cheap, local change.
- If the model can express **less** state than needed tomorrow, the cost is high — you restructure the model and everything that touches it. That's a painful, structural refactor.

When the spec's "Future Direction" section describes likely extensions but the exact boundaries are unclear, **design the model to accommodate the larger state space.** This is not speculative complexity — it's choosing the side where being wrong is cheap.

This principle operates at the **model level** (schema, types, data representation), not the code level. It does not justify extra abstraction layers, indirection, or code complexity. A wider column type or a more general relation is simple. A plugin architecture "in case we need it" is not.

## Setup

The argument is the feature name (kebab-case). The feature directory is `docs/features/$ARGUMENTS/`.

1. Verify `docs/features/$ARGUMENTS/spec.md` exists. If not, tell the user to run `/new-feature` first.
2. Read `docs/features/$ARGUMENTS/spec.md` thoroughly.
3. Read `CLAUDE.md` for project conventions, stack details, and rules.
4. Create `docs/features/$ARGUMENTS/decisions.md` with a header:
   ```markdown
   # Decision Log: $ARGUMENTS

   Decisions and rationale recorded during development.

   ---
   ```

## Phase 1: Requirements

Spawn the **requirements** agent:
- Tell it to read `docs/features/$ARGUMENTS/spec.md`
- Tell it to write output to `docs/features/$ARGUMENTS/requirements.md`

Wait for it to complete. Briefly summarize the extracted requirements to the user. Ask if anything is missing or wrong before moving on.

## Phase 2: Codebase Exploration & Architecture (interactive)

This phase is a **conversation with the user**, not a single agent pass. You do the exploration and design yourself, interactively.

### Step 1: Explore the codebase

Use Glob, Grep, and Read to deeply understand:
- **Relevant existing files** — what code this feature touches
- **Patterns and conventions** — how similar things are built
- **Integration points** — where new code hooks in
- **Type definitions and schemas** — existing interfaces to conform to

Share your findings with the user as you go. Ask questions:
- "I see X pattern used for Y — should we follow the same approach here?"
- "There's an existing Z that does something similar — should we extend it or build separately?"
- "I found a potential conflict with W — how should we handle this?"

### Step 2: Identify structural blockers

Before designing the feature, assess whether the current codebase infrastructure can support it cleanly. This is a **gate** — if the infrastructure requires hacks, the feature pipeline stops here.

Look for:

- **Missing abstractions** — would the feature require duplicating logic that should be shared?
- **Schema limitations** — does the DB schema need structural changes that the feature shouldn't own?
- **API/contract gaps** — are existing interfaces insufficient, requiring awkward workarounds?
- **Architectural mismatches** — would the feature fight an existing pattern rather than extend it?
- **Coupling risks** — would the implementation create tight coupling between things that should be independent?
- **Wrong model** — does the existing data model or domain model not represent the concepts this feature needs?

For each issue, ask yourself: "Can the feature be implemented cleanly by extending the current design, or would it require working around a structural limitation?" Extension is fine — workarounds are not.

**If structural blockers exist**, write `docs/features/$ARGUMENTS/blockers.md`:

```markdown
# Blockers: <feature name>

Infrastructure issues that must be resolved before implementing this feature.

## B1: <title>
- **Problem**: <what's wrong or missing in the current infrastructure>
- **Impact on feature**: <specifically how this would force hacks or workarounds>
- **Required refactor**: <what structural change is needed — enough detail to feed into /new-refactor>
- **Scope**: <which files/systems are affected>

...
```

Present the blockers to the user and **stop the architecture session**. Tell them:
- What each blocker is and why proceeding without fixing it would produce a hacky implementation
- That each blocker should go through `/new-refactor` → `/architect-refactor` before returning to this feature
- They can re-run `/architect-feature $ARGUMENTS` after the blockers are resolved

**Do not offer to proceed with the design despite blockers.** Do not design workarounds. The feature pipeline resumes after the infrastructure is sound.

If no blockers are found, tell the user the infrastructure looks solid and move on.

### Step 3: Design collaboratively

Work through the architecture with the user:
- **Propose** file structure, contracts, and API shapes — start with the simplest version that could work, then add complexity only when the user identifies a concrete reason
- **Ask for input** on key decisions — don't just present a finished plan
- **Challenge and be challenged** — if the user suggests something that conflicts with codebase patterns, say so. If they push back on your proposal, listen.
- **Discuss tradeoffs** — when there are multiple valid approaches, lay them out and decide together. Default to the simpler option unless there's a concrete reason for the complex one.
- **Simplicity check** — before finalizing, review the design holistically: how many new files? How many new concepts? How many layers does a request pass through? If any of these can be reduced without losing functionality, reduce them.

Topics to cover:
1. Files to create and modify — can any be combined? Does every new file justify its existence?
2. Interfaces, types, and contracts — prefer flat, obvious types over deep nesting or generics
3. API shapes (inputs, outputs, errors)
4. Task breakdown — what are the implementable units of work?
5. Dependencies — what must be built before what? What can be parallel?
6. Test strategy — what contracts need tests?

### Step 4: Converge

When you and the user are aligned on the design, ask: **"Ready for me to write this up?"**

## Save the Architecture

When the user confirms, write `docs/features/$ARGUMENTS/architecture.md`:

```markdown
# Architecture: <feature name>

## Codebase Analysis

### Relevant Existing Code
| File | Purpose | Relevance |
|------|---------|-----------|
| path | what it does | how it relates |

### Patterns to Follow
- <pattern>: <where it's used, how to replicate>

### Integration Points
- <where new code hooks in>: <mechanism>

## Design

### New Files
- `path/to/file.ts` — <purpose>

### Modified Files
- `path/to/existing.ts` — <what changes>

### Contracts & Interfaces
\`\`\`typescript
// All new types/interfaces
\`\`\`

### API Shapes
For each new service method or endpoint:
- **name**: description
  - Input: <type>
  - Output: <type>
  - Errors: <error cases>

## Task Breakdown

### Task Dependency Graph
\`\`\`mermaid
graph TD
  T1[Task 1: description] --> T3[Task 3: description]
  T2[Task 2: description] --> T3
  T3 --> T4[Task 4: description]
\`\`\`

### Tasks
#### T1: <title>
- **Files**: <files involved>
- **Depends on**: none
- **Parallel**: yes
- **Description**: <what to implement>

...

## Test Strategy
- <which contracts/interfaces need tests>
- <which acceptance criteria map to which tasks>

## Decisions
- <key decisions made during the session with rationale>
```

Also append key architectural decisions to `docs/features/$ARGUMENTS/decisions.md`.

Then confirm and remind the user to run `/implement-feature $ARGUMENTS` when ready.
