#!/usr/bin/env python3
"""Shell-hook Hermes (post_llm_call): озвучить ответ агента на Mac (динамики = HDMI-плазма).

Вход: JSON в stdin (поле assistant_response / response / response_text, platform).
Выход: ничего (observer). Речь запускается отдельным процессом, хук возвращается сразу.
Выключить голос: создать файл ~/.hermes/agent-hooks/speak.off (удалить — включить).
Движок: Piper (ru_RU-dmitri-medium из venv Hermes) → afplay; если Piper недоступен — macOS `say -v Milena`.
"""
import glob, json, os, re, subprocess, sys, tempfile

HOME = os.path.expanduser("~")
OFF_FILE = f"{HOME}/.hermes/agent-hooks/speak.off"
VOICE = f"{HOME}/.hermes/cache/piper-voices/ru_RU-dmitri-medium.onnx"
MAX_CHARS = 700                      # не зачитывать простыни: первые ~700 символов до конца предложения
PLATFORMS = {"telegram", "cli", "tui", "desktop", "voice", ""}  # cron и прочие фоновые — молча


def clean(text: str) -> str:
    t = re.sub(r"```.*?```", " (код пропущен) ", text, flags=re.S)
    t = re.sub(r"`([^`]*)`", r"\1", t)
    t = re.sub(r"https?://\S+", " ссылка ", t)
    t = re.sub(r"^\s{0,3}#{1,6}\s*", "", t, flags=re.M)          # заголовки
    t = re.sub(r"^\s*[-*•]\s+|^\s*\d+\.\s+", "", t, flags=re.M)   # маркеры списков
    t = re.sub(r"\|", " ", t)                                      # таблицы
    t = re.sub(r"[*_~>#]+", "", t)
    t = re.sub(r"[\U0001F300-\U0001FAFF☀-➿⬀-⯿️]", "", t)  # эмодзи
    t = re.sub(r"[ \t]+", " ", t)
    t = re.sub(r"\n\s*\n+", "\n", t).strip()
    if len(t) > MAX_CHARS:
        cut = t[:MAX_CHARS]
        m = max(cut.rfind(". "), cut.rfind("! "), cut.rfind("? "), cut.rfind("\n"))
        t = (cut[: m + 1] if m > MAX_CHARS // 2 else cut) + " Остальное — в сообщении."
    return t


def piper_python():
    cands = sorted(glob.glob(f"{HOME}/.hermes/installs/*/environments/*/venv/bin/python"),
                   key=os.path.getmtime, reverse=True)
    for py in cands:
        if os.path.exists(os.path.join(os.path.dirname(py), "..", "lib")) and \
           subprocess.run([py, "-c", "import piper"], capture_output=True, timeout=20).returncode == 0:
            return py
    return None


def speak_detached(text: str):
    py = piper_python() if os.path.exists(VOICE) else None
    if py:
        wav = tempfile.NamedTemporaryFile(prefix="hermes-speak-", suffix=".wav", delete=False).name
        code = (
            "import sys, wave; from piper import PiperVoice\n"
            f"v = PiperVoice.load({VOICE!r})\n"
            f"text = sys.stdin.read()\n"
            f"with wave.open({wav!r}, 'wb') as w: v.synthesize_wav(text, w)\n"
        )
        cmd = f"{py!s} -c {json.dumps(code)} <<'HERMES_TXT' && afplay {wav} ; rm -f {wav}\n{text}\nHERMES_TXT"
        cmd = cmd.replace(json.dumps(code), "'" + code.replace("'", "'\"'\"'") + "'")
    else:
        cmd = f"say -v Milena -r 190 <<'HERMES_TXT'\n{text}\nHERMES_TXT"
    subprocess.Popen(["/bin/bash", "-c", cmd], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                     stderr=open(f"{HOME}/.hermes/logs/speak.err.log", "a"), start_new_session=True)


def log(msg: str):
    try:
        with open(f"{HOME}/.hermes/logs/speak.log", "a", encoding="utf-8") as f:
            f.write(msg + "\n")
    except Exception:
        pass


def main():
    try:
        payload = json.load(sys.stdin)
    except Exception as e:
        log(f"bad payload: {e}")
        return
    extra = payload.get("extra") if isinstance(payload.get("extra"), dict) else {}
    payload = {**payload, **extra}          # Hermes кладёт поля события (assistant_response, platform…) в extra
    plat = str(payload.get("platform") or "").lower()
    log(f"event={payload.get('hook_event_name')} platform={plat!r} keys={sorted(payload.keys())}")
    if os.path.exists(OFF_FILE):
        log("muted (speak.off)")
        return
    if plat not in PLATFORMS:
        log(f"skip platform {plat!r}")
        return
    text = payload.get("assistant_response") or payload.get("response_text") or payload.get("response") or ""
    text = clean(str(text))
    if len(text) < 2:
        log("skip: empty text")
        return
    log(f"speak: {text[:80]!r}")
    speak_detached(text)


if __name__ == "__main__":
    main()
    print("{}")  # observer: пустой JSON-ответ
