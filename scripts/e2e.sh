#!/usr/bin/env bash
# Start a throwaway OpenSSH server on 127.0.0.1:$PORT (default 2222) and run the end-to-end
# tests against it. Linux/macOS; needs `sshd` and permission to create a user
# (CI runs it as root in a container or with sudo).
set -euo pipefail
cd "$(dirname "$0")/.."
WORK="$(mktemp -d)"
trap 'kill "$(cat "$WORK/sshd.pid" 2>/dev/null)" 2>/dev/null || true; rm -rf "$WORK"' EXIT

PORT="${HOSTWICK_E2E_PORT:-2222}"
USER_NAME="${HOSTWICK_E2E_USER:-hwtest}"
PASSWORD="hw-test-pass-$RANDOM"
SUDO=""; [ "$(id -u)" -ne 0 ] && SUDO="sudo"

id "$USER_NAME" >/dev/null 2>&1 || $SUDO useradd -m -s /bin/bash "$USER_NAME"
echo "$USER_NAME:$PASSWORD" | $SUDO chpasswd
ssh-keygen -q -t ed25519 -N "" -f "$WORK/client_key"
ssh-keygen -q -t ed25519 -N "s3cret" -f "$WORK/client_key_enc"
ssh-keygen -q -t ed25519 -N "" -f "$WORK/host_key"
HOME_DIR="$(getent passwd "$USER_NAME" | cut -d: -f6)"
$SUDO mkdir -p "$HOME_DIR/.ssh"
cat "$WORK/client_key.pub" "$WORK/client_key_enc.pub" | $SUDO tee "$HOME_DIR/.ssh/authorized_keys" >/dev/null
$SUDO chown -R "$USER_NAME" "$HOME_DIR/.ssh"
$SUDO chmod 700 "$HOME_DIR/.ssh"; $SUDO chmod 600 "$HOME_DIR/.ssh/authorized_keys"
$SUDO mkdir -p /run/sshd

cat > "$WORK/sshd_config" <<CFG
Port ${PORT}
ListenAddress 127.0.0.1
HostKey $WORK/host_key
PidFile $WORK/sshd.pid
PasswordAuthentication yes
KbdInteractiveAuthentication no
PubkeyAuthentication yes
UsePAM no
AllowTcpForwarding yes
Subsystem sftp internal-sftp
CFG
$SUDO "$(command -v sshd)" -f "$WORK/sshd_config" -E "$WORK/sshd.log"
sleep 1

export HOSTWICK_E2E_PORT="$PORT" HOSTWICK_E2E_USER="$USER_NAME" HOSTWICK_E2E_PASSWORD="$PASSWORD" \
       HOSTWICK_E2E_KEY="$WORK/client_key" HOSTWICK_E2E_KEY_ENC="$WORK/client_key_enc" \
       HOSTWICK_E2E_KEY_ENC_PASS="s3cret"
cargo test -p hostwick-sftp --test e2e -- --nocapture
