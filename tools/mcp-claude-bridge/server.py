#!/usr/bin/env python3
"""MCP bridge: lets Hermes Agent (on Artem's PC) hand tasks to Claude Code on
this server, under the Claude Pro subscription (`claude -p`, no API key).

Transport: MCP "streamable HTTP" in its simplest form — `POST /mcp` with one
JSON-RPC 2.0 message, one JSON response (no SSE). Auth: `Authorization:
Bearer <token>` (token file next to this script or $CLAUDE_BRIDGE_TOKEN).

Tools:
  claude_task(project, task, timeout_sec=1500)  -> result text of `claude -p`
  claude_projects()                             -> allowed projects
  claude_status()                               -> running/last tasks

Guards: projects allowlist (projects.json), one task at a time, timeout,
Claude's own permission mode `acceptEdits` inside the project directory only,
JSON-lines log. Standard library only.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
PORT = int(os.environ.get("CLAUDE_BRIDGE_PORT", "8650"))
BIND = os.environ.get("CLAUDE_BRIDGE_BIND", "0.0.0.0")
TOKEN_FILE = HERE / "bridge.token"
PROJECTS_FILE = HERE / "projects.json"
LOG_FILE = HERE / "bridge.log"
PROTOCOL_VERSION = "2025-03-26"
MAX_TIMEOUT = 3600
DEFAULT_TIMEOUT = 1500

_lock = threading.Lock()
_state: dict[str, Any] = {"running": None, "last": []}


def log(record: dict[str, Any]) -> None:
    record.setdefault("ts", time.time())
    try:
        with LOG_FILE.open("a", encoding="utf-8") as f:
            f.write(json.dumps(record, ensure_ascii=False) + "\n")
    except OSError:
        pass


def token() -> str:
    env = os.environ.get("CLAUDE_BRIDGE_TOKEN", "").strip()
    if env:
        return env
    if TOKEN_FILE.exists():
        t = TOKEN_FILE.read_text(encoding="utf-8").strip()
        if len(t) >= 32:
            return t
    import secrets
    t = secrets.token_hex(32)
    TOKEN_FILE.write_text(t, encoding="utf-8")
    os.chmod(TOKEN_FILE, 0o600)
    return t


def projects() -> dict[str, dict[str, str]]:
    """{name: {"path": ..., "description": ...}} from projects.json."""
    if not PROJECTS_FILE.exists():
        return {}
    try:
        data = json.loads(PROJECTS_FILE.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return {}
    out: dict[str, dict[str, str]] = {}
    for name, spec in data.items():
        if isinstance(spec, str):
            spec = {"path": spec}
        path = os.path.expanduser(str(spec.get("path", "")))
        if path and os.path.isdir(path):
            out[str(name)] = {"path": path, "description": str(spec.get("description", ""))}
    return out


TOOLS = [
    {
        "name": "claude_task",
        "description": (
            "Hand a development or server-administration task to Claude Code on Artem's Linux "
            "server (runs under his Claude Pro subscription). Use for coding, debugging, git, "
            "deploys, infrastructure and anything that needs a strong model with full access to "
            "the project repositories. One task at a time; takes from seconds to many minutes. "
            "Describe the task in Russian or English, be specific; include what to report back."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "project": {"type": "string", "description": "Project name from claude_projects()."},
                "task": {"type": "string", "description": "What to do, in detail."},
                "timeout_sec": {"type": "integer", "description": f"Max seconds (default {DEFAULT_TIMEOUT}, max {MAX_TIMEOUT})."},
            },
            "required": ["project", "task"],
        },
    },
    {
        "name": "claude_projects",
        "description": "List the projects Claude Code may work in on the server.",
        "inputSchema": {"type": "object", "properties": {}},
    },
    {
        "name": "claude_status",
        "description": "Whether a Claude task is running now and the last results.",
        "inputSchema": {"type": "object", "properties": {}},
    },
]


def run_claude(project: str, task: str, timeout_sec: int) -> dict[str, Any]:
    allowed = projects()
    if project not in allowed:
        return {"ok": False, "error": f"unknown project `{project}`; known: {', '.join(allowed) or 'none'}"}
    cwd = allowed[project]["path"]
    timeout_sec = max(30, min(int(timeout_sec or DEFAULT_TIMEOUT), MAX_TIMEOUT))
    if not _lock.acquire(blocking=False):
        return {"ok": False, "error": "another Claude task is still running; try again later"}
    started = time.time()
    _state["running"] = {"project": project, "task": task[:200], "started": started}
    log({"event": "start", "project": project, "task": task[:500], "timeout": timeout_sec})
    cmd = [
        "claude", "-p", task,
        "--output-format", "json",
        "--permission-mode", "acceptEdits",
    ]
    try:
        proc = subprocess.run(
            cmd, cwd=cwd, capture_output=True, text=True, timeout=timeout_sec,
            env={**os.environ, "CLAUDE_BRIDGE": "1"},
        )
        raw = proc.stdout.strip()
        try:
            data = json.loads(raw)
            result_text = str(data.get("result") or "")
            is_error = bool(data.get("is_error"))
            meta = {k: data.get(k) for k in ("duration_ms", "num_turns", "session_id") if k in data}
        except json.JSONDecodeError:
            result_text, is_error, meta = raw[-4000:], proc.returncode != 0, {}
        if proc.returncode != 0 and not result_text:
            result_text = proc.stderr.strip()[-2000:] or f"claude exited with {proc.returncode}"
            is_error = True
        out = {"ok": not is_error, "result": result_text, "meta": meta, "seconds": round(time.time() - started, 1)}
    except subprocess.TimeoutExpired:
        out = {"ok": False, "error": f"timed out after {timeout_sec}s", "seconds": timeout_sec}
    except FileNotFoundError:
        out = {"ok": False, "error": "claude CLI not found on PATH"}
    finally:
        _state["running"] = None
        _lock.release()
    _state["last"] = ([{"project": project, "task": task[:200], "ok": out.get("ok"), "seconds": out.get("seconds"), "ts": time.time()}] + _state["last"])[:10]
    log({"event": "done", "project": project, **{k: v for k, v in out.items() if k != "result"}, "result_chars": len(out.get("result", "") or "")})
    return out


def call_tool(name: str, args: dict[str, Any]) -> dict[str, Any]:
    if name == "claude_projects":
        text = "\n".join(f"- {n}: {p['path']}" + (f" — {p['description']}" if p["description"] else "") for n, p in projects().items()) or "no projects configured"
        return {"content": [{"type": "text", "text": text}]}
    if name == "claude_status":
        return {"content": [{"type": "text", "text": json.dumps(_state, ensure_ascii=False, indent=1)}]}
    if name == "claude_task":
        out = run_claude(str(args.get("project", "")), str(args.get("task", "")), int(args.get("timeout_sec") or DEFAULT_TIMEOUT))
        if out.get("ok"):
            text = out["result"] + f"\n\n[claude: {out.get('seconds')} s]"
            return {"content": [{"type": "text", "text": text}]}
        return {"content": [{"type": "text", "text": f"Claude task failed: {out.get('error') or out.get('result')}"}], "isError": True}
    return {"content": [{"type": "text", "text": f"unknown tool {name}"}], "isError": True}


def handle_rpc(msg: dict[str, Any]) -> dict[str, Any] | None:
    method = msg.get("method")
    rid = msg.get("id")
    params = msg.get("params") or {}
    if method == "initialize":
        result = {
            "protocolVersion": params.get("protocolVersion") or PROTOCOL_VERSION,
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "claude-bridge", "version": "0.1.0"},
        }
    elif method == "notifications/initialized" or method is None and rid is None:
        return None
    elif method == "ping":
        result = {}
    elif method == "tools/list":
        result = {"tools": TOOLS}
    elif method == "tools/call":
        result = call_tool(str(params.get("name")), params.get("arguments") or {})
    elif method and method.startswith("notifications/"):
        return None
    else:
        return {"jsonrpc": "2.0", "id": rid, "error": {"code": -32601, "message": f"method not found: {method}"}}
    return {"jsonrpc": "2.0", "id": rid, "result": result}


class Handler(BaseHTTPRequestHandler):
    server_version = "claude-bridge/0.1"

    def _auth(self) -> bool:
        auth = self.headers.get("Authorization", "")
        return auth == f"Bearer {token()}"

    def _send(self, code: int, body: Any) -> None:
        data = json.dumps(body, ensure_ascii=False).encode() if body is not None else b""
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        if data:
            self.wfile.write(data)

    def do_GET(self) -> None:  # noqa: N802
        if self.path == "/health":
            self._send(200, {"ok": True, "running": _state["running"] is not None, "projects": list(projects())})
        else:
            self._send(404, {"error": "not found"})

    def do_POST(self) -> None:  # noqa: N802
        if self.path != "/mcp":
            return self._send(404, {"error": "not found"})
        if not self._auth():
            return self._send(401, {"error": "unauthorized"})
        length = int(self.headers.get("Content-Length") or 0)
        try:
            msg = json.loads(self.rfile.read(length) or b"{}")
        except json.JSONDecodeError:
            return self._send(400, {"jsonrpc": "2.0", "id": None, "error": {"code": -32700, "message": "parse error"}})
        if isinstance(msg, list):
            responses = [r for r in (handle_rpc(m) for m in msg) if r is not None]
            return self._send(200, responses) if responses else self._send(202, None)
        resp = handle_rpc(msg)
        if resp is None:
            return self._send(202, None)
        self._send(200, resp)

    def log_message(self, format: str, *args: Any) -> None:  # noqa: A002 — quiet
        del format, args


def main() -> int:
    token()  # create on first start
    srv = ThreadingHTTPServer((BIND, PORT), Handler)
    print(f"claude-bridge listening on {BIND}:{PORT}, projects: {list(projects())}", flush=True)
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
