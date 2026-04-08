#!/usr/bin/env bash
# Build release heartbeat-client on Linux (native) and package to dist/.
# Run on a Linux machine with Rust installed.
#
# Output: dist/heartbeat-client-<version>-linux-<arch>.tar.gz
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$SCRIPT_DIR/lib.sh"

ROOT="$(release_root)"
cd "$ROOT"

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "This script expects Linux. On macOS/Windows use build-client-linux-docker.sh (Docker)." >&2
  exit 1
fi

ensure_rust
VER="$(client_version "$ROOT")"
DIST="$(dist_dir "$ROOT")"
mkdir -p "$DIST"

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|aarch64) RUST_ARCH="$ARCH" ;;
  *) echo "Unsupported arch: $ARCH" >&2; exit 1 ;;
esac

echo "Building heartbeat-client $VER for Linux ($RUST_ARCH)..."
cargo build --release -p heartbeat-client

BIN="$ROOT/target/release/heartbeat-client"
if [[ ! -x "$BIN" ]]; then
  echo "Expected binary missing: $BIN" >&2
  exit 1
fi

if command -v strip >/dev/null 2>&1; then
  strip "$BIN" 2>/dev/null || true
fi

STAGE="$DIST/stage-linux-$RUST_ARCH"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp "$BIN" "$STAGE/heartbeat-client"
chmod +x "$STAGE/heartbeat-client"
write_install_txt "$STAGE/INSTALL.txt" "$VER" "Linux $RUST_ARCH"

BUNDLE="heartbeat-client-${VER}-linux-${RUST_ARCH}"
ARCHIVE="$DIST/${BUNDLE}.tar.gz"
rm -f "$ARCHIVE"
(
  cd "$STAGE"
  tar -czvf "$ARCHIVE" heartbeat-client INSTALL.txt
)
rm -rf "$STAGE"

SUMS="$DIST/SHA256SUMS"
touch "$SUMS"
append_checksums "$SUMS" "$ARCHIVE"

echo "Built: $ARCHIVE"
