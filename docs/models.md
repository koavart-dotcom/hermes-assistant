# Локальные модели (ориентир на 11.10.2026; 15.10.2026 добавлен ПК)

Исполнение: **LM Studio** (MLX-бэкенд, OpenAI-совместимый сервер на :1234, Hermes Agent поддерживает
его как провайдера «из коробки»). Ollama — запасной вариант. Контекст для Hermes нужен ≥ 64k.
Цифры по памяти — из блогов, не из официальных карточек; перед выбором перепроверить и замерить.

| Память Mac | Основной кандидат | Альтернативы | Примечание |
|---|---|---|---|
| 16 GB | Gemma 4 E4B (~4.6 GB Q4) | Qwen3.5 9B (~6 GB Q4); gpt-oss-20b (~12–14 GB) только как эксперимент без голоса | Бюджет под LLM ≈ 8–9 GB после macOS, Hermes, STT и TTS; базовый M4 — 120 GB/s, dense 9B даёт ~20 ток/с |
| 24 GB | Gemma 4 26B-A4B (Q4) | gpt-oss-20b; Qwen3.6-35B-A3B Q4 впритык | Gemma 4 — vision + надёжный function calling |
| 32 GB | Qwen3.6-35B-A3B (Q4–Q5) | Gemma 4 31B dense; gpt-oss-20b | MoE (3B активных) → быстро даже на базовом M4 |
| 48–64 GB | Qwen3.6-35B-A3B Q8 или Gemma 4 31B Q6 | Qwen3-32B dense | На M4 Pro/Max dense-модели уже комфортны по скорости |
| 96–128 GB | gpt-oss-120b (Q5) | GLM-5.x Air, Kimi K2.6 (128+) | Класс «почти облако», только Mac Studio |
| **ПК: RTX 3080 Ti 12 GB VRAM + 32 GB ОЗУ** (15.10.2026, Ollama) | Qwen3.5 9B Q4 (~6 GB) целиком в VRAM, контекст 32k+ | dense 12–14B Q4 (~9 GB, контекст меньше); Gemma 4 26B-A4B / Qwen3.6-35B-A3B Q4 с выгрузкой экспертов в ОЗУ (эксперимент) | CUDA даёт в разы выше скорость, чем базовый M4; VRAM делят LLM + whisper (~1.5 GB). Модели хранить на D: |

Рассматриваемый Mac (11.10.2026): Mac mini M4 базовый, 10C CPU / 10C GPU, 16 GB, 512 GB SSD (MU9E3).
В объявлении написано «M4 Pro», но спецификация базового M4 — у Pro минимум 24 GB и 12C CPU.
Если ещё не куплен: для этого проекта лучше M4 24/32 GB или M4 Pro 24 GB (вдвое выше пропускная
способность памяти). 16 GB не расширяется.

Роли локальной модели в проекте:
- auxiliary-слоты Hermes (сжатие контекста, заголовки, vision, скоринг, роутинг MCP, поиск skills);
- разбор голосовых команд и короткие ответы голосом (низкая задержка, приватность);
- триаж заявок хелпдеска (рабочие данные не уходят в облако);
- офлайн-запасной вариант, когда API недоступен.

Голосовой контур на M4:
- STT: whisper-large-v3-turbo через MLX или Parakeet TDT 0.6B v3 (быстрее, русский есть);
- wake-word: openWakeWord (своё слово обучаемо) или Porcupine (бесплатно для личного);
- TTS по-русски: Silero TTS или Piper (голоса ru_RU), запасной — macOS `say` (Milena/Yuri).

Источники (блоги, 2026): vdf.ai/blog/best-local-llm-for-mac, openclawdc.com/blog/best-local-llms-32gb-ram,
knightli.com (таблица Qwen3.6), llmcheck.net (Qwen 3.6 vs Gemma 4), lmstudio.ai/docs/integrations/hermes,
hermes-agent.nousresearch.com/docs/user-guide/configuring-models.
