param(
    [Parameter(Position = 0, ValueFromRemainingArguments)]
    [string[]]$Tools
)

$ErrorActionPreference = 'Stop'

$Repo = "tulgardinc/vibe-check"
$BaseUrl = "https://github.com/$Repo/releases/latest/download"
$SkillUrl = "https://raw.githubusercontent.com/$Repo/release/SKILL.md"

function Info($msg)  { Write-Host "info  " -ForegroundColor Blue -NoNewline; Write-Host $msg }
function Ok($msg)    { Write-Host "  ok  " -ForegroundColor Green -NoNewline; Write-Host $msg }
function Warn($msg)  { Write-Host "warn  " -ForegroundColor Yellow -NoNewline; Write-Host $msg }
function Err($msg)   { Write-Host "error " -ForegroundColor Red -NoNewline; Write-Host $msg; exit 1 }

# --- Detect architecture ---

$Arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'aarch64' }
        elseif ([Environment]::Is64BitOperatingSystem) { 'x86_64' }
        else { Err "32-bit systems are not supported" }

# --- Install binaries ---

$BinDir = Join-Path $env:USERPROFILE '.local\bin'
if (-not (Test-Path $BinDir)) { New-Item -ItemType Directory -Path $BinDir -Force | Out-Null }

Info "Installing vibecheck (windows-$Arch)..."

foreach ($bin in @('vibec', 'vibecheck-mcp')) {
    $url = "$BaseUrl/$bin-windows-$Arch.exe"
    $dest = Join-Path $BinDir "$bin.exe"
    Invoke-WebRequest -Uri $url -OutFile $dest -UseBasicParsing
    Ok "$bin.exe -> $dest"
}

if ($env:PATH -notlike "*$BinDir*") {
    Write-Host ""
    Warn "$BinDir is not in your PATH. Add it:"
    Write-Host "    [Environment]::SetEnvironmentVariable('PATH', `"$BinDir;`$env:PATH`", 'User')"
}

# --- Tool-specific setup ---

if (-not $Tools -or $Tools.Count -eq 0) {
    Write-Host ""
    Info "Binaries installed. To also set up editor skills, re-run with tool names:"
    Write-Host "    & ([scriptblock]::Create((irm https://raw.githubusercontent.com/$Repo/release/install.ps1))) claude-code cursor"
    Write-Host ""
    Write-Host "    Supported: claude-code, claude-desktop, cursor, opencode"
    exit 0
}

function Add-McpConfig($McpFile) {
    $dir = Split-Path $McpFile -Parent
    if ($dir -and -not (Test-Path $dir)) {
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
    }

    if (-not (Test-Path $McpFile)) {
        @'
{
  "mcpServers": {
    "vibecheck": {
      "type": "stdio",
      "command": "vibecheck-mcp"
    }
  }
}
'@ | Set-Content $McpFile -Encoding UTF8
        Ok "Created $McpFile"
    }
    elseif ((Get-Content $McpFile -Raw) -match '"vibecheck"') {
        Ok "$McpFile already has vibecheck"
    }
    else {
        $data = Get-Content $McpFile -Raw | ConvertFrom-Json
        if (-not $data.mcpServers) {
            $data | Add-Member -NotePropertyName mcpServers -NotePropertyValue ([PSCustomObject]@{})
        }
        $data.mcpServers | Add-Member -NotePropertyName vibecheck -NotePropertyValue ([PSCustomObject]@{
            type    = "stdio"
            command = "vibecheck-mcp"
        })
        $data | ConvertTo-Json -Depth 10 | Set-Content $McpFile -Encoding UTF8
        Ok "Added vibecheck to $McpFile"
    }
}

$Home_ = $env:USERPROFILE

foreach ($Tool in $Tools) {
    Write-Host ""
    Info "Setting up for $Tool..."

    switch -Regex ($Tool) {
        '^(claude-code|claude)$' {
            $skillDir = Join-Path $Home_ '.claude\skills\vibe-check'
            New-Item -ItemType Directory -Path $skillDir -Force | Out-Null
            Invoke-WebRequest -Uri $SkillUrl -OutFile (Join-Path $skillDir 'SKILL.md') -UseBasicParsing
            Ok "Skill -> ~/.claude/skills/vibe-check/SKILL.md"
            Add-McpConfig (Join-Path $Home_ '.claude.json')
            Write-Host ""
            Info "Run /vibecheck in Claude Code to get started"
        }
        '^claude-desktop$' {
            $desktopConfig = Join-Path $env:APPDATA 'Claude\claude_desktop_config.json'
            Add-McpConfig $desktopConfig
        }
        '^cursor$' {
            $cursorRules = Join-Path $Home_ '.cursor\rules'
            New-Item -ItemType Directory -Path $cursorRules -Force | Out-Null
            Invoke-WebRequest -Uri $SkillUrl -OutFile (Join-Path $cursorRules 'vibecheck.md') -UseBasicParsing
            Ok "Skill -> ~/.cursor/rules/vibecheck.md"
            Add-McpConfig (Join-Path $Home_ '.cursor\mcp.json')
        }
        '^opencode$' {
            Invoke-WebRequest -Uri $SkillUrl -OutFile (Join-Path $Home_ 'VIBECHECK.md') -UseBasicParsing
            Ok "Skill -> ~/VIBECHECK.md"
            Add-McpConfig (Join-Path $Home_ '.mcp.json')
        }
        default {
            Warn "Unknown tool: $Tool (supported: claude-code, claude-desktop, cursor, opencode). Skipping."
        }
    }
}

Write-Host ""
Write-Host "Done! " -ForegroundColor Green -NoNewline
Write-Host "vibecheck is ready."
