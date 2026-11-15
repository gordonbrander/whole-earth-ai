#!/bin/bash
# Whole Earth drive: double-click to start the offline assistant.
# Runs llamafile with the drive's model and zim-mcp as its MCP server, on
# 127.0.0.1 only, then opens the chat page. Close this window to stop.
set -e
cd "$(dirname "$0")"
DRIVE="$PWD"
PORT=8080
CONFIG="${TMPDIR:-/tmp}/whole-earth-mcp.json"
cat > "$CONFIG" <<JSON
{"mcpServers":{"wiki":{"command":"$DRIVE/bin/macos-universal/zim-mcp","args":["serve","--dir","$DRIVE/zim","--no-library"]}}}
JSON
(until curl -sf "http://127.0.0.1:$PORT/health" >/dev/null; do sleep 1; done; open "http://127.0.0.1:$PORT") &
echo "Starting… the chat page opens in your browser when it's ready. Close this window to stop."
# --unsecure: llamafile's sandbox (Linux, CPU mode) blocks starting zim-mcp for each tool call. It still listens on 127.0.0.1 only.
exec "$DRIVE/llamafile.exe" --server -m "$DRIVE/models/model.gguf" --host 127.0.0.1 --port "$PORT" \
  -ngl 999 --unsecure --mcp-servers-config "$CONFIG"
