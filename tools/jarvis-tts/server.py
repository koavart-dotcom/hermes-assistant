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
import subprocess
import sys
import time
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


def reference_clips() -> list[str]:
    wavs = sorted(VOICE_DIR.rglob("*.wav"), key=lambda p: p.stat().st_size, reverse=True)
    if not wavs:
        raise SystemExit(f"no reference clips in {VOICE_DIR}")
    return [str(p) for p in wavs[:REF_CLIPS]]


def serve() -> None:
    import torch
    from TTS.api import TTS

    device = "cuda" if torch.cuda.is_available() else "cpu"
    t0 = time.time()
    tts = TTS("tts_models/multilingual/multi-dataset/xtts_v2").to(device)
    model = tts.synthesizer.tts_model
    latents, speaker = model.get_conditioning_latents(audio_path=reference_clips())
    print(f"ready on {device} in {time.time() - t0:.1f}s", flush=True)

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_a):  # quiet
            pass

        def do_GET(self):
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b'{"ok": true}')

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))) or b"{}")
            text = (body.get("text") or "").strip()
            out = body.get("out")
            if not text or not out:
                self.send_response(400)
                self.end_headers()
                return
            t = time.time()
            wav = model.inference(text, body.get("language", "ru"), latents, speaker, temperature=0.65,
                                  enable_text_splitting=True)["wav"]
            import soundfile as sf
            sf.write(out, wav, 24000)
            print(f"{len(text)} chars in {time.time() - t:.2f}s", flush=True)
            self.send_response(200)
            self.end_headers()
            self.wfile.write(json.dumps({"ok": True, "out": out}).encode())

    ThreadingHTTPServer((HOST, PORT), Handler).serve_forever()


def alive() -> bool:
    try:
        urllib.request.urlopen(f"http://{HOST}:{PORT}/", timeout=1)
        return True
    except Exception:
        return False


def ensure_server(wait_s: int = 180) -> None:
    if alive():
        return
    # DETACHED_PROCESS | NEW_PROCESS_GROUP, plus CREATE_BREAKAWAY_FROM_JOB: Hermes runs command
    # providers inside a job object that is torn down after each phrase; the server must outlive it,
    # otherwise every phrase would reload the model (~80 s).
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
    ensure_server()
    req = urllib.request.Request(f"http://{HOST}:{PORT}/", data=json.dumps({"text": text, "out": out}).encode(),
                                 headers={"Content-Type": "application/json"})
    urllib.request.urlopen(req, timeout=300).read()


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("serve")
    s = sub.add_parser("say")
    s.add_argument("--text-file", required=True)
    s.add_argument("--out", required=True)
    a = ap.parse_args()
    serve() if a.cmd == "serve" else say(a.text_file, a.out)
