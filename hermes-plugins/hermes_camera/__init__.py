"""hermes_camera — the PC webcam and screen as Hermes' eyes.

Installed by Hermes Assistant into ``$HERMES_HOME/plugins/hermes_camera``.
Registers the ``camera_snapshot`` tool (toolset ``camera``): one frame from the
webcam through DirectShow, using the ffmpeg that Hermes ships in
``$HERMES_HOME/tools/ffmpeg-*``. The tool returns the JPEG path; the model then
looks at it with ``vision_analyze`` (local paths are accepted).

* Frames stay on the PC in ``%LOCALAPPDATA%\\ua.koavart.hermes\\camera``; only the
  last ``KEEP`` are kept.
* ``HERMES_CAMERA=off`` in Hermes' ``.env`` disables the camera.
* ``HERMES_CAMERA_DEVICE`` picks a DirectShow device by (part of) its name;
  default — the first video device.

``screen_snapshot`` (same toolset) grabs the whole desktop or the active window
with ffmpeg ``gdigrab`` into the same folder, also for ``vision_analyze``.

The gateway runs in the interactive session of the PC user, so the camera and
the desktop are reachable; the webcam's own light shows when it is used.

Standard library only (the plugin runs inside the Hermes gateway process).
"""

from __future__ import annotations

import glob
import json
import os
import re
import shutil
import subprocess
import time
from pathlib import Path
from typing import Any

__all__ = ["register", "snapshot", "screen", "screen_args", "parse_devices", "pick_device"]

TOOL = "camera_snapshot"
SCREEN_TOOL = "screen_snapshot"
# Our own window: «what is on my screen» typed into the chat means the screen, not the chat.
OWN_WINDOWS = ("Hermes Assistant", "Hermes —", "Джарвис")
TOOLSET = "camera"
KEEP = 10
TIMEOUT_S = 25
# Webcams start dark (auto exposure): skip the first second of video.
WARMUP_S = "1.2"

SCHEMA = {
    "name": TOOL,
    "description": (
        "Take one photo with the webcam of Artem's PC (it looks at Artem's desk). "
        "Use it only when he asks you to look («посмотри», «что ты видишь», «как я "
        "выгляжу», «что у меня в руках», «прочитай, что я показываю»). Returns the "
        "file path; then call vision_analyze with image_url = that path and answer "
        "from what you see. Never take photos on your own initiative."
    ),
    "parameters": {
        "type": "object",
        "properties": {
            "reason": {
                "type": "string",
                "description": "Why the photo is taken, in a few words (shown in the log).",
            }
        },
        "required": [],
    },
}


SCREEN_SCHEMA = {
    "name": SCREEN_TOOL,
    "description": (
        "Take a screenshot of Artem's PC: the whole desktop (all monitors) or the active window. "
        "Use it when he asks about his screen («что у меня на экране», «прочитай ошибку в этом "
        "окне», «что в этой таблице», «посмотри на экран»). Returns the file path; then call "
        "vision_analyze with image_url = that path (use region to zoom into small text). "
        "Never take screenshots on your own initiative; do not repeat passwords or keys you see."
    ),
    "parameters": {
        "type": "object",
        "properties": {
            "target": {
                "type": "string",
                "enum": ["screen", "window"],
                "description": "screen — every monitor; window — only the active window (default screen).",
            },
            "reason": {"type": "string", "description": "Why, in a few words (shown in the log)."},
        },
        "required": [],
    },
}


def hermes_home() -> Path:
    return Path(os.environ.get("HERMES_HOME") or Path.home() / ".hermes")


def frames_dir() -> Path:
    local = os.environ.get("LOCALAPPDATA")
    base = Path(local) / "ua.koavart.hermes" if local else hermes_home()
    return base / "camera"


def find_ffmpeg(home: Path | None = None) -> str | None:
    home = home or hermes_home()
    bundled = sorted(glob.glob(str(home / "tools" / "ffmpeg-*" / "bin" / "ffmpeg.exe")))
    if bundled:
        return bundled[-1]
    return shutil.which("ffmpeg")


_DEVICE_LINE = re.compile(r'"([^"]+)"\s+\((video|audio)\)')


def parse_devices(ffmpeg_stderr: str) -> list[str]:
    """Video device names from ``ffmpeg -list_devices true -f dshow -i dummy``."""
    out: list[str] = []
    for line in ffmpeg_stderr.splitlines():
        m = _DEVICE_LINE.search(line)
        if m and m.group(2) == "video" and m.group(1) not in out:
            out.append(m.group(1))
    return out


def pick_device(devices: list[str], wanted: str | None) -> str | None:
    if not devices:
        return None
    if wanted:
        w = wanted.strip().lower()
        for d in devices:
            if w in d.lower():
                return d
        return None
    return devices[0]


def _run(args: list[str], timeout: int) -> subprocess.CompletedProcess:
    flags = 0x08000000 if os.name == "nt" else 0  # CREATE_NO_WINDOW
    return subprocess.run(args, capture_output=True, timeout=timeout, creationflags=flags)


def _prune(folder: Path, keep: int = KEEP, pattern: str = "cam-*.jpg") -> None:
    shots = sorted(folder.glob(pattern), key=lambda p: p.stat().st_mtime, reverse=True)
    for old in shots[keep:]:
        try:
            old.unlink()
        except OSError:
            pass


def snapshot(reason: str = "") -> dict[str, Any]:
    if os.environ.get("HERMES_CAMERA", "").strip().lower() in ("off", "0", "false", "no"):
        return {"ok": False, "error": "Камера отключена (HERMES_CAMERA=off)."}
    if os.name != "nt":
        return {"ok": False, "error": "Камера доступна только на Windows-ПК."}
    ffmpeg = find_ffmpeg()
    if not ffmpeg:
        return {"ok": False, "error": "Не найден ffmpeg (ожидался в HERMES_HOME/tools/ffmpeg-*)."}
    listing = _run([ffmpeg, "-hide_banner", "-list_devices", "true", "-f", "dshow", "-i", "dummy"], 15)
    devices = parse_devices(listing.stderr.decode("utf-8", "replace"))
    device = pick_device(devices, os.environ.get("HERMES_CAMERA_DEVICE"))
    if not device:
        return {"ok": False, "error": "Камера не найдена.", "devices": devices}
    folder = frames_dir()
    folder.mkdir(parents=True, exist_ok=True)
    path = folder / time.strftime("cam-%Y%m%d-%H%M%S.jpg")
    args = [ffmpeg, "-hide_banner", "-loglevel", "error", "-f", "dshow", "-rtbufsize", "64M",
            "-i", f"video={device}", "-ss", WARMUP_S, "-frames:v", "1", "-vf", "scale=1280:-2",
            "-q:v", "3", "-y", str(path)]
    try:
        r = _run(args, TIMEOUT_S)
    except subprocess.TimeoutExpired:
        return {"ok": False, "error": "Камера не ответила за 25 с (занята другой программой?)."}
    if r.returncode != 0 or not path.is_file() or path.stat().st_size == 0:
        tail = r.stderr.decode("utf-8", "replace").strip().splitlines()[-2:]
        return {"ok": False, "error": "Не удалось снять кадр: " + " ".join(tail)[:300],
                "device": device}
    _prune(folder)
    return {
        "ok": True,
        "path": str(path),
        "device": device,
        "reason": reason,
        "next": f"Call vision_analyze with image_url={path} to see the photo.",
    }


def active_window_title() -> str:
    """Title of the foreground window ('' off Windows or when it has none)."""
    if os.name != "nt":
        return ""
    import ctypes

    user32 = ctypes.windll.user32
    hwnd = user32.GetForegroundWindow()
    if not hwnd:
        return ""
    buf = ctypes.create_unicode_buffer(512)
    user32.GetWindowTextW(hwnd, buf, 512)
    return buf.value.strip()


def screen_args(ffmpeg: str, out: Path, target: str, title: str) -> tuple[list[str], str]:
    """ffmpeg command and what is captured; our own window or no title → the whole desktop."""
    window = target == "window" and title and not any(title.startswith(w) for w in OWN_WINDOWS)
    source = f"title={title}" if window else "desktop"
    args = [ffmpeg, "-hide_banner", "-loglevel", "error", "-f", "gdigrab", "-framerate", "1",
            "-i", source, "-frames:v", "1", "-vf", "scale='min(2560,iw)':-2", "-y", str(out)]
    return args, (f"окно «{title}»" if window else "весь экран")


def screen(target: str = "screen", reason: str = "") -> dict[str, Any]:
    if os.name != "nt":
        return {"ok": False, "error": "Снимок экрана доступен только на Windows-ПК."}
    ffmpeg = find_ffmpeg()
    if not ffmpeg:
        return {"ok": False, "error": "Не найден ffmpeg (ожидался в HERMES_HOME/tools/ffmpeg-*)."}
    folder = frames_dir()
    folder.mkdir(parents=True, exist_ok=True)
    out = folder / time.strftime("screen-%Y%m%d-%H%M%S.png")
    args, what = screen_args(ffmpeg, out, target, active_window_title())
    try:
        r = _run(args, TIMEOUT_S)
    except subprocess.TimeoutExpired:
        return {"ok": False, "error": "Снимок экрана не получился за 25 с."}
    if r.returncode != 0 or not out.is_file():
        tail = r.stderr.decode("utf-8", "replace").strip().splitlines()[-2:]
        return {"ok": False, "error": "Не удалось снять экран: " + " ".join(tail)[:300]}
    _prune(folder, pattern="screen-*.png")
    return {"ok": True, "path": str(out), "captured": what, "reason": reason,
            "next": f"Call vision_analyze with image_url={out} to see it."}


def _handle_screen(args: dict[str, Any] | None = None, **_: Any) -> str:
    args = args or {}
    try:
        result = screen(str(args.get("target") or "screen"), str(args.get("reason") or ""))
    except Exception as exc:  # never crash the gateway
        result = {"ok": False, "error": f"screen_snapshot: {exc}"}
    return json.dumps(result, ensure_ascii=False)


def _handle(args: dict[str, Any] | None = None, **_: Any) -> str:
    args = args or {}
    try:
        result = snapshot(str(args.get("reason") or ""))
    except Exception as exc:  # never crash the gateway
        result = {"ok": False, "error": f"camera_snapshot: {exc}"}
    return json.dumps(result, ensure_ascii=False)


def register(ctx) -> None:
    ctx.register_tool(
        name=TOOL,
        toolset=TOOLSET,
        schema=SCHEMA,
        handler=_handle,
        description="Photo from the PC webcam for vision_analyze",
        emoji="📷",
    )
    ctx.register_tool(
        name=SCREEN_TOOL,
        toolset=TOOLSET,
        schema=SCREEN_SCHEMA,
        handler=_handle_screen,
        description="Screenshot of the PC (desktop or active window) for vision_analyze",
        emoji="🖥️",
    )
