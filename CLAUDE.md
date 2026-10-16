# Hermes — личный ИИ-помощник на ПК Артема (Tauri-оболочка поверх Hermes Agent)

Личный проект Артема, создан 11.10.2026. Концепт менялся дважды: 11.10 — Hermes Agent на Mac;
15.10 утром — своё приложение с нуля по мастер-промту; **15.10 вечером — вариант B (действующий):
своё desktop-приложение «Hermes» на Tauri 2 + React + Rust, внутри которого в одном установщике
едет Hermes Agent (Nous Research, мозг и инструменты) и Ollama (локальные модели).** Пользователь
ничего не ставит вручную: один установщик, первый запуск сам разворачивает всё нужное.
НЕ связан с проектом Jarvis (локальная LLM в Deka).
Репозиторий: https://github.com/koavart-dotcom/hermes-assistant (private, аккаунт koavart-dotcom), ветка `main`.

## Документы-основания

- **`Nexus_AI_Assistant_Claude_Code_Master_Prompt.md`** — ТЗ Артема на приложение (в нём помощник
  называется Nexus, читать как Hermes). Действует для оболочки: §2 решения, §3 стек, §5 экраны, §6
  жизненный цикл, §13 безопасность, §17–§19 надёжность/тесты/сборка, §22 правила работы.
  Разделы §7 Agent Runtime, §8 DesktopControl, §9 браузер, §10 голосовые движки, §11 Ollama-сервис,
  §12 интеграции **закрывает Hermes Agent**, а не наш код; наша работа там — конфигурация, плагин
  разрешений и UI. При противоречии: решения Артема в чате > CLAUDE.md > мастер-промт.
- **`docs/OPEN_SOURCE_RESEARCH.md`** — исследование аналогов и Hermes Agent (15.10.2026), с фактами
  о возможностях Hermes (раздел 6). **`docs/ARCHITECTURE.md`**, **`docs/ROADMAP.md`** — архитектура и
  этапы варианта B. **`docs/decisions.md`** — ADR.
- **Релизы Hermes живут на ветках `release/<версия>`, не на `main`** (v0.22.2 = `release/0.22.2`):
  `install.ps1 -Commit` требует `-Branch` с этой веткой, иначе `commit_not_on_branch` (первый прогон
  мастера 16.10). Pin: `src-tauri/resources/hermes/pin.json` {version, commit, branch}.
- Перед опорой на любую возможность Hermes Agent проверять актуальную документацию
  (hermes-agent.nousresearch.com, репозиторий NousResearch/hermes-agent): релизы выходят ежедневно,
  API меняется. Версия, на которую ориентируемся, фиксируется в `docs/ARCHITECTURE.md`.

## Ключевые решения

- **15.10.2026, вариант B (Артем).** Приложение своё, мозг — Hermes Agent. Причина: Hermes уже
  покрывает ~90 % ТЗ (цикл инструментов, Ollama и облако, одобрения опасных действий, computer-use
  на Windows через UIAutomation, браузер, голос с русским STT/TTS, API со стримингом, cron, Telegram,
  память, skills, MCP), 254k★, 100+ контрибьюторов, MIT. Писать это заново — месяцы.
  Свой UI нужен, чтобы приложение было «своим», под наши сценарии, и ставилось одним файлом.
- **Один установщик, без ручных шагов.** В пакет входят Tauri-приложение и всё, что нужно для первого
  запуска. Hermes Agent разворачивается при первом запуске через его же стадийный протокол
  `install.ps1 -Manifest / -Stage NAME -NonInteractive -Json` (так делает официальный Tauri-установщик
  Hermes `apps/bootstrap-installer`, MIT — берём за образец). Ollama — как сайдкар из
  `ollama-windows-amd64.zip` (CLI + библиотеки NVIDIA, без установщика), запускается нашим супервизором
  с `OLLAMA_MODELS` на диске D:. Модели (LLM, Faster-Whisper, Piper) скачиваются в мастере первого
  запуска после одного согласия пользователя с индикатором прогресса (промт §10, §11).
  Допущение: первый запуск требует интернета; полностью офлайновый пакет (несколько ГБ) — позже.
- **Стек оболочки — по промту §3:** Tauri 2.12 (стабильная; Tauri 3 только alpha), Rust для
  супервизора процессов, bootstrap, конфигов, keyring и аудио; React 19 + TypeScript + Vite, Tailwind,
  shadcn/ui, Zustand, TanStack Query. Отклонение от корпоративного стека (Nuxt) согласовано.
- **Связь оболочки с Hermes** — локальный `api_server` Hermes (`127.0.0.1:8642`, Bearer-ключ, который
  мы генерируем и храним в Windows Credential Manager через `keyring`): `POST /v1/runs` + SSE
  `/events` (токены, прогресс инструментов, `approval.request`), `POST /v1/runs/{id}/approval`
  (once/session/always/deny), `/stop`, Sessions API, `/v1/capabilities`. Голос: захват микрофона в
  оболочке, транскрипция и озвучка через эндпоинты Hermes (`/api/audio/transcribe`,
  `/api/audio/voice-config`, speech WebSocket — пути проверить по коду перед реализацией).
- **Автообновление (16.10, Артем: «чтоб я каждый раз не переустанавливал»).** tauri-plugin-updater
  (Rust API, без npm-плагина; `default-features = false`, т.к. `zip ^4` снят с crates.io): endpoint
  `releases/latest/download/latest.json`, подпись ключом из `private/updater.key` (секрет GitHub
  `TAURI_SIGNING_PRIVATE_KEY`, без пароля), `release.yml` публикует релиз сразу с `latest.json`.
  Репозиторий приватный → обновлятор шлёт `Authorization: Bearer <GitHub token>` из keyring
  (`PUT /update/token` в Control API). Баннер в окне (`src/app/UpdateBanner.tsx`) проверяет при
  запуске и раз в 6 ч. Версии ставить только через тег: `tauri.conf.json`, `package.json`, `Cargo.toml`.
- **Control API (16.10, Артем: «чтоб ты сам мог заходить и настраивать»).** В приложении HTTP-API на
  `127.0.0.1:8643` с Bearer-токеном (`%LOCALAPPDATA%\ua.koavart.hermes\control.token`): статус, запуск
  мастера, события, gateway, `hermes` CLI, `config.yaml`/`.env`, журнал. С сервера — `tools/hermesctl.sh`
  (SSH-туннель, токен кэшируется в `private/`). Описание — `docs/CONTROL_API.md`. Отладку на ПК вести
  через него, а не через скриншоты Артема; приложение должно быть запущено.
- **Разрешения.** Hermes: `approvals.mode: manual` (не `smart`, не `off`), `approvals.deny` для своих
  запретов, `HERMES_WRITE_SAFE_ROOT`. Наши уровни Read-only / Normal / Confirm / Blocked из промта §7
  реализует **наш Python-плагин Hermes** на хуке `pre_tool_call` (block / modify / approve, fail-closed),
  который ставится вместе с приложением. Подтверждения показывает наш UI. Системный промпт — не
  механизм безопасности (промт §7).
- **Модели.** Локально на RTX 3080 Ti (12 ГБ): старт — Qwen3.5 9B Q4 целиком в VRAM (`docs/models.md`,
  строка ПК); Jarvis Deka (OpenAI-совместимый, 27B) — второй провайдер в сети Deka; облако (Anthropic,
  OpenAI, Gemini) через провайдеры Hermes по явному выбору. Fallback-провайдеры Hermes — с согласия.
- **Первый сценарий (MVP):** голосовой чат (push-to-talk по горячей клавише) + действия на этом ПК
  через инструменты Hermes с подтверждениями в нашем UI. Сценарии Deka (FreeScout-триаж, Telegram,
  cron) — переносятся из `archive/hermes-agent/skills` и `config` почти без правок, этап 7.
- **Mac mini M4** — Hermes там заморожен (служба не трогается); позже возможная macOS-сборка.
- **Умный дом** — на паузе; при возврате через плагин Home Assistant из каталога Hermes.
- Отклонения от промта, принятые осознанно: агентный цикл на Python (Hermes), а не Rust; браузер —
  встроенный browser tool Hermes (Chromium/CDP), а не Playwright; хранение сессий — SQLite Hermes,
  наша SQLite только для настроек оболочки.

## Окружение (проверено 15.10.2026)

**Основной ПК** (`ssh main-pc`, PowerShell; LAN 192.168.229.x). **По SSH заходим как пользователь
`claude`**, а интерактивный пользователь, под которым стоит приложение, — **Koava**: `$env:LOCALAPPDATA`
в SSH-сессии указывает на профиль `claude`, поэтому пути писать явно: `C:\Users\Koava\AppData\Local\...`.
- ASUS, Intel i5-14600KF (14C/20T), 32 ГБ ОЗУ, **RTX 3080 Ti 12 ГБ VRAM** (драйвер 617.42 ≥ 551.61
  для Ollama), Windows 11 Pro build 26300 (≥ 22H2), WebView2 155 есть.
- Диски: C: 49 ГБ свободно (мало!), D: 3,5 ТБ свободно, E: 27 ГБ, F: 196 ГБ. Модели — на D:.
- Аудио: гарнитура HyperX Cloud II Core Wireless (микрофон + наушники).
- Установлено: git 2.54, VS Code, .NET SDK 10.0.402 (не нужен, можно удалить). Нет Node, Rust,
  Python, Ollama, Hermes. В SSH-сессии `winget` не в PATH.
- Hermes нативно на Windows ставится в `%LOCALAPPDATA%\hermes` (uv-managed Python 3.14, Git for
  Windows bootstrap, cua-driver), gateway — задача Планировщика ONLOGON через `pythonw.exe`.
- Приложение (NSIS, currentUser) — `%LOCALAPPDATA%\Hermes Assistant\hermes-assistant.exe`; данные и логи
  оболочки — `%LOCALAPPDATA%\ua.koavart.hermes`. Не называть продукт «Hermes»: папка совпала бы с домом
  Hermes Agent (ADR 16.10.2026).

**Эта машина (Linux, разработка):** Node 22, pnpm 9; Rust не установлен (поставить rustup +
webkit2gtk-4.1, libayatana-appindicator, librsvg для Linux-сборки Tauri). Windows-сборка —
GitHub Actions `tauri-action` на windows-latest; `.exe/.msi` проверяем на ПК по SSH.

## Правила работы в проекте

- **На ПК Артема ничего не ставить без его «ОК»** (15.10 отклонена автоустановка Ollama). Концепт B
  предполагает, что наш установщик ставит Hermes и Ollama сам — но перед **первым** прогоном
  установщика/бутстрапа на ПК сказать, что и куда ляжет, и дождаться согласия. Диагностика
  (`Get-CimInstance`, версии, свободное место) — без спроса.
- Правила промта §22: проверяемые этапы; после каждого — lint, typecheck, тесты, сборка; мокап не
  выдавать за функцию; различать реализовано / частично / запланировано; `docs/ROADMAP.md` актуален.
- Всё, что меняет состояние ПК (удаление, команды, системные настройки, отправка сообщений, деньги) —
  Confirm или Blocked, проверка в Hermes-плагине и политике Hermes, не в промпте.
- Секреты — Windows Credential Manager (`keyring`) или `.env` Hermes с правами только владельца;
  `private/` в `.gitignore`. Доступ к хостам — по SSH-ключам.
- Язык UI и общения — русский; код, имена инструментов, логи — английские.
- Схемы для `docs/` — Mermaid через скил `/design-doc-mermaid`, исходники `.mmd` рядом.
- Важное фиксировать в Obsidian (`Projects/AI-Agent.md`, `Daily/`), ADR — `docs/decisions.md`.
- Коммитить и пушить в GitHub по ходу этапов (решение Артема 15.10).

## Структура репозитория

Целевая (уточняется по ходу, пустые пакеты не создавать):
- `src/` — React-оболочка: `features/{chat,approvals,voice,status,settings,setup}`, `services/`
  (клиент api_server, SSE), `stores/`, `types/` (генерируются из Rust через ts-rs)
- `src-tauri/` — Rust: `supervisor/` (gateway, ollama), `bootstrap/` (стадии install.ps1, Ollama zip,
  модели), `hermes_config/` (config.yaml, .env, плагин), `audio/`, `secrets/`, `commands/`,
  `capabilities/` (минимальные)
- `hermes-plugins/hermes_permissions/` — Python-плагин Hermes (pre_tool_call, уровни разрешений) + тесты
- `hermes-skills/` — наши skills для Hermes (из `archive/hermes-agent/skills` после переноса)
- `docs/` — `ARCHITECTURE.md`, `ROADMAP.md`, `OPEN_SOURCE_RESEARCH.md`, `decisions.md`, `models.md`,
  `diagrams/`; `concept.md`, `deka-scenarios.md` — история этапа Mac
- `archive/hermes-agent/` — конфиг, skills, скрипты этапа Mac (11–14.10.2026)
- `tools/` — `hermesctl.sh`: доступ к Control API на ПК с этого сервера через SSH-туннель
- `.github/workflows/` — CI (lint/typecheck/tests на push, сборка Windows на тег `v*`)
- `private/` — реквизиты хостов, не в git

## Статус

15.10.2026: вариант B выбран; этап 1 в работе — каркас написан (Tauri 2.12, React 19, TypeScript 6,
Vite 8, Tailwind 4, shadcn/ui preset nova, Zustand, TanStack Query, react-router 8, vitest 5; Rust-крейт с
командой `app_info` и ts-rs; CI: frontend + rust(ubuntu) + windows cargo check; release по тегу).
Локально проходят lint/typecheck/test/build. **На этом сервере диск заполнен** (после чистки ~2 ГБ),
Rust не установлен — Rust проверяется только в CI; `tsc -b` с project references (`tsconfig.app.json`,
`tsconfig.node.json`), артефакты в `node_modules/.tmp`. Для shadcn CLI нужен алиас `@` в корневом
`tsconfig.json`, иначе он пишет файлы в папку `@/`. Этап 1 закрыт: v0.1.0 установлен на ПК и открывается.
**Этап 2 написан (v0.2.0)**: workspace `crates/hermes-core` (без Tauri: `bootstrap` — стадийный протокол
install.ps1 v0.22.2 из `src-tauri/resources/hermes/` с pin commit; `ollama` — zip v0.40.5 → `D:\hermes\ollama`,
`OLLAMA_MODELS=D:\hermes\models`, pull с прогрессом; `hermes_config` — `hermes config set` (model.*, approvals.mode
manual, API_SERVER_*); `gateway` — `hermes gateway start/stop/install` + `/health` на 8642; `secrets` — keyring 3
(Windows Credential Manager) с файловым фолбэком; `setup` — конвейер и `SetupPlan`), Tauri-команды
(`setup_plan/setup_start/setup_cancel/status_snapshot/gateway_start/gateway_stop`, события `setup:event`),
мастер «Установка» с согласием на скачивание и живой экран «Состояние». На сервере стоит rustup minimal
(`~/.cargo/bin`, в PATH не добавлен): `cargo check/clippy -p hermes-core` работают локально, полные тесты и
Tauri — в CI. 16.10: v0.3.0 — продукт переименован в «Hermes Assistant», папка приложения отделена от дома
Hermes Agent, NSIS-хук сносит старую «Hermes» (ADR). v0.3.1 — ветка `release/<v>` для pin; v0.4.0 — Control API;
v0.4.1 — не-UTF-8 вывод uv; v0.4.2 — свободный порт Ollama. **Этап 2 закрыт 16.10.2026**: на ПК Hermes v0.22.2,
gateway (автозапуск через «Автозагрузку», `Hermes_Gateway.vbs`), Ollama на 127.0.0.1:11437 (11434 занят VS Code),
модель `qwen3.5:latest` в `D:\hermes\models`. Процесс релиза: коммит и `git push` делаю я, **тег, `git push
origin v*` иногда и публикацию блокирует классификатор auto-режима** — тогда прошу Артема выполнить команду через
`! ...`. **Этап 3 закрыт 16.10 (v0.5.x)**: чат через Sessions API, проверено на ПК; v0.5.1 — автообновление;
v0.5.2 — `OLLAMA_CONTEXT_LENGTH=65536` (при 4096 Hermes-промпт ~14k токенов обрезался и модель не вызывала
инструменты; теперь `terminal`/`search_files` работают из чата) + перезапуск сайдкара при старте приложения.
Факты ПК: рабочий стол Koava — `C:\Users\Koava\OneDrive\Desktop`; приложение не в автозагрузке (gateway — да).
Открыто: публичность репозитория для обновлятора, Markdown в ответах, автозагрузка приложения, затем этап 4.
Открыто: слово-активации по-русски (openWakeWord требует свою `.tflite`; Porcupine имеет встроенное
«jarvis»; sherpa — открытый словарь), точные пути голосовых эндпоинтов api_server, формат пакета
(NSIS vs MSI) и размер первого скачивания.
