#!/usr/bin/env bash
# hermesctl — drive the Hermes Assistant control API on the main PC from this
# server through an SSH tunnel. The API listens on 127.0.0.1:8643 inside the
# PC only; nothing is opened on the network.
#
#   tools/hermesctl.sh health
#   tools/hermesctl.sh info | status | plan | events [after] | logs [tail]
#   tools/hermesctl.sh setup qwen3.5 [autostart=true]   # starts the first-run setup
#   tools/hermesctl.sh cancel | gateway-start | gateway-stop
#   tools/hermesctl.sh cli config get model.default      # any `hermes ...` command
#   tools/hermesctl.sh config-get | config-put <file> | env
#   tools/hermesctl.sh tickets                           # FreeScout: mine + unassigned
#   tools/hermesctl.sh keys | keys-presets | key-set NAME value | key-del NAME | gateway-restart
#   tools/hermesctl.sh raw GET /status                   # any endpoint
#
# Env: HERMES_PC (ssh host, default main-pc), HERMES_PC_USER_PROFILE
# (default C:/Users/Koava), HERMES_CTL_LOCAL_PORT (default 18643).
set -euo pipefail

HOST="${HERMES_PC:-main-pc}"
PROFILE="${HERMES_PC_USER_PROFILE:-C:/Users/Koava}"
LPORT="${HERMES_CTL_LOCAL_PORT:-18643}"
RPORT=8643
BASE="http://127.0.0.1:${LPORT}/control/v1"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TOKEN_FILE="${ROOT}/private/control.token"
REMOTE_TOKEN="${PROFILE}/AppData/Local/ua.koavart.hermes/control.token"

die() { echo "hermesctl: $*" >&2; exit 1; }

tunnel_up() {
  if ! (exec 3<>"/dev/tcp/127.0.0.1/${LPORT}") 2>/dev/null; then
    ssh -f -N -o ExitOnForwardFailure=yes -o ConnectTimeout=10 \
      -L "${LPORT}:127.0.0.1:${RPORT}" "${HOST}" \
      || die "cannot open SSH tunnel to ${HOST}"
  fi
}

token() {
  if [[ ! -s "${TOKEN_FILE}" ]]; then
    mkdir -p "$(dirname "${TOKEN_FILE}")"
    ssh -o ConnectTimeout=10 "${HOST}" \
      "powershell -NoProfile -Command \"Get-Content -Raw '${REMOTE_TOKEN//\//\\}'\"" 2>/dev/null \
      | tr -d '\r\n' > "${TOKEN_FILE}" || true
    [[ -s "${TOKEN_FILE}" ]] || die "cannot read ${REMOTE_TOKEN} on ${HOST} (is the app installed and started once?)"
    chmod 600 "${TOKEN_FILE}"
  fi
  cat "${TOKEN_FILE}"
}

call() { # METHOD PATH [BODY] [CONTENT-TYPE]
  local method="$1" path="$2" body="${3:-}" ctype="${4:-application/json}"
  tunnel_up
  local args=(-sS --max-time 900 -X "${method}" -H "Authorization: Bearer $(token)" "${BASE}${path}")
  if [[ -n "${body}" ]]; then args+=(-H "Content-Type: ${ctype}" --data-binary "${body}"); fi
  local out code
  out="$(curl "${args[@]}" -w $'\n%{http_code}')" || die "request failed"
  code="${out##*$'\n'}"; out="${out%$'\n'*}"
  if [[ "${code}" == 401 ]]; then rm -f "${TOKEN_FILE}"; die "unauthorized (token cache cleared, retry)"; fi
  printf '%s\n' "${out}"
  [[ "${code}" =~ ^2 ]] || die "HTTP ${code}"
}

cmd="${1:-health}"; shift || true
case "${cmd}" in
  health)        tunnel_up; curl -sS "${BASE}/health"; echo ;;
  info)          call GET /info ;;
  status)        call GET /status ;;
  plan)          call GET /setup/plan ;;
  events)        call GET "/setup/events?after=${1:-0}&limit=${2:-500}" ;;
  logs)          call GET "/logs?tail=${1:-200}" ;;
  setup)         model="${1:?model tag}"; auto="${2:-true}"
                 call POST /setup/start "{\"model\":\"${model}\",\"consent\":true,\"autostart\":${auto}}" ;;
  cancel)        call POST /setup/cancel ;;
  gateway-start) call POST /gateway/start ;;
  gateway-stop)  call POST /gateway/stop ;;
  cli)           [[ $# -gt 0 ]] || die "cli needs hermes arguments"
                 args="$(printf '%s\n' "$@" | python3 -c 'import json,sys; print(json.dumps([l.rstrip("\n") for l in sys.stdin]))')"
                 call POST /hermes/cli "{\"args\":${args}}" ;;
  config-get)    call GET /hermes/config ;;
  config-put)    call PUT /hermes/config "@${1:?file}" text/plain ;;
  env)           call GET /hermes/env ;;
  keys)          call GET /hermes/env/keys ;;
  keys-presets)  call GET /hermes/env/presets ;;
  key-set)       n="${1:?NAME}"; shift; body="$(printf '%s' "$*" | python3 -c 'import json,sys; print(json.dumps({"name": sys.argv[1], "value": sys.stdin.read()}))' "$n")"
                 call PUT /hermes/env/keys "$body" ;;
  key-del)       call DELETE "/hermes/env/keys/${1:?NAME}" ;;
  gateway-restart) call POST /gateway/restart ;;
  tickets)       call GET /freescout/tickets ;;
  chat-sessions) call GET /chat/sessions ;;
  chat-new)      body="$(printf '%s' "${1:-}" | python3 -c 'import json,sys; t=sys.stdin.read(); print(json.dumps({"title": t or None}))')"
                 call POST /chat/sessions "${body}" ;;
  chat-messages) call GET "/chat/sessions/${1:?session id}/messages" ;;
  chat-send)     sid="${1:?session id}"; shift; msg="$(printf '%s' "$*" | python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()))')"
                 call POST /chat/send "{\"session_id\":\"${sid}\",\"message\":${msg}}" ;;
  chat-events)   call GET "/chat/events?after=${1:-0}&limit=${2:-500}" ;;
  chat-stop)     call POST /chat/stop "{\"session_id\":\"${1:?session id}\"}" ;;
  chat-approve)  call POST /chat/approve "{\"run_id\":\"${1:?run id}\",\"choice\":\"${2:?choice}\"}" ;;
  voice-status)  call GET /voice/status ;;
  voice-setup)   call POST /voice/setup ;;
  voice-restart) call POST /voice/restart ;;
  voice-speak)   msg="$(printf '%s' "$*" | python3 -c 'import json,sys; print(json.dumps({"text": sys.stdin.read()}))')"
                 call POST /voice/speak "${msg}" ;;
  voice-transcribe) f="${1:?wav file}"; body="$(python3 -c 'import base64,json,sys; print(json.dumps({"audio_base64": base64.b64encode(open(sys.argv[1],"rb").read()).decode(), "mime_type": "audio/wav"}))' "$f")"
                 call POST /voice/transcribe "${body}" ;;
  raw)           call "${1:?METHOD}" "${2:?PATH}" "${3:-}" ;;
  *)             die "unknown command: ${cmd}" ;;
esac
