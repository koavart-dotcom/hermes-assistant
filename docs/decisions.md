# Решения (ADR)

## 2026-10-11 — Ядро: Hermes Agent, дом: Mac M4
- Ядро агента — Hermes Agent (Nous Research), не пишем свой агентный цикл.
- Агент живёт на Mac M4 постоянно; Windows-ПК и Linux-сервер — удалённые цели по SSH.
- Голос (wake-word → STT → TTS) — обязательный канал, по возможности локально на M4.
- Умный дом — через Home Assistant / MCP, а не напрямую в устройства.
- Личный проект, стек Python; корпоративный стек (PHP/Symfony/Nuxt) не применяется.

## 2026-10-14 — Основная модель: локальный Jarvis Deka вместо облака

**Контекст.** Hermes Agent установлен на Mac mini M4 16 ГБ. Планировали main-модель в облаке (Claude API),
локальную — только для auxiliary. У Deka уже работает локальный LLM-сервер «Джарвис»:
OpenAI-совместимый API на llama.cpp (llama-swap), модель Qwen3.8-27B Q4_K_XL, контекст 98k, vision.

**Решение (Артем).** Main-модель Hermes — Jarvis (`provider: custom`, `base_url` в `config/README.md`).
Auxiliary-слоты тоже на него. Проверено: tool calling корректный, ~59 ток/с, пробная задача через терминал
выполнена. Облако Claude — fallback (`hermes fallback`) и длинные цепочки с доступом к ПК, когда появится ключ.
LM Studio на Mac отложен: 16 ГБ против 27B-модели на сервере — не конкурент; остаётся офлайн-вариантом.

**Последствия.** Рабочие данные Deka не уходят в облако; агент зависит от доступности сервера Jarvis и сети
до него; при недоступности — нужен fallback. Блокер «ключ Anthropic» снят.

## 2026-10-15 — Смена концепта: Nexus AI Assistant на ПК Артема, Hermes заморожен

**Контекст.** Hermes Agent на Mac mini M4 16 ГБ работал (Telegram, голос, триаж хелпдеска), но
мощности Mac не хватает, а основной компьютер Артема (RTX 3080 Ti 12 ГБ, 32 ГБ ОЗУ) простаивает.
Артем подготовил мастер-промт `Nexus_AI_Assistant_Claude_Code_Master_Prompt.md` — полное ТЗ на
desktop-приложение-агент.

**Решение (Артем).** Делаем приложение на основном ПК по мастер-промту: Tauri 2 + React/TypeScript +
Rust, local-first, Ollama как локальный провайдер по умолчанию, облако по выбору, инструменты с
уровнями Read-only/Normal/Confirm/Blocked, SQLite. Hermes на Mac заморожен (служба не трогается,
файлы в `hermes/`). Первый сценарий — голосовой чат и действия на этом же ПК. В опросе Артем
сначала назвал C#/.NET, но присланный следом промт задаёт Tauri — действует промт.

**Последствия.** Старые `docs/concept.md`, `deka-scenarios.md`, схемы — история. Сборка под Windows
только на ПК (по SSH), на Linux — фронт и `cargo check`. Ничего не ставить на ПК без согласия Артема
(автоустановка Ollama 15.10 отклонена). Jarvis Deka остаётся вторым «локальным» провайдером через
OpenAI-совместимый адаптер.

## 2026-10-15 — Имя Hermes, GitHub, на ПК только готовое приложение
- Имя помощника — **Hermes** (в мастер-промте «Nexus»). Архив Hermes Agent переименован в `archive/hermes-agent/`.
- Репозиторий — GitHub `koavart-dotcom/hermes-assistant` (private), ветка `main`.
- На основной ПК не ставятся инструменты разработки: Windows-сборку делает GitHub Actions, на ПК попадают
  только установщик Hermes и Ollama. Разработка и Linux-сборка — на этом сервере.

## 2026-10-15 — Вариант B: своё Tauri-приложение поверх Hermes Agent, один установщик

**Контекст.** Исследование (`docs/OPEN_SOURCE_RESEARCH.md`): аналоги из промта слабые, а Hermes Agent
(254k★, MIT, нативный Windows, Hermes Desktop на Electron, computer-use, браузер, голос ru, api_server
со стримингом и одобрениями, cron, Telegram, skills, MCP) закрывает ~90 % мастер-промта.
Рассмотрены: A — Hermes Desktop как есть + надстройка; B — свой UI на Tauri поверх gateway Hermes;
C — всё с нуля по промту.

**Решение (Артем).** B: своё приложение «Hermes» на Tauri 2 + React + Rust, которое ставится одним
установщиком вместе с Hermes Agent и Ollama; пользователь ничего не ставит отдельно. Мозг, инструменты,
голосовые движки, интеграции — Hermes Agent; наш код — оболочка (UI, супервизор, бутстрап, аудио,
секреты), плагин разрешений для Hermes и skills.

**Последствия.** Агентный цикл на Python (Hermes), а не на Rust — осознанное отклонение от промта §2.4/§7.
Браузер — встроенный Hermes (CDP), не Playwright. Зависимость от темпа релизов Hermes: версию фиксируем
(pin commit в бутстрапе), обновления — через наш мастер. Первый запуск требует интернета (скачивание
Hermes-окружения, Ollama и моделей с согласия). Образец бутстрапа — `apps/bootstrap-installer` Hermes (Tauri, MIT).

## 2026-10-16 — Продукт «Hermes Assistant»: папка приложения отделена от дома Hermes Agent

**Контекст.** NSIS Tauri ставит приложение в `%LOCALAPPDATA%\<productName>`, а Hermes Agent живёт в
`%LOCALAPPDATA%\hermes` (`HERMES_HOME`, checkout `hermes-agent\`, `bin\`, `config.yaml`, `.env`).
С productName «Hermes» это одна и та же папка (пути Windows не различают регистр), туда же попадал бы
и фолбэк данных оболочки `app_data_dir`. Замечено 16.10 при проверке ПК перед первым прогоном мастера.

**Решение.** productName → **«Hermes Assistant»** (каталог `%LOCALAPPDATA%\Hermes Assistant`),
`mainBinaryName` → `hermes-assistant`; данные оболочки → `%LOCALAPPDATA%\ua.koavart.hermes`
(как `app_local_data_dir` Tauri, там же логи). Дом Hermes Agent не трогаем — его CLI, `hermes doctor`,
`hermes update` и задача Планировщика рассчитаны на стандартный путь. Старая запись «Hermes» 0.1.0/0.2.0
удаляется хуком NSIS `NSIS_HOOK_PREINSTALL` (`src-tauri/windows/hooks.nsi`): запускает старый
`uninstall.exe /S`, если он есть. Имя окна и помощника остаётся «Hermes».

**Последствия.** Установщик называется `Hermes Assistant_<v>_x64-setup.exe`; ключ реестра удаления —
новый. Тест `shell_data_dir_never_collides_with_hermes_home` охраняет разделение путей.

## 2026-10-16 — Control API: удалённое управление оболочкой через loopback + SSH-туннель

**Контекст.** Артем: «надо сразу к программе добавить API, чтоб ты сам мог заходить и настраивать».
Отладка этапа 2 шла через его скриншоты; журнал приложения был пуст; по SSH на ПК заходим как
`claude`, а приложение работает у `Koava`, поэтому из SSH-сессии не достать ни HKCU, ни задачи
Планировщика Koava.

**Решение.** В приложение встроен HTTP-сервер (axum) на `127.0.0.1:8643` с Bearer-токеном
(`control.token` в данных оболочки): состояние, план/запуск/отмена установки, воспроизводимый поток
событий, gateway start/stop, произвольные команды `hermes` CLI, чтение/запись `config.yaml`, `.env`
с маскировкой, хвост журнала. Общее состояние для UI и API — `hermes_core::supervisor` (Tauri-команды
стали тонкими). С сервера — `tools/hermesctl.sh` через `ssh -L`. Порт наружу не открывается;
`/health` без авторизации. Описание — `docs/CONTROL_API.md`.

**Последствия.** Любой процесс под Koava с токеном получает Hermes CLI — токен не копировать в git
(кэш в `private/`). Пока приложение закрыто, API недоступен (трей — позже). События установки
теперь зеркалятся в файл журнала. Этап 2 отлаживается без участия Артема: запуск мастера и чтение
ошибок — через API.

## 2026-10-16 — Порт Ollama выбирается динамически (11434 занят VS Code)

**Контекст.** Первый полный прогон через Control API: `ollama serve` «не стал готов за 20 с». На ПК порт
11434 слушает VS Code (pid «Code», расширение для локальных моделей): принимает соединения и не отвечает,
поэтому наш сервер не мог занять порт, а проверка готовности зависала на чужом слушателе. Вывод Ollama
не сохранялся — причину пришлось искать пробами по SSH.

**Решение.** `Ollama::is_running` считает сервер живым только по настоящему `/api/version`;
`Ollama::ensure_port` перед настройкой Hermes берёт первый свободный порт из 11434..11443, запоминает
его в `<app_data_dir>/ollama-port` и передаёт в `model.base_url` (на ПК вышло 11437). stdout/stderr
`ollama serve` — в `logs/ollama.log`, ожидание до 90 с, при раннем выходе ошибка с хвостом журнала.
Чтение вывода всех дочерних процессов — с потерями (`next_line_lossy`): uv на Windows пишет не UTF-8.

**Последствия.** Любой клиент Ollama на ПК должен брать адрес из конфига Hermes/`ollama-port`, а не
предполагать 11434. Автозапуск gateway — через папку «Автозагрузка» (`Hermes_Gateway.vbs`): задача
Планировщика требует UAC, которого в неинтерактивном режиме нет; поведение Hermes штатное.

## 2026-10-16 — Контекст Ollama 64k: иначе инструменты отрезаются

**Контекст.** Первый чат через приложение: Qwen 3.5 отвечала, но на «посмотри файлы на рабочем столе»
написала команду текстом вместо вызова инструмента. Журнал Ollama: `truncating input prompt
prompt=13584 → 2050`. Hermes шлёт ~14 тыс. токенов системного промпта и схем 21 инструмента, а Ollama
по умолчанию держит контекст 4096 — схемы инструментов обрезались. Напрямую через `/v1/chat/completions`
с одним инструментом модель вызывает его корректно, то есть дело не в модели.

**Решение.** Сайдкар запускается с `OLLAMA_CONTEXT_LENGTH=65536` (минимум по документации Hermes для
агентной работы) и `OLLAMA_FLASH_ATTENTION=1`. Проверено на ПК: qwen3.5 с контекстом 65536 занимает
7,3 ГБ VRAM целиком на GPU (гибридное внимание, KV-кэш мал). Для более тяжёлых моделей (gemma4:12b,
qwen3:14b) контекст и VRAM проверять отдельно; при нехватке Ollama выгрузит слои на CPU.

**Последствия.** Любая модель в мастере должна оцениваться с учётом 64k контекста, а не только
размера весов (`docs/models.md`). Изменение env требует перезапуска `ollama serve` — приложение
делает это само при старте следующей версии (процесс принадлежит приложению).

## 2026-10-16 — Этап 4: уровни разрешений как плагин Hermes на `pre_tool_call`

**Контекст.** Промт §7 требует уровни Read-only / Normal / Confirm / Blocked, проверяемые не в
системном промпте. Hermes v0.22.2 даёт хук `pre_tool_call(tool_name, args, task_id)` с ответами
`block` / `approve` (уходит в штатный approval gate, который api_server доставляет нашему чату как
`approval.request`, а «always» запоминается по `rule_key`) / `modify`. Встроенная проверка Hermes
покрывает только опасные shell-команды; `approvals.unattended_mode: deny` не мешает — для api_server
с клиентом, умеющим отвечать, запрос доходит.

**Решение.** Плагин `hermes-plugins/hermes_permissions` (stdlib, `plugin.yaml` + `__init__.py`):
терминал — чёрный список регулярок (shutdown, format, diskpart, `rm -rf /`, `reg delete HKLM`,
`curl | sh`…) → block; команды только на чтение (`ls`, `dir`, `Get-ChildItem`, `git status`,
`reg query`… без перенаправлений) → allow; остальное → confirm с `rule_key = hp:terminal:<программа>`;
`write_file`/`patch` — allow внутри корней (профиль, D:\, HERMES_HOME), confirm вне, block для
`.ssh`, `.env`, credential-хранилищ; остальные инструменты по таблице (browser_cdp/vault — block,
execute_code/computer_use/process_manage — confirm, чтение и поиск — allow), неизвестные — confirm.
Переопределение — `$HERMES_HOME/hermes_permissions.json`, читается на каждом вызове; журнал —
`hermes_permissions.log` (JSON lines). Ошибка плагина → confirm (fail-closed). Плагин едет в ресурсах
приложения, копируется мастером и при каждом старте (`plugins_ensure`), включается `hermes plugins
enable`, при изменении файлов gateway перезапускается.

**Последствия.** Безопасность не зависит от промпта и от модели. Цена — лишние подтверждения для
новых инструментов (default confirm), регулируется политикой. `HERMES_WRITE_SAFE_ROOT` как второй
рубеж — позже, когда станет ясно, какие папки реально нужны.

## 2026-10-16 — Этап 5: голос через веб-сервер Hermes, Whisper локально, Edge TTS

**Контекст.** В v0.22.2 api_server не имеет аудио-эндпоинтов (`audio_api: false`); они есть у веб-сервера
Hermes (`hermes dashboard`): `POST /api/audio/transcribe` (`{data_url}`) и `POST /api/audio/speak`
(`{text}` → `data_url`), авторизация заголовком `X-Hermes-Session-Token`, токен задаётся env
`HERMES_DASHBOARD_SESSION_TOKEN`. Окружение Python Hermes управляется `pm` (хэшированные каталоги), прямые
пути к venv ненадёжны.

**Решение.** Второй супервизируемый процесс `hermes dashboard --no-open --host 127.0.0.1 --port 8644`
с нашим токеном (`dashboard.token` в данных оболочки), вывод в `logs/dashboard.log`. Зависимости —
`hermes pm install --extra voice` (Faster-Whisper, sounddevice), маркер `voice-ready`. Настройки Hermes:
`stt.provider local`, `stt.local.model small`, `stt.local.language ru`, `tts.provider edge`,
`tts.edge.voice ru-RU-DmitryNeural` (ключ `stt.language` из этапа 2 был неверным, убран). Захват
микрофона в webview → WAV 16 кГц mono (свой кодировщик, без зависимостей от контейнеров на стороне Hermes),
транскрипт идёт обычным ходом чата, ответ озвучивается `speak` и играет через `<audio>`.

**Последствия.** Edge TTS требует интернета и не хранится локально — Piper (офлайн, `ru_RU`) следующий
шаг. Первый запуск веб-сервера может долго ставить fastapi/uvicorn через PM. Push-to-talk пока только в
окне (кнопка/пробел); глобальная горячая клавиша и прерывание озвучки — отдельно.
