#!/usr/bin/env bash
# Stop processes started by scripts/start.sh (uses pidfiles).
# Usage: ./scripts/stop.sh [server|client|all]

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=common.sh
source "$SCRIPT_DIR/common.sh"

TARGET="${1:-all}"

usage() {
  echo "Usage: $0 [server|client|all]" >&2
  exit 1
}

stop_one() {
  local pidfile="$1"
  local label="$2"
  if [[ ! -f "$pidfile" ]]; then
    echo "$label: not running (no pidfile)"
    return 0
  fi
  local pid
  pid="$(cat "$pidfile" 2>/dev/null || true)"
  if ! is_pid_alive "$pid"; then
    rm -f "$pidfile"
    echo "$label: stale pidfile removed"
    return 0
  fi
  kill "$pid" 2>/dev/null || true
  local i
  for i in $(seq 1 30); do
    is_pid_alive "$pid" || break
    sleep 0.1
  done
  if is_pid_alive "$pid"; then
    kill -9 "$pid" 2>/dev/null || true
    echo "$label: force-stopped (PID $pid)"
  else
    echo "$label: stopped (PID $pid)"
  fi
  rm -f "$pidfile"
}

case "$TARGET" in
server) stop_one "$PID_SERVER" "heartbeat-server" ;;
client) stop_one "$PID_CLIENT" "heartbeat-client" ;;
all)
  stop_one "$PID_SERVER" "heartbeat-server"
  stop_one "$PID_CLIENT" "heartbeat-client"
  ;;
*) usage ;;
esac
