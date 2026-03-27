#!/usr/bin/env bash
# Quick status report for experiment agent fleet.
# Usage: ./scripts/agent-status.sh [LOG_DIR]
#
# If LOG_DIR not given, finds the most recent agent-* log directory.

set -euo pipefail

LOG_DIR="${1:-$(ls -td logs/agent-* logs/experiment-agent-* 2>/dev/null | head -1)}"
if [[ -z "$LOG_DIR" ]] || [[ ! -d "$LOG_DIR" ]]; then
    echo "No log directory found. Usage: $0 <log_dir>"
    exit 1
fi

SERVER=$(python3 -c "import json; print(json.load(open('$LOG_DIR/config.json'))['server'])" 2>/dev/null || echo "http://localhost:3001")
RELAY=$(python3 -c "
import json
d=json.load(open('$LOG_DIR/config.json'))
url=d.get('dashboard','ws://localhost:4000/ws/worker')
print(url.replace('ws://','http://').replace('/ws/worker',''))
" 2>/dev/null || echo "http://localhost:4000")
N=$(python3 -c "import json; print(json.load(open('$LOG_DIR/config.json'))['n'])" 2>/dev/null || echo "25")
COMMIT=$(python3 -c "import json; print(json.load(open('$LOG_DIR/config.json'))['commit'])" 2>/dev/null || echo "?")
STARTED=$(python3 -c "import json; print(json.load(open('$LOG_DIR/config.json'))['started'])" 2>/dev/null || echo "?")

echo "========================================"
echo "  EXPERIMENT STATUS  $(date '+%H:%M:%S')"
echo "  commit=$COMMIT  n=$N  started=$STARTED"
echo "  logs=$LOG_DIR"
echo "========================================"
echo ""

# ── Leaderboard ──────────────────────────────────────────
echo "--- Leaderboard (n=$N) ---"
curl -sf "$SERVER/api/leaderboards/$N?limit=3" 2>/dev/null | python3 -c "
import json, sys
try:
    data = json.load(sys.stdin)
    print(f'  Total: {data[\"total\"]} entries')
    for e in data['entries']:
        h = e['histogram']
        tiers = {t['k']: (t['red'], t['blue']) for t in h['tiers']}
        c4 = tiers.get(4, (0,0))
        c3 = tiers.get(3, (0,0))
        print(f'  #{e[\"rank\"]}: 4c=({c4[0]},{c4[1]}) 3c=({c3[0]},{c3[1]}) gap={e[\"goodman_gap\"]} aut={e[\"aut_order\"]} key={e[\"key_id\"][:8]}')
except: print('  (unreachable)')
" 2>/dev/null || echo "  (server unreachable)"
echo ""

# ── Workers via dashboard relay ──────────────────────────
echo "--- Fleet ---"
WORKER_DATA=$(curl -sf "$RELAY/api/workers" 2>/dev/null || echo '{"count":0,"workers":[]}')
WORKER_COUNT=$(echo "$WORKER_DATA" | python3 -c "import json,sys; print(json.load(sys.stdin)['count'])" 2>/dev/null || echo 0)
echo "  $WORKER_COUNT workers on dashboard"
echo ""

# ── Per-worker metrics via HTTP API ──────────────────────
echo "--- Per-Worker Metrics ---"
printf "  %-12s %6s %7s %7s %8s %8s\n" "WORKER" "ROUND" "DISC" "ADMIT" "RATE/m" "ROUND_MS"
printf "  %-12s %6s %7s %7s %8s %8s\n" "------" "-----" "----" "-----" "------" "--------"

TOTAL_DISC=0
TOTAL_ADMIT=0
TOTAL_ROUNDS=0

for f in "$LOG_DIR"/*.log; do
    NAME=$(basename "$f" .log)
    PORT=$(sed 's/\x1b\[[0-9;]*m//g' "$f" 2>/dev/null | grep "API server ready" | grep -oP 'api=http://0.0.0.0:\K[0-9]+' || true)
    if [[ -z "$PORT" ]]; then continue; fi

    # Check if process is alive
    if ! curl -sf "http://localhost:$PORT/api/status" > /dev/null 2>&1; then
        printf "  %-12s %s\n" "$NAME" "(stopped)"
        continue
    fi

    python3 -c "
import json, sys, time
d = json.loads('''$(curl -sf "http://localhost:$PORT/api/status" 2>/dev/null)''')
m = d['metrics']
r = d['round']
disc = m['total_discoveries']
admit = m['total_admitted']
ms = m['last_round_ms']
# Estimate rate from round time
rate = admit / (r * ms / 60000) if r > 0 and ms > 0 else 0
print(f'  {\"$NAME\":<12} {r:>6} {disc:>7} {admit:>7} {rate:>7.1f} {ms:>8}')
print(f'TOTALS:{r}:{disc}:{admit}', file=sys.stderr)
" 2>>"$LOG_DIR/.status_tmp" || printf "  %-12s %s\n" "$NAME" "(error)"
done

# ── Totals ───────────────────────────────────────────────
if [[ -f "$LOG_DIR/.status_tmp" ]]; then
    python3 -c "
import sys
rounds = disc = admit = 0
for line in open('$LOG_DIR/.status_tmp'):
    if line.startswith('TOTALS:'):
        parts = line.strip().split(':')
        rounds += int(parts[1])
        disc += int(parts[2])
        admit += int(parts[3])
print(f'  {\"TOTAL\":<12} {rounds:>6} {disc:>7} {admit:>7}')
" 2>/dev/null
    rm -f "$LOG_DIR/.status_tmp"
fi

echo ""

# ── Recent log activity ──────────────────────────────────
echo "--- Recent Rounds (last per worker) ---"
for f in "$LOG_DIR"/*.log; do
    NAME=$(basename "$f" .log)
    LAST=$(sed 's/\x1b\[[0-9;]*m//g' "$f" 2>/dev/null | grep "round complete" | tail -1 || true)
    if [[ -n "$LAST" ]]; then
        # Extract key metrics from the log line
        ROUND=$(echo "$LAST" | grep -oP 'round=\K[0-9]+' || echo "?")
        UNIQUE=$(echo "$LAST" | grep -oP 'new_unique=\K[0-9]+' || echo "?")
        SKIP_THR=$(echo "$LAST" | grep -oP 'skip_thr=\K[0-9]+' || echo "?")
        SUBMITTED=$(echo "$LAST" | grep -oP ' submitted=\K[0-9]+' || echo "?")
        ADMITTED=$(echo "$LAST" | grep -oP ' admitted=\K[0-9]+' | head -1 || echo "?")
        MS=$(echo "$LAST" | grep -oP ' ms=\K[0-9]+' || echo "?")
        printf "  %-12s r%-3s unique=%-3s submit=%-3s admit=%-3s skip_thr=%-7s %sms\n" "$NAME" "$ROUND" "$UNIQUE" "$SUBMITTED" "$ADMITTED" "$SKIP_THR" "$MS"
    fi
done
