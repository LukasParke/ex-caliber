#!/usr/bin/env bash
# M2 acceptance demo: an unattended Claude Code session builds and exports a
# diagram purely through the excaliber MCP server. Requires: cargo build, claude CLI.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build -q -p xc
WORK="$(mktemp -d ~/exval-demo.XXXX)"
cat > "$WORK/mcp.json" <<JSON
{"mcpServers": {"excaliber": {"command": "$PWD/target/debug/xc", "args": ["mcp", "--file", "$WORK/demo.excalidraw"]}}}
JSON
claude -p "Use the excaliber MCP tools to build an architecture diagram: three boxes labeled Client, API Server, and PostgreSQL, arranged left to right. Connect Client to API Server with an arrow labeled REST, and API Server to PostgreSQL with an arrow labeled SQL. Take a screenshot to check your work, then export it as PNG to $WORK/demo.png. Report the element count when done." \
  --mcp-config "$WORK/mcp.json" --allowedTools "mcp__excaliber__*" --max-turns 30
echo "artifacts in $WORK"
