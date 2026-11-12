#!/usr/bin/env bash
set -euo pipefail

# ============================================================================
# sshd-proof.sh — prove `rustorm cleanup --auth` against a real OpenSSH server
# ============================================================================
# Brings up the throwaway container `.iz` defines as [machine.sshd-proof],
# starts sshd in it, then runs rustorm against a fixture HOME (never ~/.ssh):
# a host defined twice, the live block with a wrong user and the copy with the
# right one, plus a second name for the same target. The proof passes when
# cleanup reports live `auth failure` and copy `works`, `--apply` takes the
# copy, a real `ssh` login through the result succeeds, and `--fold` leaves no
# duplicates.
#
# SSHD_PROOF_HOST and SSHD_PROOF_PORT name where the container's sshd answers
# from this machine (default 127.0.0.1:22022, the port `.iz` publishes). When
# the docker daemon runs on another host, forward that port here first, for
# example: ssh -N -L 127.0.0.1:22022:127.0.0.1:22022 <docker-host>
# KEEP_HOME=1 keeps the fixture HOME for inspection.
# ============================================================================

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
KEY_DIR="$ROOT/target/sshd-proof"
KEY="$KEY_DIR/id_ed25519"
HOST="${SSHD_PROOF_HOST:-127.0.0.1}"
PORT="${SSHD_PROOF_PORT:-22022}"
RUSTORM="$ROOT/target/debug/rustorm"

fail() { echo "sshd-proof: $*" >&2; exit 1; }
step() { echo "sshd-proof: $*"; }

command -v izmachine >/dev/null || fail "izmachine is not on PATH"
command -v docker >/dev/null || fail "docker is not on PATH"
[[ -x "$RUSTORM" ]] || fail "$RUSTORM not built (run 'make build' first)"

mkdir -p "$KEY_DIR"
[[ -f "$KEY" ]] || ssh-keygen -q -t ed25519 -N '' -C rustorm-sshd-proof -f "$KEY"

step "izmachine up sshd-proof"
(cd "$ROOT" && izmachine up sshd-proof --format json >/dev/null) || fail "izmachine up sshd-proof failed"
docker exec -u root sshd-proof sh -c 'mkdir -p /run/sshd && (pgrep -x sshd >/dev/null || /usr/sbin/sshd)' \
    || fail "could not start sshd in the sshd-proof container"

SSH_OPTS=(-F /dev/null -o BatchMode=yes -o IdentitiesOnly=yes -i "$KEY"
          -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -p "$PORT")
ssh "${SSH_OPTS[@]}" "tester@$HOST" true \
    || fail "no sshd answers at $HOST:$PORT; if docker runs on another host, forward the port (see the header of this script)"

HOME_DIR="$(mktemp -d "${TMPDIR:-/tmp}/rustorm-sshd-proof.XXXXXX")"
trap '[[ -n "${KEEP_HOME:-}" ]] || rm -rf "$HOME_DIR"' EXIT
mkdir -p "$HOME_DIR/.ssh/config.d"
chmod 700 "$HOME_DIR/.ssh"
cat > "$HOME_DIR/.ssh/config" <<EOF
Include ~/.ssh/config.d/*

Host *
    IdentityFile $KEY
    IdentitiesOnly yes
    StrictHostKeyChecking no
    UserKnownHostsFile /dev/null
    LogLevel ERROR

Host proof
    HostName $HOST
    Port $PORT
    User tester

Host proof-twin
    HostName $HOST
    Port $PORT
    User tester
EOF
cat > "$HOME_DIR/.ssh/config.d/old" <<EOF
Host proof
    HostName $HOST
    Port $PORT
    User wronguser
EOF

expect_line() {
    # expect_line <output> <regex> <what>
    grep -Eq "$2" <<<"$1" || { echo "$1" >&2; fail "expected $3"; }
}

step "cleanup --auth"
out="$(HOME="$HOME_DIR" "$RUSTORM" cleanup --auth --timeout 5 || true)"
expect_line "$out" 'live .*wronguser@.*auth failure' "the live definition to fail auth"
expect_line "$out" 'copy .*tester@.*works' "the copy to work"

step "cleanup --auth --apply"
HOME="$HOME_DIR" "$RUSTORM" cleanup --auth --apply --timeout 5 || true
grep -q 'User tester' "$HOME_DIR/.ssh/config.d/old" || fail "--apply did not take the working copy"

step "ssh through the applied definition"
login="$(ssh -F "$HOME_DIR/.ssh/config.d/old" -o IdentityFile="$KEY" -o IdentitiesOnly=yes \
    -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o BatchMode=yes \
    proof 'echo login-$(whoami)')"
[[ "$login" == "login-tester" ]] || fail "login through the applied definition printed '$login'"

step "cleanup --fold proof-twin"
HOME="$HOME_DIR" "$RUSTORM" cleanup --fold proof-twin
out="$(HOME="$HOME_DIR" "$RUSTORM" cleanup --no-probe)"
expect_line "$out" '^no duplicates' "no duplicates after the fold"

step "passed (stop the container with: docker stop sshd-proof)"
