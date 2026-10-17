"""Jarvis voice for Hermes: XTTS v2 voice clone served locally.

Run once (keeps the model in VRAM):   python server.py serve
Hermes "command" TTS provider calls:  python server.py say --text-file IN.txt --out OUT.wav

`say` starts the server in the background when it is not running and waits for
it, so Hermes needs no extra supervision. Reference clips: the Jarvis voice
pack imported into Hermes Assistant (`voice-pack/Jarvis`), the longest N clips.

Binds 127.0.0.1 only. Model licence: Coqui Public Model License (personal,
non-commercial use).
"""

from __future__ import annotations

import argparse
import json
import os
import socket
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

HOST, PORT = "127.0.0.1", 8655
HERE = Path(__file__).resolve().parent
# Model files next to the server (D:), licence accepted once at install.
os.environ.setdefault("TTS_HOME", str(HERE / "models"))
os.environ.setdefault("COQUI_TOS_AGREED", "1")
LOG = HERE / "server.log"
VOICE_DIR = Path(os.environ.get("LOCALAPPDATA", "")) / "ua.koavart.hermes" / "voice-pack" / "Jarvis"
REF_CLIPS = 8
# Unload after this many quiet minutes (frees ~6-7 GB RAM and ~3 GB VRAM); 0 keeps the model loaded.
# Hermes Assistant wakes the server again when a voice question goes to the model.
IDLE_MIN = float(os.environ.get("JARVIS_TTS_IDLE_MIN", "30"))


def reference_clips() -> list[str]:
    wavs = sorted(VOICE_DIR.rglob("*.wav"), key=lambda p: p.stat().st_size, reverse=True)
    if not wavs:
        raise SystemExit(f"no reference clips in {VOICE_DIR}")
    return [str(p) for p in wavs[:REF_CLIPS]]


class _Server(ThreadingHTTPServer):
    # Exclusive port: on Windows SO_REUSEADDR lets a second server bind the same port, so two copies
    # would each load XTTS (~6-7 GB RAM) — on 17.10.2026 that ran the PC out of commit memory.
    allow_reuse_address = False

    def server_bind(self):
        if hasattr(socket, "SO_EXCLUSIVEADDRUSE"):
            self.socket.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
        super().server_bind()


def serve() -> None:
    # Bind first, load later: the port is the single-instance lock, and callers see "loading" (503)
    # instead of a closed port and do not start another copy.
    ready = threading.Event()
    state: dict = {}

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_a):  # quiet
            pass

        def do_GET(self):
            self.send_response(200 if ready.is_set() else 503)
            self.end_headers()
            self.wfile.write(b'{"ok": true}' if ready.is_set() else b'{"ok": false, "loading": true}')

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))) or b"{}")
            text = (body.get("text") or "").strip()
            out = body.get("out")
            if not text or not out or not ready.wait(300):
                self.send_response(400 if not text or not out else 503)
                self.end_headers()
                return
            t = time.time()
            state["last"] = t
            with state["lock"]:
                wav = state["model"].inference(text, body.get("language", "ru"), state["latents"],
                                               state["speaker"], temperature=0.65,
                                               enable_text_splitting=True)["wav"]
            import soundfile as sf
            sf.write(out, wav, 24000)
            state["last"] = time.time()
            print(f"{len(text)} chars in {time.time() - t:.2f}s", flush=True)
            self.send_response(200)
            self.end_headers()
            self.wfile.write(json.dumps({"ok": True, "out": out}).encode())

    try:
        httpd = _Server((HOST, PORT), Handler)
    except OSError:
        print("another jarvis-tts server owns the port, exiting", flush=True)
        return
    threading.Thread(target=httpd.serve_forever, daemon=True).start()

    import torch
    from TTS.api import TTS

    device = "cuda" if torch.cuda.is_available() else "cpu"
    t0 = time.time()
    tts = TTS("tts_models/multilingual/multi-dataset/xtts_v2").to(device)
    model = tts.synthesizer.tts_model
    latents, speaker = model.get_conditioning_latents(audio_path=reference_clips())
    state.update(model=model, latents=latents, speaker=speaker, lock=threading.Lock(), last=time.time())
    ready.set()
    print(f"ready on {device} in {time.time() - t0:.1f}s", flush=True)
    while True:
        time.sleep(30)
        if IDLE_MIN > 0 and time.time() - state["last"] > IDLE_MIN * 60 and state["lock"].acquire(blocking=False):
            print(f"idle for {IDLE_MIN:g} min, unloading", flush=True)
            httpd.server_close()
            os._exit(0)


def port_open() -> bool:
    try:
        with socket.create_connection((HOST, PORT), timeout=1):
            return True
    except OSError:
        return False


def alive() -> bool:
    try:
        urllib.request.urlopen(f"http://{HOST}:{PORT}/", timeout=1)
        return True
    except Exception:
        return False


def ensure_server(wait_s: int = 180) -> None:
    if alive():
        return
    if not port_open():
        # DETACHED_PROCESS | NEW_PROCESS_GROUP, plus CREATE_BREAKAWAY_FROM_JOB: Hermes runs command
        # providers inside a job object that is torn down after each phrase; the server must outlive
        # it, otherwise every phrase would reload the model (~80 s).
        base = 0x00000008 | 0x00000200 if os.name == "nt" else 0
        with open(LOG, "ab") as log:
            for flags in ((base | 0x01000000, base) if os.name == "nt" else (0,)):
                try:
                    subprocess.Popen([sys.executable, str(Path(__file__)), "serve"], stdout=log, stderr=log,
                                     stdin=subprocess.DEVNULL, creationflags=flags, close_fds=True)
                    break
                except OSError:
                    continue  # breakaway not allowed by the job: start inside it
    deadline = time.time() + wait_s
    while time.time() < deadline:
        if alive():
            return
        time.sleep(1)
    raise SystemExit("jarvis-tts server did not start, see server.log")


def say(text_file: str, out: str) -> None:
    text = Path(text_file).read_text(encoding="utf-8").strip()
    for attempt in range(2):
        ensure_server()
        req = urllib.request.Request(f"http://{HOST}:{PORT}/", data=json.dumps({"text": text, "out": out}).encode(),
                                     headers={"Content-Type": "application/json"})
        try:
            urllib.request.urlopen(req, timeout=300).read()
            return
        except urllib.error.URLError:
            if attempt:  # the server unloaded itself between the check and the request: start it again
                raise


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("serve")
    s = sub.add_parser("say")
    s.add_argument("--text-file", required=True)
    s.add_argument("--out", required=True)
    a = ap.parse_args()
    serve() if a.cmd == "serve" else say(a.text_file, a.out)
