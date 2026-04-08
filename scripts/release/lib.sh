#!/usr/bin/env bash
# Shared helpers for release packaging. Source from other scripts: source "$(dirname "$0")/lib.sh"
set -euo pipefail

release_root() {
  local here
  here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  echo "$(cd "$here/../.." && pwd)"
}

# Version from heartbeat-client/Cargo.toml (single source of truth for the binary).
client_version() {
  local root="$1"
  local line
  line="$(grep -E '^version\s*=' "$root/heartbeat-client/Cargo.toml" | head -1)"
  echo "${line#*=}" | tr -d ' "'\''\r'
}

dist_dir() {
  local root="$1"
  echo "$root/dist"
}

ensure_rust() {
  command -v cargo >/dev/null 2>&1 || {
    echo "cargo not found. Install Rust: https://rustup.rs/" >&2
    exit 1
  }
}

write_install_txt() {
  local out="$1"
  local version="$2"
  local os_label="$3"
  cat >"$out" <<EOF
Heartbeat client ${version} (${os_label})

Run the binary from a terminal in this folder.

Environment (optional):
  HEARTBEAT_URL     Server heartbeat URL (default http://127.0.0.1:9847/heartbeat)
  HEARTBEAT_INTERVAL_SECS  Seconds between heartbeats (default 5)
  CLIENT_ID         Pre-registered client id from your server
  OPEN_BROWSER=0    Do not open the browser on start

Example:
  ./heartbeat-client --heartbeat-url https://your-server.example/heartbeat

Support: see your product documentation.
EOF
}

sha256_file() {
  local f="$1"
  local base
  base="$(basename "$f")"
  if command -v shasum >/dev/null 2>&1; then
    (cd "$(dirname "$f")" && shasum -a 256 "$base") | awk -v b="$base" '{print $1 "  " b}'
  elif command -v sha256sum >/dev/null 2>&1; then
    (cd "$(dirname "$f")" && sha256sum "$base")
  else
    echo "(no shasum/sha256sum; skip checksum)" >&2
  fi
}

append_checksums() {
  local sums="$1"
  local file="$2"
  local line
  line="$(sha256_file "$file")"
  [[ -n "$line" ]] && echo "$line" >>"$sums"
}
