# Control API — удалённое управление оболочкой (с v0.4.0)

Локальный HTTP-API внутри приложения Hermes Assistant. Нужен, чтобы помощник-разработчик (Claude на
Linux-сервере) мог сам смотреть состояние, запускать установку, читать журналы и настраивать Hermes
Agent на ПК Артема, не трогая окно приложения. Реализация: `crates/hermes-core/src/control.rs`
(axum) + `supervisor.rs` (общее состояние с Tauri-командами).

## Безопасность

- Слушает только `127.0.0.1:8643` (`HERMES_CONTROL_PORT` переопределяет порт). В сеть порт не
  открывается; с сервера доступ через SSH-туннель (`tools/hermesctl.sh` открывает его сам).
- Все маршруты, кроме `/health`, требуют `Authorization: Bearer <token>`. Токен (64 hex) создаётся при
  первом запуске и лежит в `%LOCALAPPDATA%\ua.koavart.hermes\control.token`; сравнение постоянного
  времени. Утечка токена = полный доступ к Hermes CLI под пользователем Koava — файл не копировать
  в репозиторий (скрипт кладёт кэш в `private/`).
- API работает, только пока приложение запущено (сворачивание в трей — позже).

## Маршруты (`/control/v1`)

| Метод | Путь | Что делает |
|---|---|---|
| GET | `/health` | без авторизации: `{ok, name, version}` |
| GET | `/info` | версия, pin Hermes (version/commit/branch), все пути (`hermes_home`, `install_dir`, `hermes_cli`, `app_data_dir`, `ollama_dir`, `models_dir`, `log_dir`), `hermes_installed`, `setup_running` |
| GET | `/status` | `StatusSnapshot` (как экран «Состояние») |
| GET | `/setup/plan` | `SetupPlan` мастера |
| POST | `/setup/start` | тело `SetupOptions` `{model, consent:true, autostart}` → 202; 409 если уже идёт; 400 без согласия |
| POST | `/setup/cancel` | отмена текущей установки |
| GET | `/setup/events?after=N&limit=500` | события установки с номерами `seq` (буфер 2000), `last_seq`, `setup_running` — опрос вместо SSE |
| POST | `/gateway/start`, `/gateway/stop` | Ollama (если установлена) + `hermes gateway start/stop` |
| POST | `/hermes/cli` | `{args:["config","get","model.default"], timeout_secs:600}` → `{stdout[], stderr[], success, status}` даже при ненулевом коде |
| GET / PUT | `/hermes/config` | `config.yaml` целиком (текст); PUT пишет и сохраняет `config.yaml.bak` |
| GET | `/hermes/env` | `.env` с замаскированными значениями ключей `*KEY*`, `*TOKEN*`, `*SECRET*`, `*PASSWORD*` |
| GET | `/logs?tail=200&file=` | хвост самого свежего `*.log` приложения (туда же зеркалятся события установки) |

Ошибки — JSON `{error}` с кодами 400/401/404/409/500.

## Использование с сервера

```bash
tools/hermesctl.sh health            # туннель + проверка
tools/hermesctl.sh info
tools/hermesctl.sh setup qwen3.5     # запустить установку с согласием, автозапуск включён
tools/hermesctl.sh events 0          # события с начала
tools/hermesctl.sh logs 300
tools/hermesctl.sh cli config get model.default
tools/hermesctl.sh cli doctor
tools/hermesctl.sh config-get > /tmp/config.yaml && tools/hermesctl.sh config-put /tmp/config.yaml
```

Скрипт читает токен с ПК один раз (`ssh main-pc`, файл в профиле Koava) и кэширует в
`private/control.token`; при 401 кэш сбрасывается.
