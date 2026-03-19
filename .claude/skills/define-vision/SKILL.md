---
name: define-vision
description: Interactive session to define or refine the project's product vision. Produces docs/VISION.md through structured conversation — covers purpose, design pillars, audience, constraints, and non-goals.
user_invocable: true
---

# Product Vision Session

You are helping the user articulate **what this project is, why it exists, and what principles guide every design decision**. The output is `docs/VISION.md` — a concise document that orients all future feature work.

## Before you begin

1. Check if `docs/VISION.md` already exists. If it does, read it — this is a refinement session, not a blank slate. Reference what's already written and focus on what needs updating.
2. Read `CLAUDE.md` for current technical context.
3. Read `docs/features/INDEX.md` (if it exists) to understand what's been built so far — the features themselves reveal what the project values.

## Your approach

1. **Listen first** — let the user describe their vision in their own words
2. **Probe for depth** — ask follow-up questions that force specificity
3. **Challenge assumptions** — push back on anything vague or contradictory
4. **Synthesize** — reflect back what you're hearing so the user can confirm or correct
5. **Converge** — work through each section until it's crisp

## Conversation structure

Work through these areas one at a time. Don't dump all questions at once — have a real conversation about each before moving on.

### 1. Purpose
- What is this project? (Beyond the technical description — what experience are you creating?)
- Why does this need to exist? What gap does it fill?
- If someone played this for 30 minutes, what would they walk away feeling?

### 2. Design Pillars
- What are the 3-5 principles that should guide every tradeoff?
- When two good ideas conflict, which one wins and why?
- What does this project value MORE than other projects in the same space?
- For each pillar: give a concrete example of a decision it would guide

### 3. Target Audience
- Who is this for? Be specific — not "gamers" but what kind?
- What experience level do they have with narrative games? With text interfaces?
- What are they looking for that they can't get elsewhere?
- What would make them stop playing?

### 4. Constraints
- What deliberate boundaries exist? (e.g., single-player, turn-based, LLM-driven)
- Why these constraints specifically? What do they enable?
- Which constraints are fundamental to the vision vs. practical/temporary?

### 5. Non-Goals
- What is this project explicitly NOT trying to be?
- What features would be impressive but wrong for this project?
- Where does scope end?

## Zero tolerance for vagueness

**Do not accept vague, implicit, or fuzzy answers.** This is the most important rule.

- If the user says "it should feel immersive" — ask: "What specifically creates that immersion? What breaks it?"
- If the user says "NPCs should feel alive" — ask: "What does 'alive' mean concretely? What behaviors signal aliveness vs. scripted responses?"
- If the user says "it's for people who like story games" — ask: "Which story games? What specifically about those games? A Telltale player and a Dwarf Fortress player are very different audiences."
- If a pillar is generic like "player agency" — ask: "Agency over what? Where does player control end and the world's logic begin?"
- If two pillars could contradict each other — surface the tension: "If emergent behavior produces something that breaks narrative coherence, which pillar wins?"

**Keep probing until every statement is concrete enough that two people reading it would make the same design decision.** If a pillar could apply to any game, it's not specific enough.

## When the user is satisfied

When the user says they're happy (or says "save", "done", "looks good", etc.):

Write `docs/VISION.md` with this structure:

```markdown
# Product Vision

## What This Is
<1-2 paragraphs: the elevator pitch — what experience this creates and why it matters>

## Design Pillars
### <Pillar Name>
<What it means, concretely. Include an example of a decision this pillar would guide.>

### <Pillar Name>
...

## Target Audience
<Who this is for, what they're looking for, what experience they bring>

## Constraints
- <Constraint>: <why it exists and what it enables>

## Non-Goals
- <What this is NOT, and why that boundary exists>
```

After saving, confirm and remind the user that `/new-feature` will now use this document to evaluate feature ideas against the vision.
