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

**Чат (с v0.5.0, этап 3)** — те же вызовы, что делает окно «Чат», через Sessions API Hermes:

| Метод | Путь | Что делает |
|---|---|---|
| GET / POST | `/chat/sessions` | список сессий Hermes (`SessionSummary[]`) / создать (`{title?}` → 201) |
| DELETE | `/chat/sessions/{id}` | удалить сессию |
| GET | `/chat/sessions/{id}/messages` | история (`ChatMessage[]`, старые первыми) |
| POST | `/chat/send` | `{session_id, message}` → 202; 409 если в сессии уже идёт ход |
| GET | `/chat/events?after=N&limit=500` | события чата (`ChatEnvelope` + `seq`): `run_started`, `delta`, `thinking`, `tool_started/finished`, `commentary`, `approval_request`, `completed`, `failed`, `cancelled`, `ended` |
| POST | `/chat/stop` | `{session_id}` — остановить ход |
| POST | `/chat/approve` | `{run_id, choice}` — `once` / `session` / `always` / `deny` |

**Обновления (с v0.5.1):** `PUT /update/token` `{token}` — read-only токен GitHub (fine-grained, Contents: read
на `hermes-assistant`) для встроенного обновлятора, пока репозиторий приватный; пустая строка удаляет.
Хранится в Credential Manager (фолбэк — файл `secrets/github_token`). Сама проверка и установка
обновления — в окне (баннер «Доступна версия …» → «Обновить»), Tauri-команды `update_check`/`update_install`.

**Голос (с v0.7.0, этап 5):** `GET /voice/status` (`VoiceStatus`: deps_ready, server_running, провайдеры);
`POST /voice/setup` → 202 (ставит `hermes pm install --extra voice`, пишет настройки STT/TTS, запускает веб-сервер
Hermes на 127.0.0.1:8644; прогресс — в `/setup/events`); `POST /voice/transcribe` `{audio_base64, mime_type}` →
`{transcript}`; `POST /voice/speak` `{text}` → `{data_url}` (Edge TTS, mp3).

**Разрешения (с v0.6.0, этап 4):** `GET /permissions/log?tail=200` — журнал решений плагина
`hermes_permissions` (`{ts, tool, action: allow|confirm|block, reason, preview, task_id}`);
`GET`/`PUT /permissions/policy` — переопределение политики `hermes_permissions.json`
(`{roots: [...], tools: {name: allow|confirm|block|roots|terminal}, default, terminal: {block: [regex], allow: [cmd],
default}}`); плагин читает файл при каждом вызове инструмента, перезапуск не нужен.

**Ключи и интеграции (с v0.9.1):** `GET /hermes/env/keys` — список переменных из `.env` Hermes как
`[{name, masked}]` (значения не возвращаются, только `••••` + последние 4 символа); `GET /hermes/env/presets` —
известные интеграции для экрана «Настройки» (FreeScout, Telegram, Anthropic, OpenAI, Gemini, GitHub: id, заголовок,
имена переменных, подсказка); `PUT /hermes/env/keys` `{name, value}` — записать `NAME=value` (имя — только
`A-Z0-9_`, значение в одну строку; файл переписывается с резервной копией `.bak`); `DELETE /hermes/env/keys/{name}`.
`.env` gateway читает при старте, поэтому после изменений нужен `POST /gateway/restart` (стоп + старт).
Сами значения ключей в логах и ответах API не появляются.

**Быстрые команды, дополнение (v0.12–0.13):** `POST /quick/run` принимает `{id, confirmed, query}` — `query` подставляется
вместо `{query}` в действия параметрической команды (`hermesctl quick-run web.google курс доллара`); ответ
`QuickRunResult` получил `ui` (окно, которое должна открыть оболочка), `handoff` (текст для модели, например страница
для пересказа) и `spoken` (короткий ответ без модели). `/quick/match` возвращает `query` в `QuickMatch`.

**Хелпдеск (с v0.10.0, этап 7):** `GET /freescout/tickets` — `TicketsSnapshot` `{fetched_at, my_user_id, mine: [Ticket],
unassigned: [Ticket], summary}` из FreeScout по ключам `FREESCOUT_URL`/`FREESCOUT_API_KEY`/`FREESCOUT_MY_USER_ID` в `.env`
Hermes (активные + ожидающие, корзина отброшена, `mark: new|updated|working` по сегодняшнему дню UTC); `summary` —
фраза для озвучки. Без ключей — 500 с подсказкой. Окно «Мои тикеты» в приложении открывает быстрая команда «мои тикеты»
(действие `ui:tickets`, Tauri-команда `tickets_window_open`); через API окно не открыть (нужен Tauri).

**Настройки (с v0.14.0):** `GET`/`PUT /prefs` — настройки оболочки (`Prefs`: `voice_volume` 0..1, `silent`, `wake_words`,
`wake_required`, `voice_pack`), файл `settings.json` в данных оболочки; `GET /models/options` — выбор моделей (локальные
Ollama, Jarvis Deka, Anthropic/OpenAI/Gemini с именем нужного ключа), `GET`/`PUT /models/current` (`{provider, model,
base_url}`, запись через `hermes config set` + перезапуск gateway), `POST /models/pull {tag}` → 202, прогресс в
`/setup/events` (`model: ready <tag>` в конце); `GET`/`PUT /speech` (`{stt_provider, stt_model, stt_language, tts_provider,
tts_voice}`, перезапуск сервера голоса), `GET /speech/tts-options`; `GET /voice/packs` (установленные наборы реакций и
доступные для импорта из Luxify), `POST /voice/packs/import {name}`. `hermesctl prefs|models|model|model-set|model-pull|
speech|speech-set|voice-packs|voice-pack-import`.

**Утренняя сводка (с v0.16.0, источники — с v0.23.0):** `POST /digest/run {telegram}` → `DigestResult` `{text, spoken, telegram_sent,
telegram_error}` — текст для чата/Telegram и фраза для озвучки. С v0.23.0 это утренняя сводка: встречи на сегодня
из Google Calendar (`GOOGLE_CALENDAR_ICS_URL` — секретный адрес iCal, повторы через `rrule`), непрочитанная почта Gmail
(`GMAIL_ADDRESS` + `GMAIL_APP_PASSWORD`, IMAP `EXAMINE` — письма не помечаются прочитанными), открытые задачи Jira
(`JIRA_USER` + `JIRA_API_TOKEN`, `JIRA_SITES`) и заявки FreeScout; ненастроенный источник пропускается, упавший — строка
«⚠️» в тексте, остальные уходят; при `telegram: true`
отправка через Bot API (`TELEGRAM_BOT_TOKEN` + `TELEGRAM_CHAT_ID` или первый из `TELEGRAM_ALLOWED_USERS` в `.env`).
Расписание (`prefs.digest`: enabled, times `HH:MM`, speak, telegram) исполняет главное окно приложения по местному
времени. `hermesctl digest [true]`.

**Ответ на тикет (с v0.17.0):** команда «ответь на тикет N» (`ticket_draft:N`) скачивает переписку заявки
(`GET /api/conversations/{id}?embed=threads`, хронологически, без HTML) и передаёт её модели как `handoff` с
инструкцией написать черновик от имени Артема; id заявки запоминается как ожидающий. «Отправляй» (`ui:ticket_send`,
с подтверждением) берёт последний ответ помощника из активного диалога и шлёт `POST /api/conversations/{id}/threads`
`{type: message, user: FREESCOUT_MY_USER_ID}`. Control API: `GET /freescout/draft` → `{pending_conversation_id}`,
`POST /freescout/reply {text, number?}`; `hermesctl ticket-draft`, `ticket-reply N текст`. «Открой тикет N»
(`ticket_open:N`) открывает заявку в браузере.

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

**Созвоны (с v0.25.0):** `GET /calls` → `CallStatus {in_call, apps}` — какие приложения (кроме наших) сейчас держат
микрофон или камеру по реестру Windows (`CapabilityAccessManager\ConsentStore`); окно по нему включает тишину.

