@echo off
rem Whole Earth AI: double-click to start the offline assistant.
rem Runs llamafile with the drive's model and zim-mcp as its MCP server, on
rem 127.0.0.1 only, then opens the chat page. Close this window to stop.
setlocal
rem Read the folder once: %~dp0 can go wrong after cd when started through a quoted path.
set "HERE=%~dp0"
cd /d "%HERE%"
set "DRIVE=%HERE:\=/%"
set "PORT=8080"
set "CONFIG=%TEMP%\whole-earth-mcp.json"
> "%CONFIG%" echo {"mcpServers":{"wiki":{"command":"%DRIVE%bin/windows-x64/zim-mcp.exe","args":["serve","--dir","%DRIVE%zim","--no-library"]}}}
start "" /b powershell -NoProfile -WindowStyle Hidden -Command "while ($true) { try { Invoke-WebRequest -UseBasicParsing http://127.0.0.1:%PORT%/health | Out-Null; break } catch { Start-Sleep 1 } }; Start-Process http://127.0.0.1:%PORT%"
echo Starting... the chat page opens in your browser when it's ready. Close this window to stop.
rem --unsecure: llamafile's sandbox (Linux, CPU mode) blocks starting zim-mcp for each tool call. It still listens on 127.0.0.1 only.
"%HERE%llamafile.exe" --server -m "%HERE%models\model.gguf" --host 127.0.0.1 --port %PORT% -ngl 999 --unsecure --mcp-servers-config "%CONFIG%"
