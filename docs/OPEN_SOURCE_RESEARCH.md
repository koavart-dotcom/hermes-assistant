# Open-source research — аналоги и библиотеки для Hermes

Дата проверки: **15.10.2026**. Метод: `gh api` (метаданные, коммиты, issues, releases), чтение README,
`package.json`, `Cargo.toml` и ключевых файлов через raw-ссылки; репозитории не клонировались.
Задание — мастер-промт §15. Все выводы ниже — из кода, не из README.

## 1. Сводная таблица аналогов

| | Open Nexus | AgentChat | CatDesk | ChatShell Desktop |
|---|---|---|---|---|
| URL | github.com/chiarorosa/open-nexus | github.com/Zorahm/AgentChat | github.com/AlexisBERT-Work/ai-desktop-copilot | github.com/chatshellapp/chatshell-desktop |
| Лицензия | MIT | MIT | MIT | Apache-2.0 (без NOTICE; «ChatShell» — товарный знак Seedvector LLC) |
| Звёзды / авторы | 2 / 1 | 2 / 1 | 0 / 1 | 24 / 1 |
| Активность | 8 коммитов за один день 23.04.2026, мёртв | 127 коммитов, 25.05–27.08.2026, релиз v3.1.0 | 194 коммита, 28.05–13.10.2026, 20 открытых issue-планов | 523 коммита, 13.11.2025–13.10.2026, 12 релизов, v0.8.2 |
| Языки / слои | TS + немного Rust | **Python**-бэкенд (PyInstaller-сайдкар, FastAPI+SSE), Tauri-оболочка | React → Rust → **Node-сайдкар** (агент, 68 инструментов) → **Python-сайдкар** (скриншот/OCR) | React + **настоящее Rust-ядро** (rig, rmcp, sqlx) + соседний crate `chatshell-agent` |
| Tauri | 2 | 2 | 2.5 | 2 |
| Агентный цикл | нет: 1 вызов LLM → JSON-команда | `AgentLoop.run_stream()`, события, лимит итераций 50, retry | в Node: `/api/chat` tools + восстановление tool-call из текста, отбор инструментов, лёгкая/тяжёлая модель | в Rust (`agent.rs`, `agent_streaming.rs`), compaction, context pruning |
| Провайдеры | один OpenAI-compatible (по умолчанию OpenRouter) | LiteLLM (всё) | **только Ollama** | ~20 модулей: OpenAI, Anthropic, Gemini, Azure, Ollama, LM Studio, OpenRouter… |
| Локальная модель | через смену baseUrl | Ollama/LM Studio «без ключа» по эвристике хоста | Ollama нативно (stream, tools, keep_alive, num_ctx, think:false) | Ollama/LM Studio/GPUStack через OpenAI-compat |
| Инструменты ОС | 4 команды через allowlist в Rust (`router.rs`) | файлы/shell внутри chat_dir, bwrap-песочница (Linux) | 68 инструментов: PowerShell/cmd с deny-regex, запуск приложений, файлы под %USERPROFILE% | bash/read/edit/write/grep/glob, `bash_security.rs` (tree-sitter AST), `path_policy.rs` |
| Разрешения / подтверждения | `window.confirm()` | нет per-tool; инструмент `ask_user` | **есть**: low/medium/high/critical, session-grant, safe-mode, UI PermissionPrompt, аудит в Rust | статические политики, UI-подтверждений нет |
| Браузер | открыть URL | web_fetch | нет | `headless_chrome` (CDP) + Readability, не Playwright |
| Голос | Web Speech API (WebView), TTS нет | нет | **есть, в Rust**: sherpa-onnx (Silero VAD + Parakeet STT + Piper TTS) + cpal | нет |
| Хранение | localStorage (ключи открытым текстом!) | SQLite (1 таблица, JSON-blob), keyring | sql.js (SQLite в памяти → файл), JSON | SQLite (sqlx) + FTS, AES-256-GCM + OS keychain |
| MCP / skills | skills-JSON | MCP stdio с idle-reaper; SKILL.md | нет | MCP stdio + Streamable HTTP + OAuth 2.1; SKILL.md |
| Платформы, CI | msi/dmg объявлены, CI нет, ничего не собрано | Windows, Linux, Android; macOS нет | **только Windows** (CI windows-latest, Inno Setup) | Windows MSI, macOS DMG, Linux AppImage в каждом релизе |
| Тесты | нет | 59 файлов pytest | 640+ тестов, clippy -D warnings | vitest + cargo test + clippy |

**Главный вывод.** Ни один проект не годится как основа для форка: все — проекты одного автора без
сообщества, три из четырёх держат агентную логику вне Rust (Python/Node), а единственный Rust-first
(ChatShell) не собирается без соседнего репозитория, не имеет голоса, подтверждений и интеграций.
Брать — отдельные модули и идеи (ниже), архитектуру строить свою по промту.

## 2. Что берём из каждого

### Open Nexus (MIT)
- `src-tauri/src/router.rs` — enum разрешённых команд, типизированные аргументы, коды ошибок,
  ограничение путей. Каркас для нашего Tool Registry на стороне Rust.
- `src-tauri/src/commands.rs` (fn ~686–960) — кроссплатформенный поиск и запуск приложений
  (Windows: `where`, ярлыки Start Menu; macOS: `open`; Linux: `xdg-open`).
- Не брать: LLM-слой (без стриминга и tool calling), ключи в localStorage, Web Speech API
  (в WebView2 уходит в облако Microsoft, в WebKitGTK не работает), `window.confirm` как разрешения.

### AgentChat (MIT)
- Событийная модель стрима агента (`backend/agent/loop.py`: `pass_start`, `token`, `reasoning`,
  `tool_start/chunk/end`, `tool_progress`, `user_question`, `iterations_exhausted`) — переносим
  как enum событий в Rust и контракт для UI.
- Ошибка инструмента возвращается модели текстом, успех оценивается отдельно (`agent/tool_outcome.py`).
- Маркировка недоверенного контента (`agent/untrusted.py`) — обязательна для браузера и файлов (промт §9, §13).
- Whitelist инструментов поверх политики (`tools/factory.py`), idle-reaper MCP-серверов
  (`mcp_integration/manager.py`), «локальный хост → ключ не нужен» (`llm/api_base.py`).
- Не переносится: LiteLLM, PyInstaller-сайдкар, bwrap.

### CatDesk (MIT)
- **Модель разрешений** `packages/agent-runtime/src/permissions/PermissionEngine.ts` +
  `packages/shared-types/src/permissions.ts`: уровни low (авто) / medium (раз в сессию) / high
  (каждый раз) / critical (выключен), safe-mode, `features/agent/PermissionPrompt.tsx`. Ближе всего
  к нашим Read-only/Normal/Confirm/Blocked; переносим логику в Rust.
- `llm/recoverToolCalls.ts` (tool-call из текста: `<tool_call>`, ```json) и `llm/selectTools.ts`
  (подмножество инструментов под запрос) — критично для небольших моделей на Ollama.
- **Голосовой контур в Rust**: `src-tauri/src/core/voice/*` — sherpa-onnx (VAD + STT + Piper TTS)
  + cpal; доказано работающим на Windows в Tauri 2.
- Overlay-окно (`tauri.conf.json`: decorations:false, transparent, alwaysOnTop, skipTaskbar) и
  двойной hotkey (`core/hotkeys.rs`); deny-list PowerShell-эвазий (`security/commandPolicy.ts`).
- Не брать: три процесса (Node + Python сайдкары, установщик ~18 ГБ), sql.js, балласт
  (Supabase, биржи, новости), 68 уязвимостей в `pnpm audit`.

### ChatShell Desktop (Apache-2.0)
- `src-tauri/src/bash_security.rs` (валидаторы команд через tree-sitter → `SecurityVerdict`) и
  `path_policy.rs` (блок-листы чтения/записи, граница корня) — база для Permission Manager.
- `crypto.rs` + `keychain.rs` (AES-256-GCM + OS keychain), `provider_catalog.rs` /
  `provider_registry.rs` (каталог провайдеров и capabilities), `skills/scanner.rs` (SKILL.md,
  совместим с Claude-skills), `mcp/{manager,oauth}.rs` (rmcp, OAuth 2.1/PKCE), `compaction.rs`.
- ts-rs генерация TypeScript-типов из Rust (`src/types/generated`, скрипт `types:check`) — берём
  как практику: один источник типов.
- CI `.github/workflows/build.yml`: по тегу Windows MSI + macOS universal DMG + Linux AppImage —
  образец для нашего пайплайна.
- Условия: сохранить LICENSE и copyright, пометить изменённые файлы, указать источник в NOTICE;
  имя/логотип ChatShell не использовать.

## 3. Библиотеки и инфраструктура (проверено 15.10.2026)

| Компонент | Выбор | Факты | Примечание |
|---|---|---|---|
| Tauri | **2.12.4** (стабильная, 14.10.2026) | Tauri 3 — только alpha.5 | на 3 не переходим |
| Шаблон | `dannysmith/tauri-template` (MIT, 309★, push 12.08.2026) | React, TS, shadcn/ui, Tailwind, TanStack Query, Zustand — ровно наш стек §3 | взять как референс структуры, не как зависимость |
| Ollama | v0.40.5 (14.10.2026), MIT | `/api/chat`: streaming + `tools` вместе, `format` (JSON-schema), `keep_alive`, `think`; `/api/tags`, `/api/ps`, `/api/show` (capabilities), `/api/pull` с прогрессом; есть `docs/api/openai-compatibility` и `anthropic-compatibility` | Windows: трей-приложение, порт 11434, `OLLAMA_MODELS` для папки моделей (у нас D:), драйвер NVIDIA ≥ 551.61 (у ПК 617.42) |
| Rust-клиент Ollama | `ollama-rs` 0.3.6 или свой на `reqwest` | | свой проще держать под общий `AIProvider` |
| LLM-абстракция в Rust | свои адаптеры (промт §3) | ChatShell использует `rig` 0.42 — оценить на этапе 2 как опцию | |
| MCP | `rmcp` 2.x (официальный Rust SDK) | используется в ChatShell | этап 7 |
| Playwright | v1.64.1, Apache-2.0 | **нет Rust-биндинга**: crate `playwright` заброшен (0.0.20, 2022). Варианты: Node-сайдкар с Playwright (+ Chromium ~300 МБ) или `@playwright/mcp` 0.0.83 как MCP-сервер | см. вопрос 1 ниже |
| CDP из Rust | `chromiumoxide` 0.9.1 (02.2026), `headless_chrome` 1.0.22 (06.2026) | ChatShell работает на headless_chrome | альтернатива Playwright без Node |
| whisper.cpp | v1.9.5 (06.10.2026), MIT | биндинги в репо: go, java, javascript, ruby — Rust нет; `whisper-rs` 0.16 (Unlicense, переехал на Codeberg) требует CMake + clang + CUDA toolkit при сборке | усложняет CI |
| STT/TTS/VAD/wake-word в одном | **sherpa-onnx** (Apache-2.0, есть Rust-крейт) | whisper, Parakeet, Silero VAD, Piper TTS, keyword spotting; CatDesk доказал на Windows | рекомендуемый путь для §10 |
| Аудио | `cpal` 0.18.2, `rodio` 0.23 | | |
| Секреты | `keyring` 4.2.0 | Windows Credential Manager / macOS Keychain / Secret Service | промт §13 |
| SQLite | `sqlx` (как ChatShell) или `tauri-plugin-sql` 2.6 | репозитории и миграции — в Rust, не во фронте | промт §14 |
| Плагины Tauri | global-shortcut 2.4.1, autostart 2.7, shell 2.5, opener 2.7, single-instance, updater, `tauri-plugin-screenshots`, `tauri-plugin-system-info` | все обновлены 14.10.2026 | |
| Системная информация | `sysinfo` crate | | промт §8 |

## 4. Выводы для архитектуры Hermes

1. **Rust-first подтверждается, но без иллюзий.** Все «богатые» аналоги держат агента в Node/Python,
   потому что там готовые SDK. В Rust придётся писать адаптеры провайдеров, agent loop и
   tool registry самим (ChatShell показывает, что это посильно: rig + rmcp + sqlx).
2. **Ollama и малые модели.** Нативный `/api/chat` с tools и streaming есть, но маленькие модели
   часто отдают tool-call текстом — нужен recovery (CatDesk) и отбор инструментов под запрос.
   Capabilities модели читаем из `/api/show`, как требует промт §3.
3. **Разрешения — в Rust, с session-grant.** Комбинация: модель рисков CatDesk (уровни, safe-mode,
   UI-промпт) + статические политики ChatShell (AST команд, границы путей) + untrusted-маркировка
   AgentChat. Это закрывает промт §7, §8, §13.
4. **Голос — sherpa-onnx в Rust**, а не whisper.cpp напрямую: одна библиотека на VAD, STT, TTS и
   wake-word, проверена в Tauri на Windows, не требует CMake/CUDA на CI. Промт §10 допускает
   «whisper.cpp или совместимое решение».
5. **Браузер — открытый вопрос.** Промт §9 требует Playwright, но в Rust-first приложении это
   означает Node-сайдкар с Playwright и собственным Chromium (большой установщик, второй runtime).
   Альтернатива — CDP из Rust (chromiumoxide) с отдельным профилем системного Chrome/Edge:
   меньше, быстрее, но без accessibility-локаторов Playwright «из коробки». Решение за Артемом.
6. **Чего нет ни у кого:** Integration SDK (§12), голос + агент + разрешения вместе, честный
   cross-platform. Это и есть наша добавленная ценность; аналоги подтверждают, что по отдельности
   каждый кусок делается.

## 5. Открытые решения для Артема

1. Браузер: Playwright через Node-сайдкар (по промту) или CDP из Rust (chromiumoxide)?
2. Голос: согласен ли на sherpa-onnx вместо whisper.cpp напрямую?
3. LLM-слой в Rust: свои адаптеры с нуля или поверх `rig`?

## 6. Вариант «Hermes Agent как основа» (проверено 15.10.2026)

Источник: github.com/NousResearch/hermes-agent (MIT), документация hermes-agent.nousresearch.com.
Метаданные: 254 081★, 55 171 форков, 100+ контрибьюторов, релизы v0.22.0–v0.22.2 за 14–15.10.2026
(проект очень активный, API меняется часто).

| Требование мастер-промта | Что есть в Hermes Agent / Hermes Desktop | Статус |
|---|---|---|
| Desktop-приложение Windows/macOS (§2.4) | **Hermes Desktop**: Electron + React 19 (`apps/desktop`), чат со стримингом и активностью инструментов, боковые панели (файлы, терминал, cron, skills, память, мессенджеры), настройки провайдеров/моделей/инструментов/MCP, трей, HUD-панель поверх окон, Quick Entry по глобальной горячей клавише, самообновление. Установщики NSIS/MSI, DMG | есть, но **Electron, не Tauri** |
| Нативный Windows (§2.4) | `install.ps1`, установка в `%LOCALAPPDATA%\hermes`, gateway как задача Планировщика (ONLOGON, `pythonw.exe`), CLI/TUI/gateway/cron/MCP/браузер/дашборд работают нативно. Терминал — через Git Bash | есть |
| Гибридный AI, local-first (§2.1, §4, §11) | Ollama, LM Studio, llama-server локально; OpenAI, Anthropic, Gemini, OpenRouter и др.; fallback-провайдеры, provider routing | есть |
| Agent Runtime, инструменты, лимиты (§7) | ядро Hermes: цикл инструментов, toolsets, память, skills, delegation, MCP (stdio/HTTP) | есть |
| Разрешения и подтверждения (§7, §13) | `approvals.mode: smart/manual/off`, ответы once/session/always/deny, `approvals.deny` (glob, действует даже в YOLO), неотключаемый hardline-blocklist, `HERMES_WRITE_SAFE_ROOT`, запрещённые пути (`~/.ssh`, `.env`); плагин-хук `pre_tool_call` может **block / modify / approve** (fail-closed по таймауту) | есть; наши уровни Read-only/Normal/Confirm/Blocked реализуются плагином `pre_tool_call` |
| Управление ПК (§8) | `terminal`, `read_file`, `patch`, `process`; **computer-use**: на Windows UIAutomation + `SendInput`/`PostMessage` без захвата фокуса, accessibility-дерево окон, драйвер cua-driver ставится вместе с Hermes | есть (драйвер сторонний, cua.ai) |
| Браузер (§9) | встроенный browser tool: Chromium через Node (CDP), снапшоты, vision, отдельный/реальный профиль, website blocklist, SSRF-защита | есть (не Playwright, но функционально то же) |
| Голос (§10) | STT: local Faster-Whisper (Windows x64 поддержан, без ключа, `stt.language: ru`), live-транскрипция; TTS: Piper/NeuTTS/KittenTTS локально, Edge/OpenAI/ElevenLabs облако, стриминг; voice mode в CLI/TUI/Desktop, push-to-talk, barge-in, фильтр галлюцинаций; wake word: openWakeWord («hey hermes», Windows x64), sherpa, Porcupine (встроенное слово «jarvis») | есть; русская фраза активации — через sherpa/Porcupine custom или своя `.tflite` |
| Локальный API для своего UI (§5, если UI делать свой) | `api_server` на `127.0.0.1:8642`, Bearer `API_SERVER_KEY`: OpenAI-совместимый `/v1/chat/completions` и `/v1/responses` со SSE, `/v1/runs` + `/events` (SSE) + `/approval` + `/steer` + `/stop`, сессии `/api/sessions/*/chat/stream`; Desktop ходит в gateway по `/api/ws` | есть |
| Хранение (§14) | SQLite-состояние (`hermes_state_*`), сессии, FTS-поиск, чекпоинты и откат | есть |
| Секреты (§13) | `.env` 600, credential vault/pools, фильтрация KEY/TOKEN/SECRET из окружения инструментов | есть, но не Credential Manager/Keychain |
| Интеграции (§12) | 25+ платформ gateway (Telegram, Discord, Slack, WhatsApp, Signal, Email, Home Assistant…), webhooks, plugin catalog, skills (SKILL.md) | есть, шире промта |
| Cron, память, дашборд | cron, memory providers, web dashboard (FastAPI + React, :9119) с плагинными вкладками | есть, сверх промта |

**Чего нет или что не совпадает с промтом:** стек Electron + Python вместо Tauri + Rust; терминал на
Windows — Git Bash, не PowerShell (PowerShell вызывается из bash); `smart`-режим одобрений решает
LLM (для «полного доступа к ПК» ставить `manual` + `approvals.deny`); вкладка Chat дашборда на нативном
Windows требует WSL2; desktop-плагины не изолированы; очень быстрый темп релизов — обновления могут
ломать конфиг; computer-use опирается на сторонний драйвер cua-driver.

**Наш опыт (11–14.10.2026, Mac):** Telegram, голосовые сообщения (Faster-Whisper ru + Piper ru), озвучка через
хук, cron-триаж FreeScout, SSH-инструменты к ПК и серверу — всё заработало за три дня; упёрлись только в
16 ГБ памяти Mac. Skills и конфиг из `archive/hermes-agent/` переносятся на ПК без переписывания.

**Варианты:**
- **A. Hermes Desktop как есть + наша надстройка.** Установить Hermes нативно на ПК, Ollama на 3080 Ti,
  настроить toolsets/approvals/голос на русском, писать только: плагин разрешений (`pre_tool_call`),
  skills и интеграции Deka, свои вкладки дашборда/HUD. Срок до рабочего помощника — дни, не месяцы.
- **B. Свой тонкий UI (Tauri) поверх Hermes gateway** через `api_server` (`/v1/runs` + SSE + `/approval`).
  Нужен, только если UI Hermes Desktop не устроит. Мозг, инструменты и голос — всё равно Hermes.
- **C. С нуля по мастер-промту** (Tauri + Rust): полный контроль и «чистый» стек, но это повторение
  того, что в Hermes уже сделано сотней контрибьюторов; реалистичный срок до уровня Hermes — месяцы.
