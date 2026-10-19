#!/usr/bin/env bash
# Prepare a Linux host for Hermes read-only diagnostics (docs/DIAG.md). Run as root:
#
#   sudo bash linux-setup.sh '<public key from `hermesctl diag-key`>' <hermes-server-ip>
#
# Creates the user hermes-diag without a password and without sudo, adds it to the
# groups that may read the system journal (systemd-journal, adm), and authorises the
# Hermes service key only from the Hermes server, without forwarding or a terminal.
# What the service runs is a fixed catalogue (crates/hermes-core/src/diag.rs):
# uptime/free, df, du, ps, systemctl --failed, journalctl, last, ss.
set -euo pipefail

KEY="${1:?public key (ssh-ed25519 ...)}"
FROM="${2:?IP of the Hermes server}"
USER_NAME="hermes-diag"

[[ "$(id -u)" -eq 0 ]] || { echo "run as root" >&2; exit 1; }
[[ "$KEY" == ssh-ed25519\ * ]] || { echo "expected an ssh-ed25519 public key" >&2; exit 1; }
[[ "$FROM" =~ ^[0-9a-fA-F:.]+$ ]] || { echo "server address must be an IP" >&2; exit 1; }

if ! id "$USER_NAME" >/dev/null 2>&1; then
  useradd --system --create-home --shell /bin/bash --comment "Hermes read-only diagnostics" "$USER_NAME"
fi
passwd -l "$USER_NAME" >/dev/null
for g in systemd-journal adm; do
  getent group "$g" >/dev/null && usermod -aG "$g" "$USER_NAME"
done

home="$(getent passwd "$USER_NAME" | cut -d: -f6)"
install -d -m 700 -o "$USER_NAME" -g "$USER_NAME" "$home/.ssh"
opts="from=\"$FROM\",no-port-forwarding,no-agent-forwarding,no-X11-forwarding,no-pty"
printf '%s %s\n' "$opts" "$KEY" > "$home/.ssh/authorized_keys"
chown "$USER_NAME:$USER_NAME" "$home/.ssh/authorized_keys"
chmod 600 "$home/.ssh/authorized_keys"

echo "ok: $USER_NAME ready; groups: $(id -nG "$USER_NAME")"
echo "check from the Hermes server: hermesctl diag-run $(hostname)"
