"""hermes_permissions — permission levels for Hermes Agent tool calls.

Installed by Hermes Assistant into ``$HERMES_HOME/plugins/hermes_permissions``.
On ``pre_tool_call`` every call is classified into one of four levels:

* ``allow``   — read-only or harmless, runs silently;
* ``confirm`` — escalated to the Hermes approval gate (``{"action": "approve"}``),
  which the Hermes Assistant chat shows as a card (once / session / always / deny);
* ``block``   — never runs (``{"action": "block"}``), the model gets the reason.

The policy is code-shipped defaults merged with an optional user override in
``$HERMES_HOME/hermes_permissions.json``. Decisions are appended as JSON lines to
``$HERMES_HOME/hermes_permissions.log``. Fail-closed: an internal error confirms.

Standard library only (the plugin runs inside the Hermes gateway process).
"""

from __future__ import annotations

import json
import os
import re
import time
from pathlib import Path
from typing import Any

__all__ = ["register", "classify", "load_policy", "DEFAULT_POLICY", "Decision"]

PLUGIN_NAME = "hermes_permissions"
LOG_FILE = "hermes_permissions.log"
POLICY_FILE = "hermes_permissions.json"
PREVIEW_CHARS = 200

# --------------------------------------------------------------------- policy --

# Terminal commands that never run, whatever the user says in chat. Matched
# case-insensitively against the whole command line.
BLOCKED_TERMINAL = [
    r"\b(shutdown|logoff)\b",
    r"\b(restart|stop)-computer\b",
    r"\bformat\b\s+[a-z]:",
    r"\b(diskpart|bcdedit|vssadmin|mkfs(\.\w+)?)\b",
    r"\bcipher\s+/w",
    r"\bdd\s+if=",
    r"\breg(\.exe)?\s+(delete|add)\s+\"?hk(lm|ey_local_machine)",
    r"\brm\s+(-[a-z]*r[a-z]*\s+|-[a-z]*\s+-[a-z]*r[a-z]*\s+)(/|~|\$home|/c/?|c:\\?)(\s|$|\*)",
    r"\b(rd|rmdir|del|erase)\b[^|&;]*\s/s\b[^|&;]*\\(windows|program files|users)\b",
    r"\bremove-item\b[^|&;]*\\(windows|program files)\b",
    r"\bnet\s+user\b",
    r"\bnetsh\s+advfirewall\b",
    r"\bschtasks\s+/delete\b",
    r"\bwmic\b[^|&;]*\bdelete\b",
    r":\(\)\s*\{\s*:\s*\|\s*:\s*&\s*\}",  # fork bomb
    r"(curl|wget|iwr|invoke-webrequest)[^|]*\|\s*(sh|bash|powershell|pwsh|iex|invoke-expression)\b",
    r"\b(iex|invoke-expression)\b[^|&;]*\b(downloadstring|invoke-webrequest|iwr)\b",
]

# First word of a terminal segment that is read-only. Only applies when the
# command has no redirections and every segment is read-only.
READ_ONLY_COMMANDS = {
    "ls", "dir", "pwd", "cd", "echo", "cat", "type", "head", "tail", "less", "more",
    "wc", "grep", "rg", "find", "findstr", "which", "where", "whoami", "hostname",
    "date", "uptime", "df", "du", "free", "ps", "tasklist", "systeminfo", "ver",
    "uname", "env", "printenv", "tree", "stat", "file", "sort", "uniq", "cut", "awk",
    "sed", "jq", "true", "false", "test", "basename", "dirname", "realpath",
    "get-childitem", "gci", "get-content", "gc", "get-process", "gps", "get-service",
    "get-date", "get-computerinfo", "get-psdrive", "get-location", "get-item",
    "test-path", "select-string", "measure-object", "select-object", "sort-object",
    "where-object", "format-table", "format-list", "out-string", "get-volume",
    "get-disk", "get-netipaddress", "get-nettcpconnection", "get-itemproperty",
    "reg", "nvidia-smi", "ollama", "git", "python", "python3", "node", "hermes",
}

# Sub-commands that keep a normally-mutating program read-only.
READ_ONLY_SUBCOMMANDS = {
    "git": {"status", "log", "diff", "show", "branch", "remote", "ls-files", "rev-parse", "describe", "blame", "tag"},
    "reg": {"query"},
    "ollama": {"list", "ps", "show", "--version", "-v"},
    "python": {"--version", "-v"},
    "python3": {"--version", "-v"},
    "node": {"--version", "-v"},
    "hermes": {"status", "version", "--version", "doctor", "prompt-size", "tools", "plugins", "skills"},
    "sed": set(),  # sed without -i is read-only; -i checked separately
}

# Per-tool levels. "roots" = allow inside the allowed roots, confirm outside.
DEFAULT_TOOLS: dict[str, str] = {
    "read_file": "allow",
    "search_files": "allow",
    "web_search": "allow",
    "web_extract": "allow",
    "vision_analyze": "allow",
    # Webcam frame for vision_analyze (hermes_camera): only on request, logged; camera light shows it.
    "camera_snapshot": "allow",
    "video_analyze": "allow",
    "browser_snapshot": "allow",
    "browser_navigate": "allow",
    "browser_scroll": "allow",
    "browser_back": "allow",
    "browser_get_images": "allow",
    "browser_console": "allow",
    "browser_vision": "allow",
    "browser_click": "allow",
    "browser_type": "allow",
    "browser_press": "allow",
    "browser_dialog": "allow",
    "browser_exec": "allow",
    "browser_cdp": "block",
    "browser_vault": "block",
    "write_file": "roots",
    "patch": "roots",
    "execute_code": "allow",
    "process_manage": "allow",
    "computer_use": "allow",
    "terminal": "terminal",
    "delegate_task": "allow",
    "todo": "allow",
    "memory": "allow",
    "skill_view": "allow",
    "skills_list": "allow",
    "skill_manage": "allow",
    "session_search": "allow",
    "clarify": "allow",
    # Hermes meta-tools: looking up tool descriptions changes nothing.
    "tool_search": "allow",
    "tool_describe": "allow",
    # MCP bridge to Claude Code on the server (tools/mcp-claude-bridge):
    # listing is harmless, handing over a task needs a human.
    "mcp_claude_claude_projects": "allow",
    "mcp_claude_claude_status": "allow",
    "mcp_claude_claude_task": "allow",
    "send_message": "allow",
    "schedule_cron": "allow",
    "cronjob": "allow",
}

# Paths that are never written, even inside the allowed roots.
PROTECTED_PATH_PARTS = [
    r"[\\/]\.ssh[\\/]",
    r"[\\/]\.gnupg[\\/]",
    r"[\\/]appdata[\\/](roaming|local)[\\/]microsoft[\\/](credentials|protect|vault)",
    r"[\\/]\.aws[\\/]",
    r"[\\/]\.azure[\\/]",
    r"[\\/]\.config[\\/]gh[\\/]",
    r"\.env$",
    r"(^|[\\/])(id_rsa|id_ed25519|id_ecdsa)(\.pub)?$",
]

DEFAULT_POLICY: dict[str, Any] = {
    "version": 1,
    # Write roots. Empty list = %USERPROFILE%, D:\ and HERMES_HOME (filled at load).
    "roots": [],
    "tools": DEFAULT_TOOLS,
    # Level for a tool not listed in "tools".
    "default": "allow",
    "terminal": {
        "block": BLOCKED_TERMINAL,
        # Extra read-only first words / extra blocked patterns from the user.
        "allow": [],
        "default": "allow",
    },
}


class Decision:
    __slots__ = ("action", "reason", "rule_key")

    def __init__(self, action: str, reason: str, rule_key: str | None = None):
        self.action = action  # allow | confirm | block
        self.reason = reason
        self.rule_key = rule_key

    def __repr__(self) -> str:  # pragma: no cover
        return f"Decision({self.action!r}, {self.reason!r}, {self.rule_key!r})"

    def __eq__(self, other: object) -> bool:
        return isinstance(other, Decision) and (self.action, self.reason, self.rule_key) == (
            other.action, other.reason, other.rule_key)


def hermes_home() -> Path:
    return Path(os.environ.get("HERMES_HOME") or (Path.home() / ".hermes"))


def default_roots(home: Path | None = None) -> list[str]:
    roots: list[str] = []
    profile = os.environ.get("USERPROFILE") or str(Path.home())
    roots.append(profile)
    if os.name == "nt" and Path("D:\\").exists():
        roots.append("D:\\")
    roots.append(str(home or hermes_home()))
    return roots


def load_policy(home: Path | None = None) -> dict[str, Any]:
    """Defaults merged with ``$HERMES_HOME/hermes_permissions.json`` (shallow per section)."""
    home = home or hermes_home()
    policy: dict[str, Any] = json.loads(json.dumps(DEFAULT_POLICY))  # deep copy
    override_path = home / POLICY_FILE
    if override_path.exists():
        try:
            override = json.loads(override_path.read_text(encoding="utf-8"))
        except Exception:  # noqa: BLE001 — a broken override must not disable the guard
            override = {}
        if isinstance(override, dict):
            if isinstance(override.get("roots"), list):
                policy["roots"] = [str(r) for r in override["roots"]]
            if isinstance(override.get("tools"), dict):
                policy["tools"].update({str(k): str(v) for k, v in override["tools"].items()})
            if isinstance(override.get("default"), str):
                policy["default"] = override["default"]
            term = override.get("terminal")
            if isinstance(term, dict):
                if isinstance(term.get("block"), list):
                    policy["terminal"]["block"] = policy["terminal"]["block"] + [str(p) for p in term["block"]]
                if isinstance(term.get("allow"), list):
                    policy["terminal"]["allow"] = [str(p) for p in term["allow"]]
                if isinstance(term.get("default"), str):
                    policy["terminal"]["default"] = term["default"]
    if not policy["roots"]:
        policy["roots"] = default_roots(home)
    return policy


# --------------------------------------------------------------- classify --

_SEGMENT_SPLIT = re.compile(r"\|\||&&|;|\|(?!\|)|\n")
_REDIRECT = re.compile(r"(^|[^0-9&])>+|<(?!<)")
_HARMLESS_REDIRECT = re.compile(r"2>\s*(&1|/dev/null|nul)", re.IGNORECASE)


def _norm_path(p: str) -> str:
    p = os.path.expanduser(os.path.expandvars(p.strip().strip('"').strip("'")))
    p = p.replace("/", "\\") if os.name == "nt" else p
    try:
        p = os.path.normpath(os.path.abspath(p))
    except Exception:  # noqa: BLE001
        pass
    return p.lower() if os.name == "nt" else p


def _inside(path: str, root: str) -> bool:
    path_n, root_n = _norm_path(path), _norm_path(root)
    if not root_n.endswith(os.sep):
        root_n += os.sep
    return path_n == root_n.rstrip(os.sep) or path_n.startswith(root_n)


def classify_path_write(path: str | None, policy: dict[str, Any], tool: str) -> Decision:
    if not path:
        return Decision("confirm", f"{tool}: путь не указан", f"hp:{tool}")
    raw = str(path)
    for pat in PROTECTED_PATH_PARTS:
        if re.search(pat, raw, re.IGNORECASE):
            return Decision("block", f"{tool}: запись в защищённый файл запрещена: {raw}")
    if not os.path.isabs(os.path.expanduser(raw)) and not re.match(r"^[a-zA-Z]:", raw):
        return Decision("allow", f"{tool}: относительный путь {raw}")
    for root in policy["roots"]:
        if _inside(raw, root):
            return Decision("allow", f"{tool} внутри {root}")
    # Outside the roots is still allowed (Artem: no prompts for ordinary work);
    # protected credential paths above are the hard line.
    return Decision("allow", f"{tool}: путь вне разрешённых папок: {raw}")


def _first_word(segment: str) -> tuple[str, list[str]]:
    words = segment.strip().split()
    if not words:
        return "", []
    head = words[0].lower()
    # Strip a leading path or sudo/env prefixes: `C:\...\git.exe`, `/usr/bin/ls`
    head = head.replace("\\", "/").rsplit("/", 1)[-1]
    if head.endswith(".exe"):
        head = head[:-4]
    if head in {"sudo", "env", "command", "builtin"} and len(words) > 1:
        return _first_word(" ".join(words[1:]))
    return head, [w.lower() for w in words[1:]]


def classify_terminal(command: str | None, policy: dict[str, Any]) -> Decision:
    cmd = (command or "").strip()
    if not cmd:
        return Decision("confirm", "terminal: пустая команда", "hp:terminal")
    term = policy["terminal"]
    for pat in term["block"]:
        try:
            if re.search(pat, cmd, re.IGNORECASE):
                return Decision("block", f"terminal: команда запрещена политикой ({pat})")
        except re.error:
            continue
    segments = [s for s in _SEGMENT_SPLIT.split(cmd) if s.strip()]
    extra_allow = {str(w).lower() for w in term.get("allow", [])}
    read_only = True
    for seg in segments:
        seg_no_harmless = _HARMLESS_REDIRECT.sub("", seg)
        if _REDIRECT.search(seg_no_harmless):
            read_only = False
            break
        head, rest = _first_word(seg)
        if head not in READ_ONLY_COMMANDS and head not in extra_allow:
            read_only = False
            break
        subs = READ_ONLY_SUBCOMMANDS.get(head)
        if subs is not None and head != "sed":
            if not rest or rest[0] not in subs:
                read_only = False
                break
        if head == "sed" and any(w in {"-i", "--in-place"} or w.startswith("-i") for w in rest):
            read_only = False
            break
        if head == "find" and any(w in {"-delete", "-exec", "-execdir", "-ok"} for w in rest):
            read_only = False
            break
    if read_only:
        return Decision("allow", "terminal: команда только на чтение")
    head, _ = _first_word(segments[0]) if segments else ("", [])
    level = term.get("default", "confirm")
    if level == "allow":
        return Decision("allow", "terminal: разрешено политикой")
    return Decision("confirm", f"terminal: команда меняет состояние ({head or 'cmd'})", f"hp:terminal:{head or 'cmd'}")


def classify(tool_name: str, args: dict[str, Any] | None, policy: dict[str, Any]) -> Decision:
    args = args or {}
    level = policy["tools"].get(tool_name, policy.get("default", "confirm"))
    if level == "terminal":
        return classify_terminal(args.get("command"), policy)
    if level == "roots":
        return classify_path_write(args.get("path"), policy, tool_name)
    if level == "allow":
        return Decision("allow", f"{tool_name}: разрешено политикой")
    if level == "block":
        return Decision("block", f"{tool_name}: запрещено политикой")
    return Decision("confirm", f"{tool_name}: требует подтверждения", f"hp:{tool_name}")


# ------------------------------------------------------------------ plugin --

def _preview(tool_name: str, args: dict[str, Any]) -> str:
    for key in ("command", "path", "code", "url", "message", "action"):
        if key in args and args[key] is not None:
            text = str(args[key]).replace("\n", " ")
            return text[:PREVIEW_CHARS]
    return ""


def _log(home: Path, record: dict[str, Any]) -> None:
    try:
        with (home / LOG_FILE).open("a", encoding="utf-8") as f:
            f.write(json.dumps(record, ensure_ascii=False) + "\n")
    except Exception:  # noqa: BLE001 — logging must never break the gate
        pass


def make_hook(home: Path | None = None):
    home = home or hermes_home()

    def pre_tool_call(tool_name: str = "", args: dict[str, Any] | None = None, task_id: str = "", **_: Any):
        args = args if isinstance(args, dict) else {}
        try:
            decision = classify(tool_name, args, load_policy(home))
        except Exception as exc:  # noqa: BLE001 — fail closed
            decision = Decision("confirm", f"hermes_permissions: внутренняя ошибка ({exc})", "hp:error")
        preview = _preview(tool_name, args)
        _log(home, {
            "ts": time.time(),
            "tool": tool_name,
            "action": decision.action,
            "reason": decision.reason,
            "preview": preview,
            "task_id": task_id or "",
        })
        # The approval card in Hermes Assistant shows this message: include
        # what exactly is about to run, not only the rule name.
        message = f"{decision.reason} — {preview}" if preview else decision.reason
        if decision.action == "block":
            return {"action": "block", "message": message}
        if decision.action == "confirm":
            out: dict[str, Any] = {"action": "approve", "message": message}
            if decision.rule_key:
                out["rule_key"] = decision.rule_key
            return out
        return None

    return pre_tool_call


def register(ctx) -> None:
    ctx.register_hook("pre_tool_call", make_hook())
