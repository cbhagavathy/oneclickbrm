#!/usr/bin/env bash
# Build Linux heartbeat-client using Docker (works from macOS or Windows with Docker Desktop).
# Produces glibc-linked binary for the requested platform (default: linux/amd64).
#
# Usage:
#   ./build-client-linux-docker.sh              # amd64
#   ./build-client-linux-docker.sh arm64        # aarch64 / arm64
#
# Requires: docker
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$SCRIPT_DIR/lib.sh"

PLATFORM="${1:-amd64}"
case "$PLATFORM" in
  amd64|x86_64)
    DOCKER_PLATFORM="linux/amd64"
    RUST_ARCH="x86_64"
    ;;
  arm64|aarch64)
    DOCKER_PLATFORM="linux/arm64"
    RUST_ARCH="aarch64"
    ;;
  *)
    echo "Usage: $0 [amd64|arm64]" >&2
    exit 1
    ;;
esac

command -v docker >/dev/null 2>&1 || {
  echo "docker not found. Install Docker Desktop or Docker Engine." >&2
  exit 1
}

ROOT="$(release_root)"
VER="$(client_version "$ROOT")"
DIST="$(dist_dir "$ROOT")"
mkdir -p "$DIST"

IMAGE="${RUST_LINUX_IMAGE:-rust:1-bookworm}"

echo "Building in Docker ($DOCKER_PLATFORM) image=$IMAGE ..."

docker run --rm \
  --platform "$DOCKER_PLATFORM" \
  -v "$ROOT:/workspace:rw" \
  -w /workspace \
  "$IMAGE" \
  bash -lc "rustup target add ${RUST_ARCH}-unknown-linux-gnu 2>/dev/null || true; cargo build --release -p heartbeat-client --target ${RUST_ARCH}-unknown-linux-gnu"

BIN="$ROOT/target/${RUST_ARCH}-unknown-linux-gnu/release/heartbeat-client"
if [[ ! -f "$BIN" ]]; then
  echo "Expected binary missing: $BIN" >&2
  exit 1
fi

STAGE="$DIST/stage-linux-docker-$RUST_ARCH"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp "$BIN" "$STAGE/heartbeat-client"
chmod +x "$STAGE/heartbeat-client"
write_install_txt "$STAGE/INSTALL.txt" "$VER" "Linux $RUST_ARCH (glibc, built in Docker)"

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
