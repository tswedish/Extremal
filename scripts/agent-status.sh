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
curl -sf --max-time 5 "$SERVER/api/leaderboards/$N?limit=3" 2>/dev/null | python3 -c "
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

# ── Recent Leaderboard Activity ──────────────────────────
echo "--- Recent Leaderboard Activity ---"
curl -sf --max-time 5 "$SERVER/api/leaderboards/$N?limit=10" 2>/dev/null | python3 -c "
import json, sys
from datetime import datetime, timezone
try:
    data = json.load(sys.stdin)
    entries = data['entries']
    # Find most recent admission
    if entries:
        latest = entries[0]
        t = datetime.fromisoformat(latest['admitted_at'].replace('+00:00', '+00:00'))
        now = datetime.now(timezone.utc)
        age = now - t
        hours = age.total_seconds() / 3600
        if hours < 1:
            age_str = f'{int(age.total_seconds() / 60)}m ago'
        elif hours < 24:
            age_str = f'{hours:.1f}h ago'
        else:
            age_str = f'{hours/24:.1f}d ago'
        print(f'  Most recent admission: {latest[\"admitted_at\"][:19]} ({age_str})')
    # Check bottom of board too
except: print('  (unavailable)')
" 2>/dev/null || echo "  (server unreachable)"

# Check bottom of board for threshold context
curl -sf --max-time 5 "$SERVER/api/leaderboards/$N?limit=3&offset=$(($(curl -sf --max-time 5 "$SERVER/api/leaderboards/$N?limit=1" 2>/dev/null | python3 -c "import json,sys;print(json.load(sys.stdin)['total']-3)" 2>/dev/null || echo 497)))" 2>/dev/null | python3 -c "
import json, sys
from datetime import datetime, timezone
try:
    data = json.load(sys.stdin)
    for e in data['entries']:
        h = e['histogram']
        tiers = {t['k']: (t['red'], t['blue']) for t in h['tiers']}
        c4 = tiers.get(4, (0,0))
        t = e['admitted_at'][:19]
        print(f'  Bottom #{e[\"rank\"]}: 4c=({c4[0]},{c4[1]}) gap={e[\"goodman_gap\"]} admitted={t}')
except: pass
" 2>/dev/null

# Threshold
curl -sf --max-time 5 "$SERVER/api/leaderboards/$N/threshold" 2>/dev/null | python3 -c "
import json, sys
try:
    data = json.load(sys.stdin)
    sb = data.get('threshold_score_bytes', '')
    if sb:
        # Score bytes layout: [0-15]=zeros, [16-23]=4c_max, [24-31]=4c_min, [32-39]=3c_max, [40-47]=3c_min
        b = bytes.fromhex(sb)
        c4_max = int.from_bytes(b[16:24], 'big')
        c4_min = int.from_bytes(b[24:32], 'big')
        c3_max = int.from_bytes(b[32:40], 'big')
        c3_min = int.from_bytes(b[40:48], 'big')
        print(f'  Threshold: 4c=({c4_max},{c4_min}) 3c=({c3_max},{c3_min}), count={data[\"count\"]}/{data[\"capacity\"]}')
except: pass
" 2>/dev/null
echo ""

# ── Workers via dashboard relay ──────────────────────────
echo "--- Fleet ---"
WORKER_DATA=$(curl -sf --max-time 5 "$RELAY/api/workers" 2>/dev/null || echo '{"count":0,"workers":[]}')
WORKER_COUNT=$(echo "$WORKER_DATA" | python3 -c "import json,sys; print(json.load(sys.stdin)['count'])" 2>/dev/null || echo 0)
echo "  $WORKER_COUNT workers on dashboard"
echo ""

# ── Per-worker metrics via HTTP API ──────────────────────
echo "--- Per-Worker Metrics ---"
printf "  %-14s %5s %6s %5s %7s %7s %8s  %s\n" "WORKER" "ROUND" "DISC" "ADMIT" "RATE/m" "MS/rnd" "UPTIME" "BEST_LOCAL"
printf "  %-14s %5s %6s %5s %7s %7s %8s  %s\n" "------" "-----" "----" "-----" "------" "------" "------" "----------"

rm -f "$LOG_DIR/.status_tmp"

for f in "$LOG_DIR"/*.log; do
    NAME=$(basename "$f" .log)
    PORT=$(sed 's/\x1b\[[0-9;]*m//g' "$f" 2>/dev/null | grep "API server ready" | grep -oP 'api=http://0.0.0.0:\K[0-9]+' || true)
    if [[ -z "$PORT" ]]; then continue; fi

    # Check if process is alive
    STATUS_JSON=$(curl -sf --max-time 3 "http://localhost:$PORT/api/status" 2>/dev/null || true)
    if [[ -z "$STATUS_JSON" ]]; then
        printf "  %-14s %s\n" "$NAME" "(stopped)"
        continue
    fi

    python3 -c "
import json, sys
d = json.loads('''$STATUS_JSON''')
m = d['metrics']
r = d['round']
disc = m['total_discoveries']
admit = m['total_admitted']
ms = m['last_round_ms']
uptime = m.get('uptime_secs', 0)
best = m.get('best_local_score', None)
last_admit = m.get('last_admitted_at', None)

# Uptime formatting
if uptime >= 3600:
    up_str = f'{uptime//3600}h{(uptime%3600)//60}m'
elif uptime >= 60:
    up_str = f'{uptime//60}m'
else:
    up_str = f'{uptime}s'

# Rate from total elapsed
rate = admit / (uptime / 60) if uptime > 60 else 0

# Best local score: extract 4c tiers if available
best_str = '-'
if best:
    try:
        b = bytes.fromhex(best)
        c4_max = int.from_bytes(b[16:24], 'big')
        c4_min = int.from_bytes(b[24:32], 'big')
        best_str = f'4c=({c4_max},{c4_min})'
    except:
        best_str = best[:16]

# Last admission indicator
admit_flag = ''
if last_admit:
    admit_flag = ' *'

print(f'  {\"$NAME\":<14} {r:>5} {disc:>6} {admit:>5}{admit_flag} {rate:>6.1f} {ms:>7} {up_str:>8}  {best_str}')
print(f'TOTALS:{r}:{disc}:{admit}:{uptime}', file=sys.stderr)
" 2>>"$LOG_DIR/.status_tmp" || printf "  %-14s %s\n" "$NAME" "(error)"
done

# ── Totals ───────────────────────────────────────────────
if [[ -f "$LOG_DIR/.status_tmp" ]]; then
    python3 -c "
import sys
rounds = disc = admit = 0
max_uptime = 0
for line in open('$LOG_DIR/.status_tmp'):
    if line.startswith('TOTALS:'):
        parts = line.strip().split(':')
        rounds += int(parts[1])
        disc += int(parts[2])
        admit += int(parts[3])
        up = int(parts[4])
        if up > max_uptime: max_uptime = up
up_str = f'{max_uptime//3600}h{(max_uptime%3600)//60}m' if max_uptime >= 3600 else f'{max_uptime//60}m'
rate = admit / (max_uptime / 60) if max_uptime > 60 else 0
print(f'  {\"TOTAL\":<14} {rounds:>5} {disc:>6} {admit:>5}  {rate:>6.1f}         {up_str}')
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
        ROUND=$(echo "$LAST" | grep -oP 'round=\K[0-9]+' || echo "?")
        UNIQUE=$(echo "$LAST" | grep -oP 'new_unique=\K[0-9]+' || echo "?")
        SKIP_THR=$(echo "$LAST" | grep -oP 'skip_thr=\K[0-9]+' || echo "?")
        SUBMITTED=$(echo "$LAST" | grep -oP ' submitted=\K[0-9]+' || echo "?")
        ADMITTED=$(echo "$LAST" | grep -oP ' admitted=\K[0-9]+' | head -1 || echo "?")
        MS=$(echo "$LAST" | grep -oP ' ms=\K[0-9]+' || echo "?")
        printf "  %-14s r%-4s unique=%-4s submit=%-3s admit=%-3s skip_thr=%-8s %sms\n" "$NAME" "$ROUND" "$UNIQUE" "$SUBMITTED" "$ADMITTED" "$SKIP_THR" "$MS"
    fi
done

echo ""

# ── CPU load ─────────────────────────────────────────────
echo "--- System ---"
WORKER_COUNT=$(pgrep -c -f "extremal-worker" 2>/dev/null || echo 0)
CORES=$(nproc 2>/dev/null || echo "?")
LOAD=$(cat /proc/loadavg 2>/dev/null | awk '{print $1, $2, $3}' || echo "?")
MEM=$(free -h 2>/dev/null | awk '/^Mem:/ {print $3 "/" $2}' || echo "?")
echo "  Workers: $WORKER_COUNT  Cores: $CORES  Load: $LOAD  Mem: $MEM"
