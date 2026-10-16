# MCP-мост Hermes → Claude Code (подписка Pro)

Hermes на ПК получает инструмент `claude_task`: задача уходит на этот сервер, где её выполняет
Claude Code в неинтерактивном режиме (`claude -p`) под подпиской Артема, без ключа API.

- `server.py` — HTTP-MCP на `0.0.0.0:8650` (`POST /mcp`, JSON-RPC; `GET /health`), стандартная библиотека.
- `projects.json` — белый список проектов (имя → папка). Claude работает только внутри них
  (`--permission-mode acceptEdits`).
- `bridge.token` — создаётся при первом старте, в git не попадает; тот же токен прописывается в Hermes.
- `bridge.log` — журнал задач (JSON lines).
- Одна задача за раз; таймаут по умолчанию 25 мин, максимум 60.

## Установка на сервере (один раз, от пользователя admin3_1)

```bash
mkdir -p ~/.config/systemd/user
cp ~/projects/ai-agent/tools/mcp-claude-bridge/claude-bridge.service ~/.config/systemd/user/
systemctl --user daemon-reload && systemctl --user enable --now claude-bridge
systemctl --user status claude-bridge --no-pager | head -5
curl -s http://127.0.0.1:8650/health
```

## Регистрация в Hermes (на ПК, через Control API с сервера)

```bash
TOKEN=$(cat ~/projects/ai-agent/tools/mcp-claude-bridge/bridge.token)
tools/hermesctl.sh cli mcp add claude --url http://192.168.229.37:8650/mcp --auth header --connect-timeout 30
tools/hermesctl.sh cli config set mcp_servers.claude.headers.Authorization "Bearer $TOKEN"
tools/hermesctl.sh cli config set mcp_servers.claude.timeout 1800
tools/hermesctl.sh gateway-stop && tools/hermesctl.sh gateway-start
```

После этого в чате Hermes: «попроси Клода проверить, почему падает сборка в hermes-assistant».
Вызов `mcp_claude_claude_task` проходит через плагин разрешений как «требует подтверждения»
(неизвестный инструмент → confirm), пока его не внесут в политику.
