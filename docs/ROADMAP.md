# Roadmap — Hermes (вариант B)

Обновлено: 15.10.2026. Статусы: ✅ сделано · 🔄 в работе · ⬜ запланировано · ❌ отменено.
Правило: функция считается сделанной только после реальной проверки на ПК Артема (промт §22).

| Этап | Содержание | Результат | Статус |
|---|---|---|---|
| 0 | Исследование: аналоги, библиотеки, Hermes Agent (`docs/OPEN_SOURCE_RESEARCH.md`); выбор варианта B; ADR | решение принято | ✅ 15.10 |
| 1 | Каркас: Tauri 2 + React + TS + Rust (по образцу dannysmith/tauri-template), Tailwind + shadcn/ui, Zustand, навигация, темы, lint/typecheck/vitest/cargo test, ts-rs типы, CI (проверки на push, Windows-сборка на тег), Linux-сборка для разработки | приложение открывается, пустые экраны Chat / Approvals / Voice / Status / Settings, CI зелёный | 🔄 15.10: каркас, экраны, темы, тесты; CI зелёный; релиз v0.1.0 (.exe 1,5 МБ / .msi 2,2 МБ) собран в GitHub Actions; осталось проверить запуск на ПК |
| 2 | Мастер первого запуска: бутстрап Hermes по стадийному протоколу `install.ps1` с прогрессом, pin версии; Ollama-сайдкар из zip с `OLLAMA_MODELS` на D:; загрузка LLM/STT/TTS моделей с согласия; генерация `config.yaml`/`.env`, `API_SERVER_KEY` в Credential Manager; супервизор gateway (start/stop/status/ensure); экран Status | на чистом ПК после одного установщика Hermes и Ollama работают, статус зелёный | ⬜ |
| 3 | Чат: `POST /v1/runs` + SSE события (токены, reasoning, прогресс инструментов), стоп, сессии (Sessions API), Markdown + код, история | текстовый диалог с локальной моделью, видны вызовы инструментов | ⬜ |
| 4 | Разрешения: событие `approval.request` → окно подтверждения → `/approval` (once/session/always/deny); плагин `hermes_permissions` (pre_tool_call, уровни Read-only/Normal/Confirm/Blocked, конфиг уровней по инструментам); Hermes `approvals.mode: manual`, `approvals.deny`, `HERMES_WRITE_SAFE_ROOT`; просмотр журнала действий; safe-mode | опасное действие не выполняется без «ОК» в нашем UI; тесты плагина | ⬜ |
| 5 | Голос: push-to-talk по глобальной горячей клавише, выбор микрофона/вывода, запись → транскрипция Hermes (Faster-Whisper, ru) → run → озвучка (Piper ru) с прерыванием; индикаторы; позже wake word | задача голосом → ответ голосом на ПК | ⬜ |
| 6 | Управление ПК: computer-use driver (UIAutomation), политика терминала (Git Bash → PowerShell), файлы в разрешённых папках, системная информация, скриншот; тестовые сценарии «открой приложение / найди файл / прочитай экран» | первый полезный сценарий на ПК с подтверждениями | ⬜ |
| 7 | Интеграции Deka: перенос skills и cron из `archive/hermes-agent` (FreeScout-триаж), Telegram-канал Hermes, Jarvis Deka как провайдер; ссылка на web-dashboard Hermes | сводки хелпдеска и Telegram работают с ПК | ⬜ |
| 8 | Production: установщик NSIS/MSI с бандлом (Tauri bundle + Ollama zip + плагин + skills), автообновление оболочки и обновление Hermes через мастер, документация USER_GUIDE/DEVELOPMENT/SECURITY, известные ограничения; macOS-сборка на Mac mini | воспроизводимая сборка, Артем ставит и пользуется | ⬜ |
| пауза | Умный дом (Home Assistant плагин Hermes), wake word по-русски, полностью офлайновый пакет | | ⬜ |

## Проверено / не проверено
- Проверено по документации и коду Hermes (15.10.2026): нативный Windows, стадийный протокол install.ps1,
  api_server (runs/events/approval), approvals/deny/safe-root, computer-use на Windows, voice (Faster-Whisper ru,
  Piper), wake-word движки, Ollama zip без установщика.
- Этап 1 (15.10.2026): lint/typecheck/vitest/vite build проходят локально на Linux; Rust-часть проверяется только в CI (на сервере нет места под toolchain); на ПК ещё ничего не запускалось.
