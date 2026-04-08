#!/usr/bin/env bash
# Build release heartbeat-client for macOS (current machine arch) and package to dist/.
# Run from repo root or this directory. Requires: Rust toolchain, strip (optional).
#
# Output: dist/heartbeat-client-<version>-macos-<arch>.tar.gz
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$SCRIPT_DIR/lib.sh"

ROOT="$(release_root)"
cd "$ROOT"

ensure_rust
VER="$(client_version "$ROOT")"
DIST="$(dist_dir "$ROOT")"
mkdir -p "$DIST"

ARCH="$(uname -m)"
case "$ARCH" in
  arm64) RUST_ARCH="aarch64" ;;
  x86_64) RUST_ARCH="x86_64" ;;
  *) echo "Unsupported arch: $ARCH" >&2; exit 1 ;;
esac

export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-11.0}"

echo "Building heartbeat-client $VER for macOS ($RUST_ARCH)..."
cargo build --release -p heartbeat-client

BIN="$ROOT/target/release/heartbeat-client"
if [[ ! -x "$BIN" ]]; then
  echo "Expected binary missing: $BIN" >&2
  exit 1
fi

if command -v strip >/dev/null 2>&1; then
  strip "$BIN" 2>/dev/null || true
fi

STAGE="$DIST/stage-macos-$RUST_ARCH"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp "$BIN" "$STAGE/heartbeat-client"
chmod +x "$STAGE/heartbeat-client"
write_install_txt "$STAGE/INSTALL.txt" "$VER" "macOS $RUST_ARCH"

BUNDLE="heartbeat-client-${VER}-macos-${RUST_ARCH}"
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
