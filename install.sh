#!/usr/bin/env bash
set -euo pipefail

REPO="tulgardinc/vibe-check"
TOOLS=("$@")
BASE_URL="https://github.com/$REPO/releases/latest/download"
SKILL_URL="https://raw.githubusercontent.com/$REPO/release/SKILL.md"

# --- Output helpers ---

info()  { printf "\033[0;34minfo\033[0m  %s\n" "$1"; }
ok()    { printf "\033[0;32m  ok\033[0m  %s\n" "$1"; }
warn()  { printf "\033[1;33mwarn\033[0m  %s\n" "$1"; }
err()   { printf "\033[0;31merror\033[0m %s\n" "$1"; exit 1; }

# --- Detect platform ---

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Darwin) PLATFORM="darwin" ;;
  Linux)  PLATFORM="linux" ;;
  *)      err "Unsupported OS: $OS" ;;
esac

case "$ARCH" in
  x86_64|amd64)  ARCH="x86_64" ;;
  aarch64|arm64) ARCH="aarch64" ;;
  *)             err "Unsupported architecture: $ARCH" ;;
esac

# --- Install binaries ---

if [ "$PLATFORM" = "darwin" ]; then
  BIN_DIR="/usr/local/bin"
else
  BIN_DIR="${HOME}/.local/bin"
  mkdir -p "$BIN_DIR"
fi

info "Installing vibecheck ($PLATFORM-$ARCH)..."

for bin in vibec vibecheck-mcp; do
  curl -fsSL "$BASE_URL/${bin}-${PLATFORM}-${ARCH}" -o "$BIN_DIR/$bin"
  chmod +x "$BIN_DIR/$bin"
  ok "$bin -> $BIN_DIR/$bin"
done

if ! echo "$PATH" | tr ':' '\n' | grep -qx "$BIN_DIR"; then
  echo ""
  warn "$BIN_DIR is not in your PATH. Add it to your shell profile:"
  echo "    export PATH=\"$BIN_DIR:\$PATH\""
fi

# --- Tool-specific setup ---

if [ ${#TOOLS[@]} -eq 0 ]; then
  echo ""
  info "Binaries installed. To also set up editor skills, re-run with tool names:"
  echo "    curl -fsSL https://raw.githubusercontent.com/$REPO/release/install.sh | bash -s -- claude-code cursor"
  echo ""
  echo "    Supported: claude-code, cursor, opencode"
  exit 0
fi

add_mcp_config() {
  local mcp_file="$1"
  local mcp_dir
  mcp_dir="$(dirname "$mcp_file")"

  [ "$mcp_dir" != "." ] && mkdir -p "$mcp_dir"

  if [ ! -f "$mcp_file" ]; then
    cat > "$mcp_file" <<'EOF'
{
  "mcpServers": {
    "vibecheck": {
      "type": "stdio",
      "command": "vibecheck-mcp"
    }
  }
}
EOF
    ok "Created $mcp_file"
  elif grep -q '"vibecheck"' "$mcp_file" 2>/dev/null; then
    ok "$mcp_file already has vibecheck"
  elif command -v python3 &>/dev/null; then
    python3 -c "
import json
with open('$mcp_file') as f:
    data = json.load(f)
data.setdefault('mcpServers', {})['vibecheck'] = {'type': 'stdio', 'command': 'vibecheck-mcp'}
with open('$mcp_file', 'w') as f:
    json.dump(data, f, indent=2)
    f.write('\n')
"
    ok "Added vibecheck to $mcp_file"
  else
    warn "Could not auto-update $mcp_file (python3 not found). Add manually:"
    echo '    "vibecheck": { "type": "stdio", "command": "vibecheck-mcp" }'
  fi
}

for TOOL in "${TOOLS[@]}"; do
  echo ""
  info "Setting up for $TOOL..."

  case "$TOOL" in
    claude-code|claude)
      mkdir -p ".claude/skills/vibe-check"
      curl -fsSL "$SKILL_URL" -o ".claude/skills/vibe-check/SKILL.md"
      ok "Skill -> .claude/skills/vibe-check/SKILL.md"
      add_mcp_config ".mcp.json"
      echo ""
      info "Run /vibecheck in Claude Code to get started"
      ;;
    cursor)
      mkdir -p ".cursor/rules"
      curl -fsSL "$SKILL_URL" -o ".cursor/rules/vibecheck.md"
      ok "Skill -> .cursor/rules/vibecheck.md"
      add_mcp_config ".cursor/mcp.json"
      ;;
    opencode)
      curl -fsSL "$SKILL_URL" -o "VIBECHECK.md"
      ok "Skill -> VIBECHECK.md"
      add_mcp_config ".mcp.json"
      ;;
    *)
      warn "Unknown tool: $TOOL (supported: claude-code, cursor, opencode). Skipping."
      ;;
  esac
done

echo ""
printf "\033[0;32mDone!\033[0m vibecheck is ready.\n"
