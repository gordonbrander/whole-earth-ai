#!/bin/sh
# Whole Earth AI: run `sh start-linux.sh` (or double-click) to start the
# offline assistant on 127.0.0.1 and open the chat page. Ctrl-C to stop.
set -e
cd "$(dirname "$0")"
DRIVE="$PWD"
PORT=8080
case "$(uname -m)" in
  x86_64|amd64) OS=linux-x64 ;;
  aarch64|arm64) OS=linux-arm64 ;;
  *) echo "Unsupported CPU: $(uname -m)"; exit 1 ;;
esac
BIN="$DRIVE/bin/$OS"
LLAMAFILE="$DRIVE/llamafile.exe"
# Drives are often mounted without permission to run programs (noexec):
# then run copies from a temp folder instead.
if ! "$BIN/zim-mcp" --version >/dev/null 2>&1; then
  TMP="${TMPDIR:-/tmp}/whole-earth"
  echo "This drive doesn't allow running programs; copying them to $TMP (once)…"
  mkdir -p "$TMP"
  cp -R "$BIN" "$TMP/"
  [ -f "$TMP/llamafile.exe" ] || cp "$LLAMAFILE" "$TMP/llamafile.exe"
  chmod +x "$TMP/$OS/zim-mcp" "$TMP/llamafile.exe"
  BIN="$TMP/$OS"
  LLAMAFILE="$TMP/llamafile.exe"
fi
CONFIG="${TMPDIR:-/tmp}/whole-earth-mcp.json"
cat > "$CONFIG" <<JSON
{"mcpServers":{"wiki":{"command":"$BIN/zim-mcp","args":["serve","--dir","$DRIVE/zim","--no-library"]}}}
JSON
(until curl -sf "http://127.0.0.1:$PORT/health" >/dev/null 2>&1 || wget -qO /dev/null "http://127.0.0.1:$PORT/health" 2>/dev/null; do sleep 1; done
 xdg-open "http://127.0.0.1:$PORT" >/dev/null 2>&1 || echo "Open http://127.0.0.1:$PORT in your browser.") &
echo "Starting… the chat page opens in your browser when it's ready. Ctrl-C to stop."
# `sh` runs llamafile's portable executable even where binfmt_misc would hand it to Wine.
# --unsecure: llamafile's sandbox (Linux, CPU mode) blocks starting zim-mcp for each tool call. It still listens on 127.0.0.1 only.
exec sh "$LLAMAFILE" --server -m "$DRIVE/models/model.gguf" --host 127.0.0.1 --port "$PORT" \
  -ngl 999 --unsecure --mcp-servers-config "$CONFIG"
