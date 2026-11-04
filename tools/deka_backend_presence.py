#!/usr/bin/env python3
"""Morning login / evening logout on backend.deka.ua for several accounts.

Usage:
    deka_backend_presence.py login  [--users a,b] [--env PATH] [--state-dir DIR] [--notify-success]
    deka_backend_presence.py logout [--users a,b] [--env PATH] [--state-dir DIR] [--notify-success]

Configuration (environment or Hermes .env):
    DEKA_BACKEND_USERS            comma-separated logins, e.g. "koav,ivanov"
    DEKA_BACKEND_PASSWORD_<LOGIN> password per login (login upper-cased, non-alnum -> "_")
    DEKA_BACKEND_USER / DEKA_BACKEND_PASSWORD   legacy single account (still honoured)
    NOTIFY_TELEGRAM_BOT_TOKEN, NOTIFY_TELEGRAM_CHAT_ID   failure alerts (optional)

The morning session cookie is kept per user in the state dir and reused for the
evening logout, so the backend sees one session opened in the morning and closed
in the evening. If the session has expired by evening, the script logs in again
and logs out immediately so a logout is still registered.
Stdlib only: works under Hermes' Python without extra packages.
"""
from __future__ import annotations

import argparse
import http.cookiejar
import json
import os
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime
from pathlib import Path

BASE = "https://backend.deka.ua"
LOGIN_URL = f"{BASE}/index.php?r=login"
MAIN_URL = f"{BASE}/index.php?r=main"
LOGOUT_URL = f"{BASE}/logout.php"
UA = ("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 "
      "(KHTML, like Gecko) Chrome/130.0 Safari/537.36")
TIMEOUT = 30
ERR_TEXT = re.compile(r"неправильн\w* имя пользователя|incorrect username", re.I)


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):  # noqa: D401
        return None


def load_env(path: Path | None) -> None:
    candidates = [path] if path else []
    home = os.environ.get("HERMES_HOME")
    if home:
        candidates.append(Path(home) / ".env")
    candidates.append(Path.home() / ".hermes" / ".env")
    for p in candidates:
        if p and p.is_file():
            for line in p.read_text(encoding="utf-8", errors="replace").splitlines():
                line = line.strip()
                if not line or line.startswith("#") or "=" not in line:
                    continue
                k, v = line.split("=", 1)
                k, v = k.strip(), v.strip().strip('"').strip("'")
                os.environ.setdefault(k, v)
            return


def env_key(login: str) -> str:
    return "DEKA_BACKEND_PASSWORD_" + re.sub(r"[^A-Za-z0-9]", "_", login).upper()


def accounts(cli_users: str | None) -> dict[str, str]:
    users = [u.strip() for u in (cli_users or os.environ.get("DEKA_BACKEND_USERS", "")).split(",") if u.strip()]
    legacy_user = os.environ.get("DEKA_BACKEND_USER")
    if legacy_user and legacy_user not in users:
        users.insert(0, legacy_user)
    result: dict[str, str] = {}
    for u in users:
        pw = os.environ.get(env_key(u))
        if not pw and u == legacy_user:
            pw = os.environ.get("DEKA_BACKEND_PASSWORD")
        if not pw:
            raise SystemExit(f"нет пароля для {u}: задайте {env_key(u)}")
        result[u] = pw
    if not result:
        raise SystemExit("нет учётных записей: задайте DEKA_BACKEND_USERS и пароли")
    return result


def opener(jar: http.cookiejar.CookieJar, follow: bool) -> urllib.request.OpenerDirector:
    handlers = [urllib.request.HTTPCookieProcessor(jar)]
    if not follow:
        handlers.append(NoRedirect())
    op = urllib.request.build_opener(*handlers)
    op.addheaders = [("User-Agent", UA), ("Accept-Language", "ru,uk;q=0.9,en;q=0.8")]
    return op


def fetch(op, url: str, data: dict | None = None) -> tuple[int, str, str]:
    body = urllib.parse.urlencode(data).encode() if data is not None else None
    try:
        with op.open(url, body, timeout=TIMEOUT) as r:
            return r.status, r.geturl(), r.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as e:
        return e.code, e.headers.get("Location", ""), ""


def is_logged_in(jar) -> bool:
    status, loc, _ = fetch(opener(jar, follow=False), MAIN_URL)
    return status == 200


def do_login(login: str, password: str, jar) -> str:
    op = opener(jar, follow=True)
    fetch(op, LOGIN_URL)
    _, _, html = fetch(op, LOGIN_URL, {
        "LoginForm[username]": login, "LoginForm[password]": password, "yt0": "Войти"})
    if ERR_TEXT.search(html):
        raise RuntimeError("неправильное имя пользователя или пароль")
    if not is_logged_in(jar):
        raise RuntimeError("после входа сессия не открылась (снова страница входа)")
    return "вход выполнен"


def do_logout(login: str, password: str, jar) -> str:
    note = ""
    if not is_logged_in(jar):
        do_login(login, password, jar)
        note = " (утренняя сессия истекла — вошёл заново и вышел)"
    fetch(opener(jar, follow=True), LOGOUT_URL)
    if is_logged_in(jar):
        raise RuntimeError("после logout.php сессия всё ещё открыта")
    return "выход выполнен" + note


def notify(text: str) -> None:
    token, chat = os.environ.get("NOTIFY_TELEGRAM_BOT_TOKEN"), os.environ.get("NOTIFY_TELEGRAM_CHAT_ID")
    if not token or not chat:
        return
    try:
        req = urllib.request.Request(
            f"https://api.telegram.org/bot{token}/sendMessage",
            data=json.dumps({"chat_id": chat, "text": text}).encode(),
            headers={"Content-Type": "application/json"})
        urllib.request.urlopen(req, timeout=TIMEOUT).read()
    except Exception as e:  # noqa: BLE001
        print(f"telegram: не отправлено ({e})", file=sys.stderr)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("action", choices=["login", "logout"])
    ap.add_argument("--users", help="логины через запятую (иначе DEKA_BACKEND_USERS)")
    ap.add_argument("--env", type=Path, help="путь к .env (иначе $HERMES_HOME/.env или ~/.hermes/.env)")
    ap.add_argument("--state-dir", type=Path, help="папка для cookie-файлов (иначе $HERMES_HOME/state/deka_backend)")
    ap.add_argument("--notify-success", action="store_true", help="слать в Telegram и успех, не только ошибки")
    a = ap.parse_args()

    load_env(a.env)
    try:
        accs = accounts(a.users)
    except SystemExit as e:
        print(f"backend.deka.ua: {e}", file=sys.stderr)
        return 2
    state = a.state_dir or Path(os.environ.get("HERMES_HOME", Path.home() / ".hermes")) / "state" / "deka_backend"
    state.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now().strftime("%d.%m.%Y %H:%M")

    lines, failed = [], []
    for login, pw in accs.items():
        jar_path = state / f"{re.sub(r'[^A-Za-z0-9._-]', '_', login)}.cookies"
        jar = http.cookiejar.MozillaCookieJar(str(jar_path))
        if jar_path.exists():
            try:
                jar.load(ignore_discard=True, ignore_expires=True)
            except Exception:  # noqa: BLE001
                jar_path.unlink(missing_ok=True)
        try:
            msg = do_login(login, pw, jar) if a.action == "login" else do_logout(login, pw, jar)
            if a.action == "login":
                jar.save(ignore_discard=True, ignore_expires=True)
                jar_path.chmod(0o600)
            else:
                jar_path.unlink(missing_ok=True)
            lines.append(f"{login}: {msg}")
        except Exception as e:  # noqa: BLE001
            failed.append(login)
            lines.append(f"{login}: ОШИБКА — {e}")

    title = "вход" if a.action == "login" else "выход"
    report = f"backend.deka.ua, {title} {stamp}\n" + "\n".join(lines)
    print(report)
    if failed or a.notify_success:
        notify(("⚠️ " if failed else "✅ ") + report)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
