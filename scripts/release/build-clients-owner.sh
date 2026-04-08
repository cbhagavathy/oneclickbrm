#!/usr/bin/env bash
# Owner workflow: build everything you can on this machine, write artifacts under dist/.
#
# Usage:
#   ./build-clients-owner.sh           # mac: native mac + docker linux amd64 + docker linux arm64
#   ./build-clients-owner.sh mac     # macOS package only
#   ./build-clients-owner.sh linux   # docker linux amd64 + arm64 (from mac or linux with docker)
#
# Version: edit heartbeat-client/Cargo.toml or run: ./bump-version.sh X.Y.Z
#
# For Windows .zip you must run scripts/release/build-client-windows.ps1 on a Windows PC or CI.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$SCRIPT_DIR/lib.sh"

ROOT="$(release_root)"
MODE="${1:-all}"

cd "$ROOT"
mkdir -p "$(dist_dir "$ROOT")"
DIST="$(dist_dir "$ROOT")"
# Fresh checksum list for this run (avoid duplicate lines if you rebuild).
rm -f "$DIST/SHA256SUMS"
VER="$(client_version "$ROOT")"
echo "Packaging heartbeat-client version: $VER"
echo "Artifacts: $(dist_dir "$ROOT")/"
echo ""

run_mac() {
  "$SCRIPT_DIR/build-client-macos.sh"
}

run_linux_docker() {
  local p="$1"
  "$SCRIPT_DIR/build-client-linux-docker.sh" "$p"
}

case "$(uname -s)" in
  Darwin)
    case "$MODE" in
      all)
        run_mac
        if command -v docker >/dev/null 2>&1; then
          run_linux_docker amd64
          run_linux_docker arm64
        else
          echo "Skipping Linux Docker builds (install Docker for linux/amd64 and linux/arm64 artifacts)." >&2
        fi
        echo ""
        echo "Windows: run on a Windows machine:"
        echo "  pwsh scripts/release/build-client-windows.ps1"
        ;;
      mac)
        run_mac
        ;;
      linux)
        if command -v docker >/dev/null 2>&1; then
          run_linux_docker amd64
          run_linux_docker arm64
        else
          echo "docker required for MODE=linux on macOS" >&2
          exit 1
        fi
        ;;
      *)
        echo "Usage: $0 [all|mac|linux]" >&2
        exit 1
        ;;
    esac
    ;;
  Linux)
    case "$MODE" in
      all|linux)
        "$SCRIPT_DIR/build-client-linux.sh"
        echo ""
        echo "For macOS .tar.gz build on a Mac. For Windows .zip run build-client-windows.ps1 on Windows."
        ;;
      mac)
        echo "macOS builds must be run on macOS." >&2
        exit 1
        ;;
      *)
        echo "Usage: $0 [all|linux]" >&2
        exit 1
        ;;
    esac
    ;;
  *)
    echo "Unsupported host OS for this script. Use OS-specific scripts in scripts/release/." >&2
    exit 1
    ;;
esac

echo ""
echo "Done. Upload files from $(dist_dir "$ROOT")/ to your website (no git required for end users)."
