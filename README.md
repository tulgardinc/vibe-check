# vibecheck

Semantic code deduplication. Catches when new code reimplements something that already exists in the codebase — including logic buried inline inside other functions.

Designed for LLM-assisted development, where models routinely rewrite utilities instead of reusing them. Works as a CLI tool or an MCP server that gives your AI assistant visibility into what's already been written.

```
$ vibec query src/utils/process.ts

processItems (src/utils/process.ts:5)
  handleItems (src/lib/items.ts:12) — similarity: 0.91 [embedding, jaccard: 0.62]
  <block:88> in buildSummary (src/components/Report.tsx:88) — similarity: 0.84 [embedding, jaccard: 0.41]
```

## How it works

```
Index:  parse (functions + blocks) → embed → store
Query:  parse → embed → KNN overfetch → Jaccard re-rank → exclusions → top-K
```

1. **Parse** — tree-sitter extracts functions and eligible logic blocks (6+ lines with control flow)
2. **Embed** — local Ollama generates vector embeddings for each chunk
3. **Store** — SQLite + sqlite-vec for fast KNN cosine search
4. **Rank** — combines embedding distance with Jaccard token overlap for accurate re-ranking

Everything runs locally. No cloud dependencies. Single binary, no runtime needed.

## Prerequisites

- [Ollama](https://ollama.com) running locally
- An embedding model (auto-detected; prefers `nomic-embed-code`)

```bash
ollama pull nomic-embed-code
```

## Install

Download the latest binary for your platform:

**macOS (Apple Silicon)**
```bash
curl -L https://github.com/tulgardinc/vibe-check/releases/latest/download/vibec-darwin-aarch64 -o /usr/local/bin/vibec && chmod +x /usr/local/bin/vibec
curl -L https://github.com/tulgardinc/vibe-check/releases/latest/download/vibecheck-mcp-darwin-aarch64 -o /usr/local/bin/vibecheck-mcp && chmod +x /usr/local/bin/vibecheck-mcp
```

**macOS (Intel)**
```bash
curl -L https://github.com/tulgardinc/vibe-check/releases/latest/download/vibec-darwin-x86_64 -o /usr/local/bin/vibec && chmod +x /usr/local/bin/vibec
curl -L https://github.com/tulgardinc/vibe-check/releases/latest/download/vibecheck-mcp-darwin-x86_64 -o /usr/local/bin/vibecheck-mcp && chmod +x /usr/local/bin/vibecheck-mcp
```

**Linux (x86_64)**
```bash
curl -L https://github.com/tulgardinc/vibe-check/releases/latest/download/vibec-linux-x86_64 -o ~/.local/bin/vibec && chmod +x ~/.local/bin/vibec
curl -L https://github.com/tulgardinc/vibe-check/releases/latest/download/vibecheck-mcp-linux-x86_64 -o ~/.local/bin/vibecheck-mcp && chmod +x ~/.local/bin/vibecheck-mcp
```

**Linux (aarch64)**
```bash
curl -L https://github.com/tulgardinc/vibe-check/releases/latest/download/vibec-linux-aarch64 -o ~/.local/bin/vibec && chmod +x ~/.local/bin/vibec
curl -L https://github.com/tulgardinc/vibe-check/releases/latest/download/vibecheck-mcp-linux-aarch64 -o ~/.local/bin/vibecheck-mcp && chmod +x ~/.local/bin/vibecheck-mcp
```

**Windows (x86_64)**
```powershell
Invoke-WebRequest -Uri https://github.com/tulgardinc/vibe-check/releases/latest/download/vibec-windows-x86_64.exe -OutFile "$env:USERPROFILE\.local\bin\vibec.exe"
Invoke-WebRequest -Uri https://github.com/tulgardinc/vibe-check/releases/latest/download/vibecheck-mcp-windows-x86_64.exe -OutFile "$env:USERPROFILE\.local\bin\vibecheck-mcp.exe"
```

### Build from source

```bash
git clone https://github.com/tulgardinc/vibe-check.git
cd vibe-check
cargo build --release
```

Binaries are in `target/release/`:
- `vibec` — CLI
- `vibecheck-mcp` — MCP server

## CLI usage

```bash
# Build the index
vibec index

# Check a file for duplicates
vibec query src/utils/helpers.ts

# Scan the whole codebase for similar pairs
vibec scan

# Check index health
vibec status
```

### Options

```bash
vibec index [path]       --force        # Full re-index
vibec query <file>       --top-k 10     # More candidates
                         --threshold 0.2 # Stricter matching
                         --json          # JSON output
vibec scan               --top-n 100    # More pairs
                         --threshold 0.2 # Stricter
```

## MCP server

Add to your MCP client config (e.g. Claude Desktop, Claude Code):

```json
{
  "mcpServers": {
    "vibecheck": {
      "command": "/path/to/vibecheck-mcp"
    }
  }
}
```

Exposes five tools:

| Tool | Purpose |
|------|---------|
| `vibecheck_query` | Find similar functions/blocks for given code |
| `vibecheck_index` | Build or update the semantic index |
| `vibecheck_scan` | Find all similar pairs across the codebase |
| `vibecheck_status` | Check index health |
| `vibecheck_add_exclusion` | Suppress a false positive match |

## Supported languages

- TypeScript

## Limitations

- **Functions only** — top-level code outside of functions (module-level logic, script-style files) is not indexed. Only named functions, methods, and eligible inline blocks within them are captured.

## False positive management

When a match isn't a real duplicate, exclude it:

```bash
# Via MCP: use vibecheck_add_exclusion with the signatureHash values from results

# Exclusions are stored in .vibecheck-ignore.json at the project root
```

## License

MIT
