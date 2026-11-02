#!/usr/bin/env bash
# End-to-end tests for the networking features: outbound proxies, X11 and
# agent forwarding, OpenSSH certificates, security keys held by an agent,
# and Kerberos (GSSAPI). Linux; needs root (creates a user, runs sshd, and
# optionally a KDC, Xvfb, tinyproxy and dante).
#
# Starts its own sshd on 127.0.0.1:$HOSTWICK_NET_PORT (default 2223), so it
# never touches the server `scripts/e2e.sh` uses. Pieces whose tools are
# missing (krb5kdc, Xvfb, tinyproxy, danted) are skipped, and so are the
# tests that need them.
#
#   scripts/e2e-net.sh               set up, run the tests, tear down
#   KEEP=1 scripts/e2e-net.sh setup  set up only; prints the env file to source
set -euo pipefail
cd "$(dirname "$0")/.."

MODE="${1:-test}"
PORT="${HOSTWICK_NET_PORT:-2223}"
# Same server, but X11 and agent forwarding are refused on this port.
RESTRICTED_PORT="${HOSTWICK_NET_RESTRICTED_PORT:-2226}"
HTTP_PROXY_PORT="${HOSTWICK_NET_HTTP_PROXY_PORT:-2224}"
SOCKS_PROXY_PORT="${HOSTWICK_NET_SOCKS_PROXY_PORT:-2225}"
KDC_PORT="${HOSTWICK_NET_KDC_PORT:-8888}"
XNUM="${HOSTWICK_NET_XDISPLAY:-98}"
USER_NAME="${HOSTWICK_NET_USER:-hwnet}"
PROXY_USER="${HOSTWICK_NET_PROXY_USER:-hwproxy}"
REALM="HOSTWICK.TEST"
WORK="${HOSTWICK_NET_WORK:-$(mktemp -d)}"
mkdir -p "$WORK"
chmod 755 "$WORK"

stop() {
    for p in sshd kdc xvfb tinyproxy danted; do
        [ -f "$WORK/$p.pid" ] && kill "$(cat "$WORK/$p.pid")" 2>/dev/null || true
    done
    [ -n "${SSH_AGENT_PID:-}" ] && kill "$SSH_AGENT_PID" 2>/dev/null || true
}
if [ "${KEEP:-0}" != 1 ]; then
    trap 'stop; rm -rf "$WORK"' EXIT
fi

[ "$(id -u)" -eq 0 ] || { echo "run as root" >&2; exit 1; }

# --- users ------------------------------------------------------------------
id "$USER_NAME" >/dev/null 2>&1 || useradd -m -s /bin/bash "$USER_NAME"
# '*' = no password but not "locked", so sshd allows key/Kerberos logins.
usermod -p '*' "$USER_NAME"
HOME_DIR="$(getent passwd "$USER_NAME" | cut -d: -f6)"
PROXY_PASS="hw-proxy-$RANDOM$RANDOM"
id "$PROXY_USER" >/dev/null 2>&1 || useradd -M -s /usr/sbin/nologin "$PROXY_USER"
echo "$PROXY_USER:$PROXY_PASS" | chpasswd

# --- keys and certificates --------------------------------------------------
k() { ssh-keygen -q -t "$1" -N "" -f "$WORK/$2" -C "$2" <<<y >/dev/null; }
k ed25519 host_key
k ed25519 user_ca
k ed25519 client_key           # in authorized_keys
k ed25519 cert_key             # only usable through its certificate
k rsa cert_rsa                 # RSA certificate (rsa-sha2-*-cert)
k ed25519 cert_fallback        # in authorized_keys, expired certificate
k ed25519 cert_wrongprinc      # in authorized_keys, cert for another user
k ed25519 agent_cert_key       # certificate held by the agent only
sign() { ssh-keygen -q -s "$WORK/user_ca" -I "$1" -n "$2" ${3:+-V "$3"} "$WORK/$1.pub"; }
sign cert_key "$USER_NAME"
sign cert_rsa "$USER_NAME"
sign cert_fallback "$USER_NAME" "20200101:20200102"
sign cert_wrongprinc nobody
sign agent_cert_key "$USER_NAME"

mkdir -p "$HOME_DIR/.ssh"
cat "$WORK/client_key.pub" "$WORK/cert_fallback.pub" "$WORK/cert_wrongprinc.pub" \
    >"$HOME_DIR/.ssh/authorized_keys"
chown -R "$USER_NAME" "$HOME_DIR/.ssh"
chmod 700 "$HOME_DIR/.ssh"; chmod 600 "$HOME_DIR/.ssh/authorized_keys"
# Written by the security-key test (in the user's home so sshd, reading as
# the user, can get to it wherever $WORK is).
SK_KEYS="$HOME_DIR/.ssh/hostwick_sk_keys"
: >"$SK_KEYS"; chmod 644 "$SK_KEYS"

# --- agent ------------------------------------------------------------------
# Holds only a key whose plain form is not authorized: agent logins must use
# its certificate.
eval "$(ssh-agent -a "$WORK/agent.sock")" >/dev/null
ssh-add -q "$WORK/agent_cert_key" 2>/dev/null # adds the key and its -cert.pub

ENV="$WORK/env.sh"
{
    echo "export HOSTWICK_NET_PORT=$PORT HOSTWICK_NET_USER=$USER_NAME HOSTWICK_NET_DIR=$WORK"
    echo "export SSH_AUTH_SOCK=$SSH_AUTH_SOCK SSH_AGENT_PID=$SSH_AGENT_PID"
    echo "export HOSTWICK_NET_SK_AUTH_KEYS=$SK_KEYS"
} >"$ENV"

# --- Kerberos ---------------------------------------------------------------
if command -v krb5kdc >/dev/null && command -v kdb5_util >/dev/null; then
    mkdir -p "$WORK/kdc"
    cat >"$WORK/krb5.conf" <<CFG
[libdefaults]
    default_realm = $REALM
    dns_lookup_kdc = false
    dns_lookup_realm = false
    dns_canonicalize_hostname = false
    rdns = false
    forwardable = true
[realms]
    $REALM = {
        kdc = 127.0.0.1:$KDC_PORT
    }
[domain_realm]
    localhost = $REALM
CFG
    cat >"$WORK/kdc.conf" <<CFG
[kdcdefaults]
    kdc_ports = $KDC_PORT
    kdc_tcp_ports = $KDC_PORT
[realms]
    $REALM = {
        database_name = $WORK/kdc/principal
        key_stash_file = $WORK/kdc/stash
        acl_file = $WORK/kdc/kadm5.acl
        kdc_ports = $KDC_PORT
        kdc_tcp_ports = $KDC_PORT
    }
CFG
    export KRB5_CONFIG="$WORK/krb5.conf" KRB5_KDC_PROFILE="$WORK/kdc.conf"
    kdb5_util create -s -r "$REALM" -P "kdc-master-$RANDOM" >/dev/null
    kadmin.local -q "addprinc -pw hwnet-krb-pass $USER_NAME" >/dev/null
    kadmin.local -q "addprinc -randkey host/localhost" >/dev/null
    kadmin.local -q "ktadd -k $WORK/krb5.keytab host/localhost" >/dev/null
    krb5kdc -P "$WORK/kdc.pid"
    sleep 1
    export KRB5CCNAME="FILE:$WORK/ccache"
    echo hwnet-krb-pass | kinit -f "$USER_NAME" >/dev/null
    {
        echo "export HOSTWICK_NET_KRB5=1 KRB5_CONFIG=$KRB5_CONFIG KRB5CCNAME=$KRB5CCNAME"
    } >>"$ENV"
fi

# --- sshd -------------------------------------------------------------------
mkdir -p /run/sshd
cat >"$WORK/sshd_config" <<CFG
Port $PORT
Port $RESTRICTED_PORT
ListenAddress 127.0.0.1
HostKey $WORK/host_key
PidFile $WORK/sshd.pid
StrictModes no
UsePAM no
PasswordAuthentication no
KbdInteractiveAuthentication no
PubkeyAuthentication yes
AuthorizedKeysFile .ssh/authorized_keys .ssh/hostwick_sk_keys
TrustedUserCAKeys $WORK/user_ca.pub
GSSAPIAuthentication yes
GSSAPIStrictAcceptorCheck no
GSSAPICleanupCredentials yes
X11Forwarding yes
X11UseLocalhost yes
XAuthLocation $(command -v xauth || echo /usr/bin/xauth)
AllowAgentForwarding yes
AllowTcpForwarding yes
Subsystem sftp internal-sftp
Match LocalPort $RESTRICTED_PORT
    X11Forwarding no
    AllowAgentForwarding no
CFG
# The KDC trace lets the delegation test see the forwarded ticket arrive.
env KRB5_CONFIG="${KRB5_CONFIG:-/etc/krb5.conf}" KRB5_KTNAME="$WORK/krb5.keytab" \
    KRB5_TRACE="$WORK/sshd-krb5.trace" \
    "$(command -v sshd)" -f "$WORK/sshd_config" -E "$WORK/sshd.log"
echo "export HOSTWICK_NET_KRB5_TRACE=$WORK/sshd-krb5.trace HOSTWICK_NET_RESTRICTED_PORT=$RESTRICTED_PORT" >>"$ENV"

# --- X server ---------------------------------------------------------------
if command -v Xvfb >/dev/null && command -v xauth >/dev/null; then
    export XAUTHORITY="$WORK/Xauthority"
    xauth -q -f "$XAUTHORITY" add ":$XNUM" MIT-MAGIC-COOKIE-1 \
        "$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')" 2>/dev/null
    Xvfb ":$XNUM" -auth "$XAUTHORITY" -nolisten tcp >"$WORK/xvfb.log" 2>&1 &
    echo $! >"$WORK/xvfb.pid"
    echo "export HOSTWICK_NET_X11_DISPLAY=:$XNUM XAUTHORITY=$XAUTHORITY" >>"$ENV"
fi

# --- proxies ----------------------------------------------------------------
if command -v tinyproxy >/dev/null; then
    cat >"$WORK/tinyproxy.conf" <<CFG
Port $HTTP_PROXY_PORT
Listen 127.0.0.1
Timeout 60
LogLevel Critical
MaxClients 20
Allow 127.0.0.1
BasicAuth $PROXY_USER $PROXY_PASS
ConnectPort $PORT
CFG
    # Foreground (-d) under our control: it drops privileges and could not
    # write a pid file into $WORK.
    tinyproxy -d -c "$WORK/tinyproxy.conf" >/dev/null 2>&1 &
    echo $! >"$WORK/tinyproxy.pid"
    echo "export HOSTWICK_NET_HTTP_PROXY=http://$PROXY_USER@127.0.0.1:$HTTP_PROXY_PORT" >>"$ENV"
fi
if command -v danted >/dev/null; then
    cat >"$WORK/danted.conf" <<CFG
logoutput: $WORK/danted.log
internal: 127.0.0.1 port = $SOCKS_PROXY_PORT
external: 127.0.0.1
socksmethod: username
user.privileged: root
user.unprivileged: nobody
client pass { from: 127.0.0.0/8 to: 0.0.0.0/0 }
socks pass { from: 127.0.0.0/8 to: 127.0.0.1/32 port = $PORT }
socks block { from: 0.0.0.0/0 to: 0.0.0.0/0 log: connect error }
CFG
    danted -D -f "$WORK/danted.conf" -p "$WORK/danted.pid"
    echo "export HOSTWICK_NET_SOCKS_PROXY=socks5://$PROXY_USER@127.0.0.1:$SOCKS_PROXY_PORT" >>"$ENV"
fi
[ -n "${PROXY_PASS:-}" ] && echo "export HOSTWICK_NET_PROXY_PASSWORD=$PROXY_PASS" >>"$ENV"
sleep 1

if [ "$MODE" = setup ]; then
    echo "environment ready; source $ENV"
    exit 0
fi
# shellcheck disable=SC1090
. "$ENV"
cargo test -p hostwick-ssh --test net_e2e -- --nocapture --test-threads=2
cargo test -p hostwick-ssh --test sk_agent_e2e -- --nocapture
cargo test -p hostwick-ssh --test krb5_no_ticket_e2e -- --nocapture
