#!/usr/bin/env bash
# Bump heartbeat-client (and optionally workspace display) version in Cargo.toml.
# Usage:
#   ./bump-version.sh 0.2.0
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$SCRIPT_DIR/lib.sh"

NEW="${1:-}"
if [[ -z "$NEW" ]] || [[ "$NEW" == -* ]]; then
  echo "Usage: $0 <new_version>" >&2
  echo "Example: $0 0.2.0" >&2
  exit 1
fi

ROOT="$(release_root)"
bump_toml() {
  local f="$1"
  if [[ ! -f "$f" ]]; then
    echo "Missing $f" >&2
    exit 1
  fi
  if sed --version >/dev/null 2>&1; then
    sed -i "s/^version = \".*\"/version = \"$NEW\"/" "$f"
  else
    sed -i '' "s/^version = \".*\"/version = \"$NEW\"/" "$f"
  fi
}

bump_toml "$ROOT/heartbeat-client/Cargo.toml"
bump_toml "$ROOT/heartbeat-server/Cargo.toml"

echo "Set package version to $NEW in heartbeat-client and heartbeat-server Cargo.toml files"
echo "Next: cargo build --release -p heartbeat-client && scripts/release/build-client-macos.sh (etc.)"
