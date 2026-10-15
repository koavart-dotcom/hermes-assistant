# Конфигурация Hermes Agent на Mac

Живой конфиг: `~/.hermes/config.yaml` на Mac (пользователь `deka`), секреты — `~/.hermes/.env`.
Здесь храним только то, что отличается от дефолта, и список решений. Ставится скриптом
`scripts/install-mac.sh`, точечно — `hermes config set <section.key> <value>`.

## Установлено 14.10.2026
- Hermes Agent v0.22.0 (main @ 0e427d7), путь `~/.hermes/hermes-agent`, свой uv + Python.
- LM Studio (brew cask), onboarding в GUI ещё не пройден.

## Отличия от дефолта
| Ключ | Значение | Зачем |
|---|---|---|
| `model.provider` | `custom` | основная модель — локальный Jarvis Deka (OpenAI-совместимый llama.cpp), решение Артема 14.10.2026 |
| `model.base_url` | `http://172.15.15.125:8000/v1` | адрес Jarvis; в `.env` продублирован как `OPENAI_BASE_URL`, `OPENAI_API_KEY=jarvis-local` (заглушка, сервер без auth) |
| `model.default` | `qwen3.8-27b` | Qwen3.8-27B Q4_K_XL, контекст 98k, vision, tool calling проверен |
| `model.context_length` | `98304` | n_ctx сервера |
| `auxiliary.*.provider` | `main` | compression, title_generation, web_extract, session_search, background_review, vision — всё через Jarvis |
| `stt.language` | `ru` | голосовые сообщения на русском |
| `stt.provider` | `local` | обязательно явно: с `OPENAI_API_KEY`-заглушкой автодетект уходил в OpenAI Whisper и падал с 401 |
| `stt.local.model` | `small` | faster-whisper локально (extra `voice`: `hermes pm install --extra voice`); `base` плохо знает русский; проверено на голосовом из Telegram, ~12 с с загрузкой модели |
| `tts.provider` | `piper` | локальный синтез (`hermes tools post-setup piper`); Edge TTS Microsoft отдавал 403 |
| `tts.piper.voice` | `ru_RU-dmitri-medium` | русский голос, модель в `~/.hermes/cache/piper-voices/`; альтернативы ru_RU-irina/denis/ruslan |

## Запланировано
- Облако (Anthropic, `claude-opus-5-5`) — как fallback или для длинных цепочек с доступом к ПК, когда появится ключ: `hermes fallback`.
- LM Studio на самом Mac пока не нужен, раз есть Jarvis; оставляем как офлайн-вариант (`docs/models.md`).
- `platform_toolsets.telegram` — урезать до нужного (без browser / image_gen), когда появятся свои инструменты.
- ~~Telegram~~ — сделано 14.10.2026: бот `@Ai_agent_deka_bot`, в `.env` `TELEGRAM_BOT_TOKEN` и `TELEGRAM_ALLOWED_USERS=332684086`; шлюз — служба launchd `ai.hermes.gateway` (`hermes gateway status|restart`), логи `~/.hermes/logs/gateway.log`.
- Список опасных действий и правила подтверждения — `config/dangerous-tools.md` (14.10.2026). Продублированы в skill `skills/remote-hosts` (на Mac: `~/.hermes/skills/deka/remote-hosts/`).
- SSH-доступ агента: ключ `~/.ssh/id_ed25519_hermes` на Mac, алиасы `linux` и `main-pc` — оба работают (14.10.2026), `wakeonlan` для ПК (MAC в `private/mac.md`; пробуждение из сна ещё не проверено).

## Хелпдеск (L0, 14.10.2026)
- Ключ: `FREESCOUT_API_KEY`, `FREESCOUT_URL` в `~/.hermes/.env` (cron-скрипты env Hermes не наследуют, скрипт читает `.env` сам).
- Источник: `scripts/hermes/helpdesk_new.py` → на Mac `~/.hermes/scripts/helpdesk_new.py` (monitor-script: одинаковый вывод не будит агента).
  С 14.10.2026 (решение Артема) показывает **только ящик IT Support (support@deka.ua, mailboxId 1)**: `sidebar` — счётчики как в
  боковой панели FreeScout (Неназначенные/Мои/Черновики/Назначенные/Спам/DBA, из `GET /api/mailboxes/1/folders`, activeCount) и
  `tickets` — без исполнителя + на Artem Kovalenko (FreeScout user id 4, переопределяется `FREESCOUT_MY_USER_ID` в `.env`), активные и
  ожидающие, с пометкой новая/обновлена за 5 ч/в работе. **Корзина исключена** (API отдаёт `state=deleted` вместе с живыми — из-за этого
  раньше «без исполнителя» было 31 вместо 1). Другие ящики — только по команде: `--mailbox 2|3|5|all`. Общий режим — флаг `--all`.
- Skill: `skills/helpdesk-triage` → `~/.hermes/skills/deka/helpdesk-triage/`. Только чтение, формат сводки, категории, срочность.
- Cron Hermes `helpdesk-triage`: `0 9,14,17 * * 1-5` (3 раза в день, с 14.10.2026), `--script helpdesk_new.py` (сводка каждый запуск,
  не только при изменениях), `--deliver telegram:332684086`, `--continuity` (видит прошлый вывод, новое помечает).
  Управление: `hermes cron list|run|pause|resume helpdesk-triage`, история `hermes cron runs`.

## Голос на Mac через плазму (14.10.2026)
- Хук `post_llm_call` → `~/.hermes/agent-hooks/speak.py` (исходник `scripts/hermes/agent-hooks/speak.py`): Piper ru_RU-dmitri из venv Hermes → `afplay`,
  запасной вариант `say -v Milena`. Озвучивает Telegram и CLI, cron — нет; первые ~700 символов, без кода/ссылок/разметки.
- В `config.yaml`: блок `hooks:` + `hooks_auto_accept: true` (шлюз не может спросить согласие интерактивно).
- Выключатель: файл `~/.hermes/agent-hooks/speak.off`. Skill `skills/voice-output` учит агента реагировать на «замолчи»/«говори».
- Звук идёт на устройство вывода macOS по умолчанию (сейчас LG TV по HDMI).
