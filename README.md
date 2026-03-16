# codeuse

Semantic code deduplication for TypeScript. Catches when new code reimplements something that already exists in the codebase — including logic buried inline inside other functions.

Designed for LLM-assisted development, where models routinely rewrite utilities instead of reusing them. Works as a CLI tool or an MCP server that gives your AI assistant visibility into what's already been written.

```
$ codeuse query src/utils/process.ts

processItems (src/utils/process.ts:5)
  handleItems (src/lib/items.ts:12) — similarity: 0.91 [embedding, jaccard: 0.62]
  <block:88> in buildSummary (src/components/Report.tsx:88) — similarity: 0.84 [embedding, jaccard: 0.41]
```

## How it works

```
Index:  parse (functions + blocks) → embed → store
Query:  parse → embed → KNN overfetch → Jaccard re-rank → exclusions → top-K
```

1. **Parse** — tree-sitter extracts functions and eligible logic blocks (6+ lines with control flow) from TypeScript
2. **Embed** — local Ollama generates vector embeddings for each chunk
3. **Store** — SQLite + sqlite-vec for fast KNN cosine search
4. **Rank** — combines embedding distance with Jaccard token overlap for accurate re-ranking

Everything runs locally. No cloud dependencies.

## Prerequisites

- Node.js >= 20
- [Ollama](https://ollama.com) running locally
- An embedding model (auto-detected; prefers `nomic-embed-code`)

```bash
ollama pull nomic-embed-code
```

## Install

```bash
git clone https://github.com/tulgardinc/vibe-check.git
cd vibe-check
npm install
npm run build
npm link
```

## CLI usage

```bash
# Build the index
codeuse index

# Check a file for duplicates
codeuse query src/utils/helpers.ts

# Scan the whole codebase for similar pairs
codeuse scan

# Check index health
codeuse status
```

### Options

```bash
codeuse index [path]       --force        # Full re-index
codeuse query <file>       --top-k 10     # More candidates
                           --threshold 0.2 # Stricter matching
                           --json          # JSON output
codeuse scan               --top-n 100    # More pairs
                           --threshold 0.2 # Stricter
```

## MCP server

Add to your MCP client config (e.g. Claude Desktop, Claude Code):

```json
{
  "mcpServers": {
    "codeuse": {
      "command": "codeuse-mcp"
    }
  }
}
```

Exposes five tools:

| Tool | Purpose |
|------|---------|
| `codeuse_query` | Find similar functions/blocks for given code |
| `codeuse_index` | Build or update the semantic index |
| `codeuse_scan` | Find all similar pairs across the codebase |
| `codeuse_status` | Check index health |
| `codeuse_add_exclusion` | Suppress a false positive match |

## False positive management

When a match isn't a real duplicate, exclude it:

```bash
# Via MCP: use codeuse_add_exclusion with the signatureHash values from results

# Exclusions are stored in .codereuse-ignore.json at the project root
```

## License

MIT
