#!/usr/bin/env bash
# Установка/обновление Hermes Agent и LM Studio на Mac (дом агента).
# Запуск с Linux-сервера: ssh mac 'bash -s' < scripts/install-mac.sh
# Повторный запуск безопасен (installer делает fetch + fast-forward).
# Первый раз выполнено вручную 14.10.2026, см. docs/decisions.md.
set -euo pipefail
export PATH="/opt/homebrew/bin:$HOME/.local/bin:$HOME/.lmstudio/bin:$PATH"

echo "== Hermes Agent =="
curl -fsSL https://hermes-agent.nousresearch.com/install.sh -o /tmp/hermes-install.sh
bash /tmp/hermes-install.sh --non-interactive
hermes --version

echo "== Базовые настройки Hermes (секреты — только в ~/.hermes/.env, руками) =="
hermes config set model.provider anthropic
hermes config set model.default claude-opus-5-5
hermes config set stt.language ru
hermes config set stt.local.model small

echo "== LM Studio (локальная модель для auxiliary-слотов) =="
brew list --cask lm-studio >/dev/null 2>&1 || brew install --cask lm-studio
# Первый запуск LM Studio — через GUI на самом Mac (onboarding), после этого:
#   ~/.lmstudio/bin/lms bootstrap && lms get <модель> && lms server start --port 1234

echo "== Проверка =="
hermes doctor | tail -20
