# codeuse — Semantic Code Reuse Enforcement for LLM-Assisted Development

## Overview

`codeuse` is a static analysis and retrieval tool designed to detect when LLM-generated code
duplicates functionality that already exists in a TypeScript codebase. Rather than making a binary
"duplicate or not" decision, it retrieves the most semantically similar existing functions and injects
them as context into the LLM's next pass, allowing the model to refactor its output to reuse existing
utilities instead of reinventing them.

It is built to run locally with no cloud dependencies, no recurring API costs, and no external services.

---

## The Problem

LLMs generating code have no inherent awareness of a project's existing utility layer. Given a task,
they will write new functions from scratch even when the codebase already contains an equivalent — often
with a different name, different style, or slightly different signature. Over time this produces:

- Redundant implementations of the same logic maintained in parallel
- Divergent behaviour when one copy is updated and others are not
- An expanding utility surface that becomes impossible to reason about
- Bugs that are fixed in one place but silently persist in clones

This is not a problem that linters or code review catch reliably. It requires semantic understanding of
what code *does*, not just what it *looks like*.

---

## Goals

- Detect when newly written code is semantically equivalent to something already in the codebase
- Surface candidate existing functions to the LLM with enough context to act on them
- Allow the LLM to mark false positives, which are persisted so they do not recur
- Run entirely locally on consumer hardware with no external API dependencies
- Integrate into CI pipelines or run as a manual pre-commit step

---

## What This Is Not

- A linter or style checker
- A binary duplicate detector that blocks PRs
- A replacement for code review
- A tool that makes refactoring decisions automatically

The tool retrieves and surfaces — the LLM decides.

---

## Clone Type Coverage

Code duplication research classifies clones into four types. `codeuse` addresses all four through
a layered approach:

| Type | Description | Detection Method |
|------|-------------|-----------------|
| **Type 1** | Exact copies, whitespace/comment differences only | jscpd (token) |
| **Type 2** | Same structure, renamed identifiers and literals | jsinspect (AST) |
| **Type 3** | Near-miss: statements added, deleted, or reordered | jsinspect (AST) + embeddings |
| **Type 4** | Semantically equivalent but syntactically different | Embedding similarity (primary) |

Type 4 is the dominant failure mode for LLM-generated code and the primary motivation for the
embedding layer.

---

## Stack

### Parsing
**`tree-sitter` + `tree-sitter-typescript`**
Parses TypeScript source into an Abstract Syntax Tree. Used to extract function-level chunks — the
unit of comparison throughout the system. tree-sitter is chosen for its speed, error tolerance on
partial code, and first-class TypeScript grammar support.

### Embedding
**`nomic-embed-code` 7B via Ollama (primary)**
A 7B parameter code embedding model that is the primary and preferred embedding backend.
It outperforms both Voyage Code 3 and OpenAI `text-embedding-3-large` on CodeSearchNet, making it
the state-of-the-art option for code retrieval — and it runs entirely locally under an Apache 2.0
licence with no API cost. Requires a GPU; an 8GB consumer card (e.g. RTX 3070, Apple M-series
unified memory) is sufficient.

**`nomic-embed-code` 137M via Ollama (fallback)**
The 137M parameter variant serves as an automatic fallback when no GPU is detected or when the 7B
model is not available in the local Ollama instance. It runs on CPU with acceptable latency
(~500ms/function) and is meaningfully better than general-purpose text embedding models for code
tasks. Expect roughly 10–20% lower recall on ambiguous Type 4 cases compared to the 7B model —
acceptable given the forgiving RAG-style candidate retrieval design.

The tool detects which model to use at startup by querying the local Ollama instance for available
models and checking for GPU availability. The active model is recorded in the index metadata so that
a mismatch between the model used to build the index and the model currently available triggers a
warning and prompts a re-index. Vectors produced by the two model sizes are not compatible and
cannot be mixed in the same index.

Both variants share the same rationale for selection:
- Specifically trained on code (not general text)
- No API key, no internet dependency, no per-token cost
- Fully open source (Apache 2.0)
- Ollama provides a simple local HTTP interface with a stable REST API

### Vector Storage
**SQLite + `sqlite-vec`**
Stores function embeddings and metadata in a single `.db` file. Chosen because:
- Zero infrastructure — no server, no daemon, no Docker
- Single file is portable, committable to `.gitignore`, and regeneratable on demand
- `sqlite-vec` provides approximate nearest-neighbour search directly in SQLite
- Sufficient performance for codebases up to tens of thousands of functions

### Pre-filtering (fast path)
**`jscpd`** (token-based, Types 1–2) and **`jsinspect`** (AST-based, Types 2–3) run as a fast
pre-filter before the embedding pipeline. This avoids embedding calls for obvious duplicates and
keeps the embedding layer focused on Type 3–4 cases where it provides unique value.

### False Positive Persistence
**`.codereuse-ignore.json`** — a human-readable JSON file committed to the repository. Stores
pair-level exclusions (a specific generated function + a specific existing function are not
duplicates) with a reason, a timestamp, and a signature hash for both sides. The LLM emits
exclusions as structured output; the tool appends them to this file automatically.

---

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                    INDEXING (one-time + incremental) │
│                                                      │
│  TypeScript source files                             │
│          ↓                                           │
│  tree-sitter → function-level chunks                 │
│          ↓                                           │
│  nomic-embed-code 7B (GPU) or 137M (CPU fallback)    │
│          ↓                                           │
│  sqlite-vec → .codeuse.db (vectors + metadata)       │
└─────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────┐
│                    QUERY (per PR / on demand)        │
│                                                      │
│  LLM-generated TypeScript code                       │
│          ↓                                           │
│  jscpd + jsinspect (fast pre-filter)                 │
│          ↓                                           │
│  tree-sitter → chunk new functions                   │
│          ↓                                           │
│  nomic-embed-code → embed new functions              │
│          ↓                                           │
│  sqlite-vec → top-N candidates by cosine similarity  │
│          ↓                                           │
│  filter against .codereuse-ignore.json               │
│          ↓                                           │
│  return: { function, path, line, similarity, source }│
└─────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────┐
│                    LLM FEEDBACK LOOP                 │
│                                                      │
│  Candidates injected into LLM context                │
│          ↓                                           │
│  LLM rewrites output using existing utilities        │
│  LLM emits structured output:                        │
│    - rewritten_code                                  │
│    - used_existing[]  (what it reused)               │
│    - false_positives[] (with reasons)                │
│          ↓                                           │
│  false_positives → appended to .codereuse-ignore.json│
└─────────────────────────────────────────────────────┘
```

---

## The False Positive System

### Why It Exists

Embedding similarity is a heuristic. Two functions can score high cosine similarity while serving
genuinely different purposes — different contracts, different domains, different invariants. Without
a mechanism to record these distinctions, the same irrelevant candidates will clutter the LLM's
context on every invocation, degrading the quality of its output over time.

### How It Works

The LLM emits false positive rejections as part of its structured output. Each rejection includes:
- The path and name of the existing function it is rejecting
- A human-readable reason explaining why the two are not equivalent

The tool computes a signature hash (function name + parameter types + return type) for both the
generated function and the rejected existing function, then appends the pair to
`.codereuse-ignore.json`.

On all subsequent queries, candidate pairs are checked against this file before being returned.
Matching pairs are silently dropped from results.

### Ignore File Format

```json
{
  "version": 1,
  "exclusions": [
    {
      "reason": "formatDateForDisplay targets UI locale formatting; serializeDateForStorage targets ISO 8601 DB writes — different contracts",
      "added": "2024-03-15",
      "pair": {
        "a": {
          "path": "src/utils/date.ts",
          "function": "formatDateForDisplay",
          "signatureHash": "a3f8c2d1"
        },
        "b": {
          "path": "src/db/serializers.ts",
          "function": "serializeDateForStorage",
          "signatureHash": "b7e4a9f2"
        }
      }
    }
  ]
}
```

### Stale Exclusion Detection

When a function referenced in an exclusion has a signature hash that no longer matches the current
codebase (the function has been renamed, its parameters changed, or it was deleted), the tool emits
a warning at query time. This surfaces to the LLM — or a human reviewer — that a previously rejected
candidate has evolved and the exclusion should be reconsidered.

### Scope

Exclusions are **pair-level**, not global. Suppressing a candidate for one generated function does
not suppress it for all future queries. A function that is genuinely irrelevant to `serializeDate`
may be highly relevant to the next generated function.

---

## LLM Output Contract

The LLM must respond with structured JSON when this tool is active:

```json
{
  "rewritten_code": "<full rewritten TypeScript>",
  "used_existing": [
    {
      "path": "src/utils/array.ts",
      "function": "groupBy",
      "how": "replaced inline reduce with existing groupBy utility"
    }
  ],
  "false_positives": [
    {
      "path": "src/utils/date.ts",
      "function": "formatDateForDisplay",
      "reason": "My function writes ISO 8601 for API payloads; this one formats for UI display — different output contracts"
    }
  ]
}
```

---

## Incremental Indexing

The full codebase index is built once. Subsequent runs only re-index files that have changed since
the last run, determined by comparing file modification times and content hashes against the stored
index metadata. Functions that are deleted are removed from the index. Functions that are added or
modified are re-embedded and upserted.

This keeps per-PR indexing overhead negligible — typically under a second for a normal-sized commit.

---

## Performance Characteristics

All figures are approximate and hardware-dependent.

### 7B model (GPU — primary)

| Operation | Small (~500 fns) | Medium (~3,000 fns) | Large (~20,000 fns) |
|-----------|-----------------|---------------------|---------------------|
| Full index | ~5 sec | ~30 sec | ~3 min |
| Per-PR query (15 new fns) | <1 sec | <1 sec | <1 sec |
| Index storage | ~6 MB | ~35 MB | ~230 MB |

### 137M model (CPU — fallback)

| Operation | Small (~500 fns) | Medium (~3,000 fns) | Large (~20,000 fns) |
|-----------|-----------------|---------------------|---------------------|
| Full index | ~1 min | ~5 min | ~30 min |
| Per-PR query (15 new fns) | ~8 sec | ~8 sec | ~8 sec |
| Index storage | ~3 MB | ~18 MB | ~120 MB |

The 7B model produces higher-dimensional vectors, hence the larger index storage. The full index is
built once and cached. Query time is independent of codebase size — it depends only on the number of
functions being checked in the current PR. The two model variants produce incompatible vector spaces;
switching models requires a full re-index.

---

## Cost

Running entirely locally with either `nomic-embed-code` variant via Ollama, the marginal cost per
PR is **zero**. There are no API calls, no tokens billed, no external services. The 7B model requires
a GPU but delivers state-of-the-art code retrieval quality. The 137M fallback runs on any hardware
at no additional cost beyond compute time.

If a cloud embedding model (OpenAI, Voyage) is substituted, the cost at typical PR sizes is under
**$0.005 per PR** — negligible at any team size.

---

## Integration Points

- **CI / GitHub Actions**: Run on every PR as a non-blocking annotation step
- **Pre-commit hook**: Run locally before a commit is finalised
- **Agent loop**: Integrate directly into an LLM coding agent as a retrieval step before code generation
- **CLI**: Manual invocation against any TypeScript file or diff

---

## Non-Goals and Limitations

- **Type 4 recall is not perfect.** Embedding similarity is a heuristic. Expect to miss
  approximately 35–50% of genuine semantic duplicates, particularly for short functions (<5 lines),
  domain-specific logic, or highly compositional code. This is acceptable because the cost of a
  miss is low — the LLM simply does not receive that candidate, and writes its own version as it
  would have without the tool.

- **This does not enforce refactoring.** The tool retrieves candidates and provides them as context.
  Whether the LLM actually reuses an existing function is a product of prompt design and model
  capability, not this tool.

- **Index freshness is manual.** The incremental indexer must be run to pick up new functions added
  to the codebase. In CI this happens automatically; in local use it requires a `codeuse index`
  invocation.

- **TypeScript only.** The tool is designed and tested for TypeScript. JavaScript support is likely
  with minimal changes; other languages are out of scope.
