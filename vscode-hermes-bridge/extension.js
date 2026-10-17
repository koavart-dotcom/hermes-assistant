// Hermes Bridge — a local MCP server inside VS Code (UI side, so it runs on the PC even
// for Remote-SSH windows). Hermes Agent connects to http://127.0.0.1:<port>/mcp with a
// Bearer token kept in %LOCALAPPDATA%\ua.koavart.hermes\vscode-bridge.token.
// Only one window owns the port; the others stay passive and take over if it closes.
"use strict";
const vscode = require("vscode");
const http = require("http");
const fs = require("fs");
const os = require("os");
const path = require("path");
const crypto = require("crypto");

const PROTOCOL_VERSION = "2025-03-26";
const MAX_TEXT = 20000;
let server = null;
let retry = null;
let out = null;

function tokenPath() {
  const base = process.env.LOCALAPPDATA || path.join(os.homedir(), ".local", "share");
  return path.join(base, "ua.koavart.hermes", "vscode-bridge.token");
}

function token() {
  const p = tokenPath();
  try {
    const t = fs.readFileSync(p, "utf8").trim();
    if (t) return t;
  } catch (_) {
    /* create below */
  }
  const t = crypto.randomBytes(32).toString("hex");
  fs.mkdirSync(path.dirname(p), { recursive: true });
  fs.writeFileSync(p, t, { mode: 0o600 });
  return t;
}

function log(msg) {
  out && out.appendLine(`[${new Date().toISOString().slice(11, 19)}] ${msg}`);
}

const cfg = () => vscode.workspace.getConfiguration("hermesBridge");

/** `/home/...` → Remote-SSH URI on the configured host; `C:\...` → local file; full URIs as is. */
function toUri(p) {
  if (/^[a-z][a-z0-9+.-]*:\/\//i.test(p)) return vscode.Uri.parse(p);
  if (p.startsWith("/") && cfg().get("remoteHost")) {
    return vscode.Uri.from({ scheme: "vscode-remote", authority: `ssh-remote+${cfg().get("remoteHost")}`, path: p });
  }
  if (p.startsWith("~/") && cfg().get("remoteHost")) {
    return vscode.Uri.from({ scheme: "vscode-remote", authority: `ssh-remote+${cfg().get("remoteHost")}`, path: "/home/admin3_1/" + p.slice(2) });
  }
  // Relative to the first workspace folder of this window.
  const ws = vscode.workspace.workspaceFolders && vscode.workspace.workspaceFolders[0];
  if (ws && !path.isAbsolute(p)) return vscode.Uri.joinPath(ws.uri, p);
  return vscode.Uri.file(p);
}

const text = (t) => ({ content: [{ type: "text", text: typeof t === "string" ? t : JSON.stringify(t, null, 1) }] });
const fail = (t) => ({ content: [{ type: "text", text: t }], isError: true });
const clip = (s) => (s.length > MAX_TEXT ? s.slice(0, MAX_TEXT) + `\n… (${s.length - MAX_TEXT} chars more)` : s);

// Editor commands Hermes may run (by id) — read-mostly and reversible.
const COMMANDS = {
  save_all: "workbench.action.files.saveAll",
  format: "editor.action.formatDocument",
  toggle_terminal: "workbench.action.terminal.toggleTerminal",
  new_terminal: "workbench.action.terminal.new",
  problems: "workbench.actions.view.problems",
  explorer: "workbench.view.explorer",
  search: "workbench.view.search",
  source_control: "workbench.view.scm",
  zen_mode: "workbench.action.toggleZenMode",
  close_editor: "workbench.action.closeActiveEditor",
  close_all: "workbench.action.closeAllEditors",
  split: "workbench.action.splitEditor",
  quick_open: "workbench.action.quickOpen",
  reload_window: "workbench.action.reloadWindow",
  go_to_definition: "editor.action.revealDefinition",
  rename_symbol: "editor.action.rename",
  comment_line: "editor.action.commentLine",
  fold_all: "editor.foldAll",
  unfold_all: "editor.unfoldAll",
  copilot_chat: "workbench.action.chat.open",
};

const TOOLS = [
  {
    name: "vscode_status",
    description:
      "What is open in VS Code right now: workspace folders, active file with cursor line, selected text, open tabs. Call first when the user says «здесь», «этот файл», «то, что выделено».",
    inputSchema: { type: "object", properties: {} },
  },
  {
    name: "vscode_open",
    description:
      "Open a file (optionally at a line) or a project folder in VS Code. Paths starting with / or ~/ are on the dev server (Remote-SSH host dev-artem), e.g. ~/projects/ai-agent or /home/admin3_1/projects/ai-agent/src/main.tsx. Folders open in a new window unless new_window=false.",
    inputSchema: {
      type: "object",
      properties: {
        path: { type: "string", description: "File or folder path, or a full vscode-remote:// URI" },
        line: { type: "number", description: "1-based line for files" },
        new_window: { type: "boolean", description: "For folders: open in a new window (default true)" },
      },
      required: ["path"],
    },
  },
  {
    name: "vscode_read",
    description: "Read a file (or a line range) through VS Code — works for files on the dev server too. Without path reads the active editor.",
    inputSchema: {
      type: "object",
      properties: {
        path: { type: "string" },
        start_line: { type: "number", description: "1-based, inclusive" },
        end_line: { type: "number", description: "1-based, inclusive" },
      },
    },
  },
  {
    name: "vscode_diagnostics",
    description: "Errors and warnings from VS Code language servers (Problems panel): for the active file, a given path, or the whole workspace (scope=workspace).",
    inputSchema: {
      type: "object",
      properties: {
        path: { type: "string" },
        scope: { type: "string", enum: ["active", "workspace"] },
        include_warnings: { type: "boolean" },
      },
    },
  },
  {
    name: "vscode_command",
    description: "Run an editor command by short name: " + Object.keys(COMMANDS).join(", ") + ".",
    inputSchema: {
      type: "object",
      properties: { name: { type: "string", enum: Object.keys(COMMANDS) } },
      required: ["name"],
    },
  },
  {
    name: "vscode_replace_selection",
    description: "Replace the current selection in the active editor with new text (or insert at the cursor). The change stays unsaved, the user can undo with Ctrl+Z. Use only when the user asked to change the code.",
    inputSchema: { type: "object", properties: { text: { type: "string" } }, required: ["text"] },
  },
];

function editorInfo(ed) {
  if (!ed) return null;
  const sel = ed.selection;
  return {
    file: ed.document.uri.toString(),
    language: ed.document.languageId,
    dirty: ed.document.isDirty,
    cursor_line: sel.active.line + 1,
    selection: sel.isEmpty ? "" : clip(ed.document.getText(sel)),
    selection_lines: sel.isEmpty ? null : [sel.start.line + 1, sel.end.line + 1],
  };
}

async function callTool(name, args) {
  switch (name) {
    case "vscode_status": {
      const tabs = [];
      for (const g of vscode.window.tabGroups.all)
        for (const t of g.tabs) if (t.input && t.input.uri) tabs.push(t.input.uri.toString());
      return text({
        window_folders: (vscode.workspace.workspaceFolders || []).map((f) => f.uri.toString()),
        remote: vscode.env.remoteName || null,
        active: editorInfo(vscode.window.activeTextEditor),
        open_tabs: tabs.slice(0, 40),
      });
    }
    case "vscode_open": {
      const uri = toUri(String(args.path || ""));
      let isDir = false;
      try {
        isDir = (await vscode.workspace.fs.stat(uri)).type === vscode.FileType.Directory;
      } catch (_) {
        // Remote folders are not stat-able from another window: guess by extension.
        isDir = !path.extname(uri.path);
      }
      if (isDir) {
        await vscode.commands.executeCommand("vscode.openFolder", uri, { forceNewWindow: args.new_window !== false });
        return text(`opening folder ${uri.toString()}`);
      }
      const doc = await vscode.workspace.openTextDocument(uri);
      const ed = await vscode.window.showTextDocument(doc, { preview: false });
      if (args.line) {
        const pos = new vscode.Position(Math.max(0, args.line - 1), 0);
        ed.selection = new vscode.Selection(pos, pos);
        ed.revealRange(new vscode.Range(pos, pos), vscode.TextEditorRevealType.InCenter);
      }
      return text(`opened ${uri.toString()}${args.line ? `:${args.line}` : ""}`);
    }
    case "vscode_read": {
      let doc;
      if (args.path) doc = await vscode.workspace.openTextDocument(toUri(String(args.path)));
      else if (vscode.window.activeTextEditor) doc = vscode.window.activeTextEditor.document;
      else return fail("no active editor and no path given");
      const s = Math.max(1, args.start_line || 1);
      const e = Math.min(doc.lineCount, args.end_line || doc.lineCount);
      const lines = [];
      for (let i = s; i <= e; i++) lines.push(`${i}\t${doc.lineAt(i - 1).text}`);
      return text(`${doc.uri.toString()} (${doc.lineCount} lines)\n` + clip(lines.join("\n")));
    }
    case "vscode_diagnostics": {
      const sev = args.include_warnings === false ? 0 : 1; // 0 error, 1 warning
      let entries;
      if (args.scope === "workspace") entries = vscode.languages.getDiagnostics();
      else {
        const uri = args.path ? toUri(String(args.path)) : vscode.window.activeTextEditor && vscode.window.activeTextEditor.document.uri;
        if (!uri) return fail("no active editor");
        entries = [[uri, vscode.languages.getDiagnostics(uri)]];
      }
      const items = [];
      for (const [uri, diags] of entries)
        for (const d of diags)
          if (d.severity <= sev)
            items.push(`${d.severity === 0 ? "ERROR" : "warn"} ${uri.path.split("/").slice(-2).join("/")}:${d.range.start.line + 1} ${d.message}${d.source ? ` [${d.source}]` : ""}`);
      return text(items.length ? items.slice(0, 80).join("\n") + (items.length > 80 ? `\n… ${items.length - 80} more` : "") : "no problems");
    }
    case "vscode_command": {
      const id = COMMANDS[args.name];
      if (!id) return fail(`unknown command ${args.name}`);
      await vscode.commands.executeCommand(id);
      return text(`ran ${args.name}`);
    }
    case "vscode_replace_selection": {
      const ed = vscode.window.activeTextEditor;
      if (!ed) return fail("no active editor");
      const ok = await ed.edit((b) => b.replace(ed.selection, String(args.text || "")));
      return ok ? text("replaced (unsaved, Ctrl+Z to undo)") : fail("edit rejected");
    }
    default:
      return fail(`unknown tool ${name}`);
  }
}

async function handleRpc(msg) {
  const { method, id, params = {} } = msg || {};
  let result;
  if (method === "initialize")
    result = { protocolVersion: params.protocolVersion || PROTOCOL_VERSION, capabilities: { tools: {} }, serverInfo: { name: "hermes-vscode-bridge", version: "0.1.0" } };
  else if (method === "ping") result = {};
  else if (method === "tools/list") result = { tools: TOOLS };
  else if (method === "tools/call") {
    try {
      result = await callTool(String(params.name), params.arguments || {});
      log(`${params.name} ${JSON.stringify(params.arguments || {}).slice(0, 120)}`);
    } catch (e) {
      result = fail(String((e && e.message) || e));
      log(`${params.name} failed: ${result.content[0].text}`);
    }
  } else if (!method || method.startsWith("notifications/")) return null;
  else return { jsonrpc: "2.0", id, error: { code: -32601, message: `method not found: ${method}` } };
  return { jsonrpc: "2.0", id, result };
}

function send(res, code, body) {
  const data = body === null ? "" : JSON.stringify(body);
  res.writeHead(code, { "Content-Type": "application/json", "Content-Length": Buffer.byteLength(data) });
  res.end(data);
}

function start() {
  const port = cfg().get("port") || 8656;
  const tok = token();
  const s = http.createServer((req, res) => {
    if (req.method === "GET" && req.url === "/health") return send(res, 200, { ok: true, folders: (vscode.workspace.workspaceFolders || []).length });
    if (req.method !== "POST" || req.url !== "/mcp") return send(res, 404, { error: "not found" });
    if (req.headers.authorization !== `Bearer ${tok}`) return send(res, 401, { error: "unauthorized" });
    let raw = "";
    req.on("data", (c) => (raw += c));
    req.on("end", async () => {
      let msg;
      try {
        msg = JSON.parse(raw || "{}");
      } catch (_) {
        return send(res, 400, { jsonrpc: "2.0", id: null, error: { code: -32700, message: "parse error" } });
      }
      if (Array.isArray(msg)) {
        const r = (await Promise.all(msg.map(handleRpc))).filter(Boolean);
        return r.length ? send(res, 200, r) : send(res, 202, null);
      }
      const r = await handleRpc(msg);
      return r ? send(res, 200, r) : send(res, 202, null);
    });
  });
  s.on("error", (e) => {
    if (e.code === "EADDRINUSE") {
      // Another window serves; try again later in case it closes.
      retry = setTimeout(start, 15000);
    } else log(`server error: ${e.message}`);
  });
  s.listen(port, "127.0.0.1", () => {
    server = s;
    log(`MCP server on http://127.0.0.1:${port}/mcp (this window owns it)`);
  });
}

function activate(context) {
  out = vscode.window.createOutputChannel("Hermes Bridge");
  context.subscriptions.push(out);
  context.subscriptions.push(
    vscode.commands.registerCommand("hermesBridge.status", () => {
      vscode.window.showInformationMessage(
        server ? `Hermes Bridge: этот window обслуживает 127.0.0.1:${cfg().get("port")}` : "Hermes Bridge: порт обслуживает другое окно VS Code",
      );
    }),
  );
  start();
}

function deactivate() {
  if (retry) clearTimeout(retry);
  if (server) server.close();
}

module.exports = { activate, deactivate, _test: { toUri, handleRpc, TOOLS, COMMANDS } };
