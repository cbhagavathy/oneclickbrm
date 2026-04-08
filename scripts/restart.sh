#!/usr/bin/env bash
# Stop then start. Same targets as start.sh / stop.sh.
# Usage: ./scripts/restart.sh [server|client|all]
# Same env vars as start.sh apply after stop.

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TARGET="${1:-all}"

"$SCRIPT_DIR/stop.sh" "$TARGET"
"$SCRIPT_DIR/start.sh" "$TARGET"
