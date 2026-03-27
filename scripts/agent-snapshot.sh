#!/usr/bin/env bash
# Take a leaderboard snapshot for the experiment agent.
# Usage: ./scripts/agent-snapshot.sh [--n 25] [--server URL]

set -euo pipefail

N="${1:-25}"
SERVER="${2:-http://localhost:3001}"
SNAP_DIR="experiments/agent/snapshots"
mkdir -p "$SNAP_DIR"

TIMESTAMP=$(date +%s)
FILE="$SNAP_DIR/${N}_${TIMESTAMP}.json"

echo "Snapshotting n=$N leaderboard..."
curl -sf "$SERVER/api/leaderboards/$N?limit=500" > "$FILE"

ENTRIES=$(python3 -c "import json; print(json.load(open('$FILE'))['total'])" 2>/dev/null || echo "?")
TOP=$(python3 -c "
import json
d = json.load(open('$FILE'))
if d['entries']:
    e = d['entries'][0]
    h = e['histogram']
    tiers = {t['k']: (t['red'], t['blue']) for t in h['tiers']}
    c4 = tiers.get(4, (0,0))
    print(f'4c=({c4[0]},{c4[1]})')
else:
    print('empty')
" 2>/dev/null || echo "?")

echo "  Saved: $FILE ($ENTRIES entries, top: $TOP)"
