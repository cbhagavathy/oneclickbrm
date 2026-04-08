#!/usr/bin/env bash
# Shared helpers for start / stop / restart. Sourced by other scripts.

set -euo pipefail

_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export ROOT="$(cd "$_SCRIPT_DIR/.." && pwd)"
export PID_DIR="${PID_DIR:-$_SCRIPT_DIR/pids}"
export LOG_DIR="${LOG_DIR:-$ROOT/logs}"

export PID_SERVER="${PID_DIR}/heartbeat-server.pid"
export PID_CLIENT="${PID_DIR}/heartbeat-client.pid"

# Args: server | client — picks release/ if that binary exists, else debug/.
resolve_bin_dir() {
  local which="$1"
  local name="heartbeat-${which}"
  for rel in "$ROOT/target/release" "$ROOT/target/debug"; do
    if [[ -x "$rel/$name" ]]; then
      echo "$rel"
      return 0
    fi
  done
  echo "Missing $name. Build with: cargo build -p $name" >&2
  exit 1
}

ensure_dirs() {
  mkdir -p "$PID_DIR" "$LOG_DIR"
}

is_pid_alive() {
  local pid="$1"
  [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null
}

remove_stale_pidfile() {
  local f="$1"
  [[ -f "$f" ]] || return 0
  local pid
  pid="$(cat "$f" 2>/dev/null || true)"
  if is_pid_alive "$pid"; then
    return 0
  fi
  rm -f "$f"
}
