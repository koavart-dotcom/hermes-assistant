# Hermes Bridge for VS Code

Local MCP server inside VS Code so the Hermes assistant (Jarvis) can work with the editor:
`vscode_status`, `vscode_open`, `vscode_read`, `vscode_diagnostics`, `vscode_command`,
`vscode_replace_selection`. Runs on the UI side (`extensionKind: ui`), so it serves on the PC
even when the window is connected to the dev server over Remote-SSH (host `dev-artem`).

- Endpoint: `http://127.0.0.1:8656/mcp`, `Authorization: Bearer <token>`; token file:
  `%LOCALAPPDATA%\ua.koavart.hermes\vscode-bridge.token` (created on first start).
- Only one VS Code window owns the port; others retry every 15 s.
- Hermes: `mcp_servers.vscode` with that URL and header (Hermes Assistant configures it).
- Build: `npx @vscode/vsce package --no-dependencies` → `hermes-bridge-<version>.vsix`.
