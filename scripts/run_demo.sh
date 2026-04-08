#!/usr/bin/env bash
# Starts the mock heartbeat server, then the client. Ctrl+C stops both.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
PORT="${PORT:-9847}"
export PORT

cargo build -q -p heartbeat-server -p heartbeat-client

./target/debug/heartbeat-server &
SRV_PID=$!
cleanup() {
  kill "$SRV_PID" 2>/dev/null || true
}
trap cleanup EXIT INT TERM
sleep 1

echo "--- Server PID $SRV_PID ---"
echo "Server dashboard: http://127.0.0.1:${PORT}/"
echo "Download client:  http://127.0.0.1:${PORT}/download/client"

if [[ -z "${CLIENT_ID:-}" ]]; then
  EMAIL="demo-$(date +%s)@heartbeat.local"
  REG_JSON=$(curl -sS -X POST "http://127.0.0.1:${PORT}/api/register" \
    -H "Content-Type: application/json" \
    -d "{\"email\":\"${EMAIL}\",\"name\":\"Demo user\",\"password\":\"demo12345\",\"country\":\"Demo\"}")
  CLIENT_ID=$(python3 -c "import json,sys; d=json.loads(sys.argv[1]); print(d.get('client_id') or '')" "$REG_JSON" 2>/dev/null || true)
  if [[ -z "$CLIENT_ID" ]]; then
    echo "Could not register a demo user (response: $REG_JSON). Set CLIENT_ID and re-run." >&2
    exit 1
  fi
  export CLIENT_ID
  echo "Registered demo account; CLIENT_ID=$CLIENT_ID"
fi

echo "--- Client (local dashboard opens in browser unless OPEN_BROWSER=0) ---"
./target/debug/heartbeat-client \
  --heartbeat-url "http://127.0.0.1:${PORT}/heartbeat" \
  --interval "${HEARTBEAT_INTERVAL_SECS:-3}" \
  --port "${CLIENT_DASHBOARD_PORT:-9860}" \
  --client-id "$CLIENT_ID"
