#!/usr/bin/env python3
"""Morning arrival / evening departure on backend.deka.ua for several accounts.

"login"  — signs in; the backend records the first login of the day as arrival
           («Приход») in «Документы → Учёт рабочего времени».
"logout" — presses «Уход» on that page (POST workingDay/index/mark, type=outcome,
           personal password), then closes the session via logout.php.

Usage:
    deka_backend_presence.py login  [--users a,b] [--env PATH] [--state-dir DIR] [--random-delay SEC] [--notify-success]
    deka_backend_presence.py logout [--users a,b] [--env PATH] [--state-dir DIR] [--random-delay SEC] [--notify-success]

Configuration (environment or Hermes .env):
    DEKA_BACKEND_USERS            comma-separated logins, e.g. "koav,ivanov"
    DEKA_BACKEND_PASSWORD_<LOGIN> password per login (login upper-cased, non-alnum -> "_")
    DEKA_BACKEND_SITE_<LOGIN>     base URL for that login (default DEKA_BACKEND_SITE or https://backend.deka.ua)
    DEKA_BACKEND_USER / DEKA_BACKEND_PASSWORD   legacy single account (still honoured)
    NOTIFY_TELEGRAM_BOT_TOKEN, NOTIFY_TELEGRAM_CHAT_ID   failure alerts (optional)

The morning session cookie is kept per user in the state dir and reused in the
evening; if it has expired, the script signs in again before pressing «Уход».
Stdlib only: works under Hermes' Python without extra packages.
"""
from __future__ import annotations

import argparse
import http.cookiejar
import json
import os
import random
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime
from pathlib import Path

DEFAULT_BASE = "https://backend.deka.ua"


class Site:
    """Same Yii application is deployed per company; the base URL differs per account."""

    def __init__(self, base: str):
        self.base = base.rstrip("/")
        self.host = urllib.parse.urlparse(self.base).hostname or self.base
        self.login = f"{self.base}/index.php?r=login"
        self.main = f"{self.base}/index.php?r=main"
        self.logout = f"{self.base}/logout.php"
        self.workday = f"{self.base}/index.php?r=workingDay/index"
        self.mark = f"{self.base}/index.php?r=workingDay/index/mark"
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


def env_key(login: str, prefix: str = "DEKA_BACKEND_PASSWORD_") -> str:
    return prefix + re.sub(r"[^A-Za-z0-9]", "_", login).upper()


def site_for(login: str) -> Site:
    return Site(os.environ.get(env_key(login, "DEKA_BACKEND_SITE_"))
                or os.environ.get("DEKA_BACKEND_SITE") or DEFAULT_BASE)


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


def is_logged_in(site: Site, jar) -> bool:
    status, loc, _ = fetch(opener(jar, follow=False), site.main)
    return status == 200


def workday_state(site: Site, jar) -> tuple[str | None, str | None, str | None]:
    """Return (seller_id, arrival, departure) from the self-service block."""
    _, _, html = fetch(opener(jar, follow=True), site.workday)
    m = re.search(r'name="user" value="(\d+)"', html)
    arr = re.search(r"Приход:\s*([0-9:]+)", html)
    dep = re.search(r"Уход:\s*([0-9:]+)", html)
    return (m.group(1) if m else None, arr.group(1) if arr else None, dep.group(1) if dep else None)


def do_login(site: Site, login: str, password: str, jar) -> str:
    op = opener(jar, follow=True)
    fetch(op, site.login)
    _, _, html = fetch(op, site.login, {
        "LoginForm[username]": login, "LoginForm[password]": password, "yt0": "Войти"})
    if ERR_TEXT.search(html):
        raise RuntimeError("неправильное имя пользователя или пароль")
    if not is_logged_in(site, jar):
        raise RuntimeError("после входа сессия не открылась (снова страница входа)")
    _, arrival, _ = workday_state(site, jar)
    return f"вход выполнен, приход {arrival}" if arrival else "вход выполнен (приход на странице не показан)"


def do_logout(site: Site, login: str, password: str, jar) -> str:
    note = ""
    if not is_logged_in(site, jar):
        do_login(site, login, password, jar)
        note = " (утренняя сессия истекла — вошёл заново)"
    seller_id, arrival, departure = workday_state(site, jar)
    if not seller_id:
        raise RuntimeError("не нашёл id сотрудника на странице учёта рабочего времени")
    if departure:
        fetch(opener(jar, follow=True), site.logout)
        return f"уход уже отмечен в {departure}, повторно не нажимал" + note
    req = urllib.request.Request(
        site.mark,
        data=urllib.parse.urlencode({"user": seller_id, "type": "outcome", "passwd": password}).encode(),
        headers={"X-Requested-With": "XMLHttpRequest", "Content-Type": "application/x-www-form-urlencoded; charset=UTF-8"})
    try:
        with opener(jar, follow=True).open(req, timeout=TIMEOUT) as r:
            reply = r.read().decode("utf-8", "replace").strip()
    except urllib.error.HTTPError as e:
        raise RuntimeError(f"«Уход» отклонён: HTTP {e.code} {e.read().decode('utf-8', 'replace').strip()[:200]}") from None
    _, _, departure = workday_state(site, jar)
    if not departure:
        raise RuntimeError(f"после «Уход» время ухода не появилось (ответ: {reply[:200]})")
    fetch(opener(jar, follow=True), site.logout)
    return f"уход отмечен в {departure} (приход {arrival}), сессия закрыта" + note


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
    ap.add_argument("--random-delay", type=int, default=0, metavar="SEC",
                    help="подождать случайное время 0..SEC секунд перед началом (чтобы время отметки не было одинаковым)")
    ap.add_argument("--notify-success", action="store_true", help="слать в Telegram и успех, не только ошибки")
    a = ap.parse_args()
    if a.random_delay > 0:
        time.sleep(random.randint(0, a.random_delay))

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
        site = site_for(login)
        try:
            msg = do_login(site, login, pw, jar) if a.action == "login" else do_logout(site, login, pw, jar)
            if a.action == "login":
                jar.save(ignore_discard=True, ignore_expires=True)
                jar_path.chmod(0o600)
            else:
                jar_path.unlink(missing_ok=True)
            lines.append(f"{login} @ {site.host}: {msg}")
        except Exception as e:  # noqa: BLE001
            failed.append(login)
            lines.append(f"{login} @ {site.host}: ОШИБКА — {e}")

    title = "вход (приход)" if a.action == "login" else "уход"
    report = f"Учёт рабочего времени, {title} {stamp}\n" + "\n".join(lines)
    print(report)
    if failed or a.notify_success:
        notify(("⚠️ " if failed else "✅ ") + report)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
