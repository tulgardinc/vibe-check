<p align="center">
  <img src="VibeCheckIcon.svg" alt="vibecheck" width="192" />
</p>

# Vibe Check

Semantic code deduplication. LLMs often end up rewriting utilities instead of reusing them.
Vibe check works as a CLI tool or an MCP server to alert your agent that there is duplicated code that can either be removed or extracted.

Pay off your tech debt early.

```
$ vibec scan
info  Scanning 552 embedded functions...

── IDENTICAL (3) ─────────────────────────────────────────
  <block:309> in computeGraphDistances (src/server/ai/location-graph.ts:309, 7L)
  <block:388> in computeShortestPath (src/server/ai/location-graph.ts:388, 8L)
  99% similar (distance: 0.0040, jaccard: 1.00)

...
```

## How it works

```
Index:  parse (functions + blocks) → embed → store
Query:  parse → embed → KNN overfetch → Jaccard re-rank → exclusions → top-K
```

1. **Parse** — tree-sitter extracts functions and eligible logic blocks (6+ lines with control flow)
2. **Embed** — Ollama generates vector embeddings for each chunk (any embedding model)
3. **Store** — SQLite + sqlite-vec for fast KNN cosine search
4. **Rank** — combines embedding distance with Jaccard token overlap for accurate re-ranking

Everything runs locally. No cloud dependencies. Single binary, no runtime needed.

## Prerequisites

- [Ollama](https://ollama.com) running locally
- Any embedding model — auto-detected; prefers `nomic-embed-code`

```bash
ollama pull nomic-embed-code
```

## Install

One-liner that downloads the binaries, adds them to your PATH, and sets up the skill + MCP config globally for your editor.

Pass one or more tool names: `claude-code`, `claude-desktop`, `cursor`, `opencode`. Omit to install binaries only.

**macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/tulgardinc/vibe-check/release/install.sh | bash -s -- claude-code
```

**Windows (PowerShell):**

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/tulgardinc/vibe-check/release/install.ps1))) claude-code
```

Multiple tools at once:

```bash
curl -fsSL https://raw.githubusercontent.com/tulgardinc/vibe-check/release/install.sh | bash -s -- claude-code claude-desktop cursor
```

The installer auto-detects your platform and architecture. It installs:
- `vibec` — CLI tool
- `vibecheck-mcp` — MCP server
- [`SKILL.md`](SKILL.md) — AI skill with setup guide and workflow reference (placed in the global location for your editor)
- MCP server configuration (created or merged into your editor's global config)

<details>
<summary><b>Build from source</b></summary>

```bash
git clone https://github.com/tulgardinc/vibe-check.git
cd vibe-check
cargo build --release
```

Binaries are in `target/release/`:
- `vibec` — CLI
- `vibecheck-mcp` — MCP server
</details>

## Recommended workflow

1. **Index after large changes** — re-index whenever you merge a big PR, finish a refactor, or pull in new code. This keeps the index fresh so results stay relevant.
   ```bash
   vibec index
   ```

2. **Use `query` for targeted checks** — instead of scanning the entire codebase, point `query` at the specific files or snippets you're working on. This gives a much better signal-to-noise ratio than a full scan.
   ```bash
   vibec query src/utils/newHelper.ts
   ```

3. **Use `scan` for periodic audits** — run a full scan occasionally (e.g. before a release or after a large feature lands) to catch duplication that slipped through.
   ```bash
   vibec scan
   ```

4. **Exclude false positives** — when results include intentional duplication (test mocks, generated code, etc.), add exclusions so future results are cleaner.

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
vibec index [path]       --force           # Full re-index
                         --dry-run         # Show what would be indexed
vibec query <file>       --top-k 10        # More candidates
                         --threshold 0.2   # Stricter matching
                         --json            # JSON output
                         --stdin           # Read from stdin
vibec scan               --top-n 100       # More pairs
                         --threshold 0.2   # Stricter
                         --json            # JSON output
```

### Global flags

These apply to all commands:

```bash
--model <name>           # Embedding model (overrides VIBECHECK_MODEL)
--ollama-host <url>      # Ollama server URL (overrides OLLAMA_HOST)
--dimensions <n>         # Override embedding dimensions
--context-length <n>     # Override model context length in tokens
--max-input-bytes <n>    # Override max input bytes for truncation
--query-prefix <str>     # Query prefix for search inputs (auto-detected for Nomic models)
--db <path>              # Database file path
--verbose                # Verbose output
```

## Configuration

### Environment variables

| Variable | Purpose | Default |
|----------|---------|---------|
| `VIBECHECK_MODEL` | Embedding model name | Auto-detected (`nomic-embed-code` preferred) |
| `OLLAMA_HOST` | Ollama server URL | `http://localhost:11434` |
| `VIBECHECK_DIMENSIONS` | Embedding dimensions | Model default |
| `VIBECHECK_QUERY_PREFIX` | Query prefix for search | Auto-detected for Nomic models |
| `VIBECHECK_CONTEXT_LENGTH` | Context length in tokens | Model default |
| `VIBECHECK_MAX_INPUT_BYTES` | Max input bytes for truncation | 16,000 |

Any embedding model supported by Ollama can be used. The model is auto-detected from your local Ollama instance, preferring `nomic-embed-code` if available. Override with `--model` or `VIBECHECK_MODEL`.

## MCP server

The MCP server lets AI assistants (Claude Code, Opencode, etc.) use vibecheck directly.

### Setup with `.mcp.json`

Add to your project's `.mcp.json` for Claude or equivalent:

```json
{
  "mcpServers": {
    "vibecheck": {
      "type": "stdio",
      "command": "vibecheck-mcp",
      "env": {
        "VIBECHECK_MODEL": "nomic-ai/nomic-embed-code",
        "VIBECHECK_QUERY_PREFIX": "search_query: "
      }
    }
  }
}
```
### Exposed tools

| Tool | Purpose |
|------|---------|
| `vibecheck_query` | Find similar functions/blocks — accepts a `file` path or raw `source` code |
| `vibecheck_index` | Build or update the semantic index |
| `vibecheck_index_stop` | Stop a running index operation (progress is saved) |
| `vibecheck_scan` | Find all similar pairs across the codebase |
| `vibecheck_status` | Check index health |
| `vibecheck_add_exclusion` | Suppress a false positive function pair match |
| `vibecheck_add_file_exclusion` | Exclude a file or glob pattern from indexing and results |
| `vibecheck_add_file_pair_exclusion` | Exclude all comparisons between functions in two files |
| `vibecheck_add_file_group_exclusion` | Exclude comparisons within a group of files (expands to all pairs) |

All tools that call Ollama accept optional `model`, `ollamaHost`, `dimensions`, `contextLength`, `maxInputBytes`, `queryPrefix`, and `db` overrides.

## Supported languages

- TypeScript (`.ts`)
- TSX (`.tsx`)
- JavaScript (`.js`, `.jsx`, `.mjs`, `.cjs`)
- Rust (`.rs`)
- Python (`.py`)

Test files (`.test.*`, `.spec.*`) and declaration files (`.d.ts`, `.d.tsx`) are automatically excluded.

## Limitations

- **Functions only** — top-level code outside of functions (module-level logic, script-style files) is not indexed. Only named functions, methods, and eligible inline blocks within them are captured.

## Exclusions

Exclusions are stored in `.vibecheck-ignore.json` at the project root. There are three levels:

**Function pair** — suppress a specific false positive match between two functions:
```bash
# Via MCP: use vibecheck_add_exclusion with the signatureHash values from results
```

**File** — exclude a file or glob pattern from indexing entirely:
```bash
# Via MCP: use vibecheck_add_file_exclusion with a pattern like "src/generated/**"
```

**File pair** — prevent any functions in two files from being compared:
```bash
# Via MCP: use vibecheck_add_file_pair_exclusion with fileA and fileB paths
```

**File group** — same as file pair, but for multiple files at once (expands to all pairwise combinations):
```bash
# Via MCP: use vibecheck_add_file_group_exclusion with a list of file paths
```

You can also edit `.vibecheck-ignore.json` directly:

```json
{
  "version": 1,
  "exclusions": [],
  "fileExclusions": [
    { "pattern": "src/generated/**", "reason": "auto-generated", "added": "2025-01-01" }
  ],
  "filePairExclusions": [
    { "a": "src/utils/math.ts", "b": "src/legacy/math.ts", "reason": "intentional fork", "added": "2025-01-01" }
  ]
}
```

File exclusion patterns use gitignore syntax (relative to project root). For pair exclusions (both function and file), `a` and `b` are interchangeable — order doesn't matter.

## License

MIT
