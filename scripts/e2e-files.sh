#!/usr/bin/env bash
# End-to-end tests for the file features: SFTP through sudo (with a
# password, NOPASSWD, wrong password, not in sudoers, requiretty), the SCP
# fallback for servers without SFTP, file properties (chmod, chown,
# symlinks, checksums, folder sizes) and two-way sync. Linux; needs root
# (creates users, sudoers rules and runs two sshd instances).
#
# Uses its own ports (default 2230 with SFTP, 2231 without), so it never
# touches the servers the other e2e scripts use.
#
#   scripts/e2e-files.sh               set up, run the tests, tear down
#   KEEP=1 scripts/e2e-files.sh setup  set up only; prints the env file to source
#   scripts/e2e-files.sh teardown      remove the users and sudoers rules
set -euo pipefail
cd "$(dirname "$0")/.."

MODE="${1:-test}"
PORT="${HOSTWICK_FILES_PORT:-2230}"
NOSFTP_PORT="${HOSTWICK_FILES_NOSFTP_PORT:-2231}"
WORK="${HOSTWICK_FILES_WORK:-$(mktemp -d)}"
SUDOERS=/etc/sudoers.d/hostwick-e2e-files
# hwfiles: sudo with password; hwfiles2: NOPASSWD; hwfiles3: no sudo;
# hwfiles4: sudo with requiretty.
USERS=(hwfiles hwfiles2 hwfiles3 hwfiles4)

stop() {
    for p in sshd sshd-nosftp; do
        [ -f "$WORK/$p.pid" ] && kill "$(cat "$WORK/$p.pid")" 2>/dev/null || true
    done
}
teardown() {
    stop
    rm -f "$SUDOERS"
    for u in "${USERS[@]}"; do
        id "$u" >/dev/null 2>&1 && userdel -r "$u" 2>/dev/null || true
    done
}
if [ "$MODE" = teardown ]; then
    teardown
    exit 0
fi
if [ "${KEEP:-0}" != 1 ]; then
    trap 'teardown; rm -rf "$WORK"' EXIT
fi

[ "$(id -u)" -eq 0 ] || { echo "run as root" >&2; exit 1; }
command -v sudo >/dev/null || { echo "sudo is not installed" >&2; exit 1; }
mkdir -p "$WORK"
chmod 755 "$WORK"

# --- users ------------------------------------------------------------------
PASS="hw-files-$RANDOM$RANDOM"
for u in "${USERS[@]}"; do
    id "$u" >/dev/null 2>&1 || useradd -m -s /bin/bash "$u"
    echo "$u:$PASS" | chpasswd
done
cat >"$SUDOERS" <<CFG
hwfiles ALL=(ALL:ALL) ALL
hwfiles2 ALL=(ALL:ALL) NOPASSWD: ALL
Defaults:hwfiles4 requiretty
hwfiles4 ALL=(ALL:ALL) NOPASSWD: ALL
CFG
chmod 440 "$SUDOERS"
visudo -cq -f "$SUDOERS"

ssh-keygen -q -t ed25519 -N "" -f "$WORK/host_key" <<<y >/dev/null
ssh-keygen -q -t ed25519 -N "" -f "$WORK/client_key" -C hwfiles <<<y >/dev/null
for u in "${USERS[@]}"; do
    h="$(getent passwd "$u" | cut -d: -f6)"
    mkdir -p "$h/.ssh"
    cp "$WORK/client_key.pub" "$h/.ssh/authorized_keys"
    chown -R "$u:$u" "$h/.ssh"
    chmod 700 "$h/.ssh"
    chmod 600 "$h/.ssh/authorized_keys"
done

# --- sshd -------------------------------------------------------------------
mkdir -p /run/sshd
common() {
    cat <<CFG
ListenAddress 127.0.0.1
HostKey $WORK/host_key
StrictModes no
UsePAM no
PasswordAuthentication no
KbdInteractiveAuthentication no
PubkeyAuthentication yes
AllowUsers ${USERS[*]}
CFG
}
{ echo "Port $PORT"; echo "PidFile $WORK/sshd.pid"; common
  echo "Subsystem sftp /usr/lib/openssh/sftp-server"; } >"$WORK/sshd_config"
# A server without SFTP (routers, appliances): SCP only.
{ echo "Port $NOSFTP_PORT"; echo "PidFile $WORK/sshd-nosftp.pid"; common; } >"$WORK/sshd-nosftp_config"
"$(command -v sshd)" -f "$WORK/sshd_config" -E "$WORK/sshd.log"
"$(command -v sshd)" -f "$WORK/sshd-nosftp_config" -E "$WORK/sshd-nosftp.log"
sleep 1

ENV="$WORK/env.sh"
cat >"$ENV" <<CFG
export HOSTWICK_FILES_PORT=$PORT HOSTWICK_FILES_NOSFTP_PORT=$NOSFTP_PORT
export HOSTWICK_FILES_KEY=$WORK/client_key HOSTWICK_FILES_PASSWORD=$PASS
export HOSTWICK_FILES_DIR=$WORK
CFG

if [ "$MODE" = setup ]; then
    echo "environment ready; source $ENV"
    exit 0
fi
# shellcheck disable=SC1090
. "$ENV"
cargo test -p hostwick-sftp --test files_e2e -- --nocapture --test-threads=2
cargo test -p hostwick-transfer --test files_e2e -- --nocapture --test-threads=2
