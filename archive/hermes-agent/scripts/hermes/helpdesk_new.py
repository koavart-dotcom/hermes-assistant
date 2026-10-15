#!/usr/bin/env python3
"""Источник для cron-сводки Hermes: заявки FreeScout (L0, только чтение).

Режим по умолчанию (cron, 3 раза в день) — один ящик, support@deka.ua (IT Support, mailboxId 1):
  - "sidebar": счётчики ровно как в боковой панели FreeScout у Артема
    (Неназначенные / Мои / Черновики / Назначенные / Спам / DBA и другие папки команд) — из API папок ящика;
  - "tickets": заявки без исполнителя (папка «Неназначенные», active+pending) и назначенные на Артема
    (FREESCOUT_MY_USER_ID, по умолчанию 4 — Artem Kovalenko); у каждой пометка новая / обновлена / в работе.
  Удалённые (корзина, state=deleted) исключены — API их возвращает вместе с живыми, из-за чего раньше
  счётчик «без исполнителя» был 31 вместо 1 (14.10.2026).
Другие ящики — только по команде: --mailbox 2|3|5 | --mailbox all.  --all — старый общий режим.
Ключ и адрес — из окружения или ~/.hermes/.env (FREESCOUT_URL, FREESCOUT_API_KEY).
Вывод стабилен по порядку (сортировка по id).
"""
import json, os, sys, re, html, urllib.request, urllib.parse
from datetime import datetime, timedelta, timezone

WINDOW_HOURS = 5          # «новые/обновлённые» (сканы в 9, 14, 17 — окно покрывает интервал между ними)
BACKLOG_DAYS = 14         # (режим --all) без исполнителя показываем только тронутые за этот срок
MAILBOXES = {1: "IT Support", 2: "Developers", 3: "Техотдел", 5: "Маркетинг"}
MY_USER_ID = int(os.environ.get("FREESCOUT_MY_USER_ID", "4"))   # Artem Kovalenko (koav@deka.ua)
DEFAULT_MAILBOX = 1       # support@deka.ua (IT Support) — единственный ящик для cron
# Типы папок FreeScout: 1 неназначенные, 20 мои (userId), 25 отмеченные, 30 черновики, 40 назначенные,
# 60 закрытые, 70 удалённые, 80 спам, 185 папка команды (userId = id команды), 200 — пользовательские.
SIDEBAR_TYPES = {1: "Неназначенные", 20: "Мои", 30: "Черновики", 40: "Назначенные", 80: "Спам", 185: None}


def load_env():
    p = os.path.expanduser("~/.hermes/.env")
    if os.path.exists(p):
        for line in open(p, encoding="utf-8"):
            if "=" in line and not line.lstrip().startswith("#"):
                k, v = line.rstrip("\n").split("=", 1)
                os.environ.setdefault(k.strip(), v.strip().strip('"').strip("'"))


def base_url():
    return os.environ.get("FREESCOUT_URL", "https://helpdesk.deka.ua").rstrip("/")


def get(path, **params):
    key = os.environ["FREESCOUT_API_KEY"]
    url = f"{base_url()}/api/{path}"
    if params:
        url += "?" + urllib.parse.urlencode({k: v for k, v in params.items() if v is not None})
    req = urllib.request.Request(url, headers={"X-FreeScout-API-Key": key, "Accept": "application/json"})
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.load(r)


def conversations(**params):
    """Список заявок без удалённых (корзина)."""
    d = get("conversations", pageSize=100, sortField="updatedAt", sortOrder="desc", embed="threads,tags", **params)
    return [c for c in (d.get("_embedded") or {}).get("conversations", []) if c.get("state", "published") == "published"]


def strip_html(s, limit=280):
    s = re.sub(r"<style.*?</style>|<script.*?</script>", " ", s or "", flags=re.S | re.I)
    s = re.sub(r"<br\s*/?>|</p>|</div>", "\n", s, flags=re.I)
    s = html.unescape(re.sub(r"<[^>]+>", " ", s))
    s = re.sub(r"[ \t]+", " ", s)
    s = re.sub(r"\n\s*\n+", "\n", s).strip()
    return (s[:limit] + "…") if len(s) > limit else s


def compact(c):
    threads = (c.get("_embedded") or {}).get("threads") or []
    customer_msgs = [t for t in threads if t.get("type") == "customer"]
    last = (customer_msgs or threads or [{}])[-1]
    cust = c.get("customer") or {}
    assignee = c.get("assignee") or {}
    return {
        "id": c["id"],
        "number": c.get("number"),
        "mailbox": MAILBOXES.get(c.get("mailboxId"), str(c.get("mailboxId"))),
        "status": c.get("status"),
        "subject": c.get("subject"),
        "customer": " ".join(x for x in [cust.get("firstName"), cust.get("lastName")] if x) or cust.get("email"),
        "customer_email": cust.get("email"),
        "assignee": " ".join(x for x in [assignee.get("firstName"), assignee.get("lastName")] if x) or None,
        "created": c.get("createdAt"),
        "updated": c.get("updatedAt"),
        "tags": [t.get("name") for t in ((c.get("_embedded") or {}).get("tags") or [])],
        "last_customer_message": strip_html(last.get("body") or last.get("text") or ""),
        "url": f"{base_url()}/conversation/{c['id']}",
    }


def mark(c, t, since, base_why):
    if (c.get("createdAt") or "") >= since:
        t["why"] = f"{base_why}, новая (создана за последние {WINDOW_HOURS} ч)"
    elif (c.get("updatedAt") or "") >= since:
        t["why"] = f"{base_why}, обновлена за последние {WINDOW_HOURS} ч"
    else:
        t["why"] = f"{base_why}, в работе"
    if c.get("status") == "pending":
        t["why"] += ", статус «ожидает»"
    return t


def sidebar(mailbox_id):
    """Счётчики как в боковой панели FreeScout (activeCount; для спама — totalCount, как показывает UI)."""
    out = {}
    for f in (get(f"mailboxes/{mailbox_id}/folders").get("_embedded") or {}).get("folders", []):
        t = f.get("type")
        if t not in SIDEBAR_TYPES:
            continue
        if t == 20 and f.get("userId") != MY_USER_ID:
            continue
        name = SIDEBAR_TYPES[t] or f.get("name")
        out[name] = f.get("totalCount") if t == 80 else f.get("activeCount")
    return out


def dump(obj):
    sys.stdout.write(json.dumps(obj, ensure_ascii=False, indent=1))
    sys.stdout.write("\n")


def main_mine(mailbox_id):
    """Основной режим: панель + заявки без исполнителя и на Артеме в одном ящике (или во всех при --mailbox all)."""
    load_env()
    now = datetime.now(timezone.utc)
    since = (now - timedelta(hours=WINDOW_HOURS)).strftime("%Y-%m-%dT%H:%M:%SZ")
    boxes = [mailbox_id] if mailbox_id is not None else sorted(MAILBOXES)
    panel = {MAILBOXES.get(b, str(b)): sidebar(b) for b in boxes}
    tickets = []
    for b in boxes:
        tickets += [mark(c, compact(c), since, "без исполнителя")
                    for c in conversations(status="active,pending", assignedTo="", mailboxId=b)]
        tickets += [mark(c, compact(c), since, "на Артеме")
                    for c in conversations(status="active,pending", assignedTo=MY_USER_ID, mailboxId=b)]
    tickets.sort(key=lambda t: t["id"])
    dump({
        "mailbox": None if mailbox_id is None else MAILBOXES.get(mailbox_id, str(mailbox_id)),
        "sidebar": panel[MAILBOXES.get(mailbox_id, str(mailbox_id))] if mailbox_id is not None else panel,
        "note": ("ящик " + MAILBOXES.get(mailbox_id, str(mailbox_id)) if mailbox_id is not None else "все ящики")
                + f"; sidebar — счётчики как в панели FreeScout (активные); tickets — без исполнителя и на Артеме "
                  f"(исполнитель Artem Kovalenko, id {MY_USER_ID}), активные и ожидающие, корзина исключена; "
                  f"«новая»/«обновлена» — за последние {WINDOW_HOURS} ч",
        "tickets": tickets,
    })


def main():
    """Режим --all: общий монитор по всем ящикам (без исполнителя за BACKLOG_DAYS + обновлённые за WINDOW_HOURS)."""
    load_env()
    now = datetime.now(timezone.utc)
    since = (now - timedelta(hours=WINDOW_HOURS)).strftime("%Y-%m-%dT%H:%M:%SZ")
    backlog_since = (now - timedelta(days=BACKLOG_DAYS)).strftime("%Y-%m-%dT%H:%M:%SZ")
    found = {}
    unassigned = conversations(status="active,pending", assignedTo="")
    for c in unassigned:
        if (c.get("updatedAt") or "") >= backlog_since:
            found[c["id"]] = compact(c) | {"why": "без исполнителя"}
    for c in conversations(status="active,pending", updatedSince=since):
        if c["id"] in found:
            found[c["id"]]["why"] = "без исполнителя, обновлена недавно"
        else:
            found[c["id"]] = compact(c) | {"why": f"обновлена за последние {WINDOW_HOURS} ч"}
    dump({"unassigned_total": len(unassigned),
          "note": f"все ящики; без исполнителя за {BACKLOG_DAYS} дн. и обновлённые за {WINDOW_HOURS} ч; "
                  f"корзина исключена; остальной бэклог — только счётчиком",
          "tickets": sorted(found.values(), key=lambda t: t["id"])})


def mailbox_arg(argv):
    """--mailbox N | --mailbox all | --mailbox=N → id ящика или None (все)."""
    for i, a in enumerate(argv):
        v = a.split("=", 1)[1] if a.startswith("--mailbox=") else (argv[i + 1] if a == "--mailbox" and i + 1 < len(argv) else None)
        if v is not None:
            return None if v == "all" else int(v)
    return DEFAULT_MAILBOX


if __name__ == "__main__":
    try:
        main() if "--all" in sys.argv[1:] else main_mine(mailbox_arg(sys.argv[1:]))
    except Exception as e:  # noqa: BLE001
        print(json.dumps({"error": f"{type(e).__name__}: {e}"}, ensure_ascii=False))
        sys.exit(1)
