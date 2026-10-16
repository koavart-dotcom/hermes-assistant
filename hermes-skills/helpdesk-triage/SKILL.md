---
name: helpdesk-triage
description: "Хелпдеск Deka (FreeScout): заявки на Артеме и без исполнителя, переписка по заявке, черновик и отправка ответа. Данные берутся из приложения Hermes Assistant на этом ПК (Control API), а не прямыми запросами к FreeScout."
version: 2.0.0
author: Артем / Claude Code
license: MIT
metadata:
  hermes:
    tags: [helpdesk, freescout, triage, deka, support]
---

# Хелпдеск Deka через Hermes Assistant

Приложение Hermes Assistant на этом ПК уже умеет ходить в FreeScout (ключи в `.env`). Пользуйся его
локальным API, а не curl к helpdesk.deka.ua: один вызов, готовый JSON, без корзины и без дублей.

Базовый адрес `http://127.0.0.1:8643/control/v1`, токен — файл
`%LOCALAPPDATA%\ua.koavart.hermes\control.token` (пользователь Koava). Все вызовы — через `terminal`
(PowerShell), шаблон:

```powershell
$t = (Get-Content -Raw "$env:LOCALAPPDATA\ua.koavart.hermes\control.token").Trim()
Invoke-RestMethod -Headers @{Authorization="Bearer $t"} "http://127.0.0.1:8643/control/v1/freescout/tickets" | ConvertTo-Json -Depth 6
```

## Что спрашивают → что вызывать

| Вопрос | Вызов | Что в ответе |
|---|---|---|
| «что на мне», «что без исполнителя», «что нового в хелпдеске», «мои заявки» | `GET /freescout/tickets` | `summary` (готовая фраза), `mine[]`, `unassigned[]`: `number, subject, customer, mailbox, status, mark (new/updated/working), last_message, url` |
| «сводка», «доложи по заявкам» | `POST /digest/run` с телом `{"telegram": false}` | `text` — готовая сводка, её и отправляй как есть |
| «что в заявке N», «покажи переписку по N» | `GET /freescout/conversation/N` | `ticket` + `transcript` (переписка по порядку, без HTML) |
| «ответь на заявку N», «подготовь ответ на N» | `GET /freescout/conversation/N`, затем **сам** напиши черновик | черновик показать Артему, **не отправлять** |
| «отправляй», «отправь ответ» — только после того, как Артем увидел черновик | `POST /freescout/reply` с телом `{"number": N, "text": "…"}` | `conversation_id` |

Пример POST:
```powershell
$t = (Get-Content -Raw "$env:LOCALAPPDATA\ua.koavart.hermes\control.token").Trim()
$body = @{ number = 4226; text = "Текст ответа" } | ConvertTo-Json
Invoke-RestMethod -Method Post -ContentType "application/json; charset=utf-8" -Headers @{Authorization="Bearer $t"} -Body $body "http://127.0.0.1:8643/control/v1/freescout/reply"
```

## Как отвечать
- Коротко, по-русски, до 15 строк. Первая строка — `summary` из ответа. Дальше по одной строке на заявку:
  `🔴/🟡/⚪ #номер [ящик] тема — клиент · категория · первый шаг`. Пометки: 🆕 новая, ✉️ обновлена сегодня.
- Категории: сеть, ПК/оргтехника, 1С/учёт, доступы/учётки, принтеры, почта, разработка (BASA/BPM/сайт),
  маркетинг, прочее. Срочность: 🔴 не работает у многих / магазин / касса / 1С-сервер / «срочно»; 🟡 обычная; ⚪ низкая.
- Если заявок больше 15 — сгруппировать по категориям и показать счётчики.
- Не больше 2–3 вызовов инструментов на вопрос. Всегда говорить, за какой момент данные (`fetched_at`).

## Черновик ответа клиенту
Писать от имени Артема на языке переписки (украинский или русский), вежливо и по делу, 3–8 предложений,
без «Уважаемый» и без подписи. Если не хватает данных — задать клиенту уточняющие вопросы. Показать
черновик целиком и спросить «отправлять?».

## Правила (обязательны)
- Клиенту отправлять **только** после явного «отправляй» / «да, отправь» Артема в этом же разговоре и только
  тот текст, который он видел. Назначать, закрывать, менять статус и теги — нельзя (API приложения этого и не умеет).
- Персональные данные клиентов (телефоны, пароли из текста заявки) в сводки не переносить.
- Не делать выводов о технических причинах без данных: «похоже на…», «проверить…».
- Прямые запросы к `helpdesk.deka.ua/api` не делать: ключ хранится в приложении, у него уже есть всё нужное.
