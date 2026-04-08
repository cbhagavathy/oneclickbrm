#!/usr/bin/env bash
# Start heartbeat-server and/or heartbeat-client in the background with nohup.
# Usage: ./scripts/start.sh [server|client|all]
# Env: PORT, HEARTBEAT_URL, HEARTBEAT_INTERVAL_SECS, CLIENT_ID, CLIENT_DASHBOARD_PORT, OPEN_BROWSER, BIN_DIR

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=common.sh
source "$SCRIPT_DIR/common.sh"

TARGET="${1:-all}"

usage() {
  echo "Usage: $0 [server|client|all]" >&2
  exit 1
}

start_server() {
  ensure_dirs
  remove_stale_pidfile "$PID_SERVER"
  if [[ -f "$PID_SERVER" ]]; then
    echo "heartbeat-server already running (PID $(cat "$PID_SERVER"))"
    return 0
  fi

  local bin_dir
  bin_dir="${BIN_DIR:-$(resolve_bin_dir server)}"
  local exe="$bin_dir/heartbeat-server"

  export PORT="${PORT:-9847}"
  cd "$ROOT"
  nohup "$exe" >>"$LOG_DIR/heartbeat-server.log" 2>&1 &
  echo $! >"$PID_SERVER"
  echo "Started heartbeat-server PID $(cat "$PID_SERVER") (PORT=$PORT)"
  echo "  Log: $LOG_DIR/heartbeat-server.log"
  echo "  Dashboard: http://127.0.0.1:${PORT}/"
}

start_client() {
  ensure_dirs
  remove_stale_pidfile "$PID_CLIENT"
  if [[ -f "$PID_CLIENT" ]]; then
    echo "heartbeat-client already running (PID $(cat "$PID_CLIENT"))"
    return 0
  fi

  local bin_dir
  bin_dir="${BIN_DIR:-$(resolve_bin_dir client)}"
  local exe="$bin_dir/heartbeat-client"

  local hb_url="${HEARTBEAT_URL:-http://127.0.0.1:${PORT:-9847}/heartbeat}"
  local hb_interval="${HEARTBEAT_INTERVAL_SECS:-5}"
  local dash_port="${CLIENT_DASHBOARD_PORT:-9860}"
  export OPEN_BROWSER="${OPEN_BROWSER:-0}"

  local -a client_args=(--port "$dash_port" --heartbeat-url "$hb_url" --interval "$hb_interval")
  if [[ -n "${CLIENT_ID:-}" ]]; then
    client_args+=(--client-id "$CLIENT_ID")
  fi

  cd "$ROOT"
  nohup "$exe" "${client_args[@]}" \
    >>"$LOG_DIR/heartbeat-client.log" 2>&1 &
  echo $! >"$PID_CLIENT"
  echo "Started heartbeat-client PID $(cat "$PID_CLIENT")"
  echo "  Log: $LOG_DIR/heartbeat-client.log"
  echo "  Local dashboard: http://127.0.0.1:${dash_port}/"
  if [[ -z "${CLIENT_ID:-}" ]]; then
    echo "  Warning: CLIENT_ID is not set. Register at http://127.0.0.1:${PORT:-9847}/register and export CLIENT_ID, then restart the client." >&2
  fi
}

case "$TARGET" in
server) start_server ;;
client) start_client ;;
all)
  start_server
  start_client
  ;;
*) usage ;;
esac
