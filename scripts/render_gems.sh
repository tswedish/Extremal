#!/usr/bin/env bash
# Render MineGraph Gems from the leaderboard.
#
# Usage:
#   ./scripts/render_gems.sh [OPTIONS]
#
# Options:
#   --server URL    Server URL (default: http://localhost:3001)
#   --k K           Ramsey k (default: 5)
#   --ell L         Ramsey ell (default: 5)
#   --n N           Vertex count (default: 25)
#   --limit N       Number of graphs to render (default: 10)
#   --output DIR    Output directory (default: gems/)
#   --open          Open sprite sheet after rendering

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"

SERVER="http://localhost:3001"
K=5
ELL=5
N=25
LIMIT=10
OUTDIR="gems"
OPEN=false

while [[ $# -gt 0 ]]; do
  case $1 in
    --server) SERVER="$2"; shift 2 ;;
    --k) K="$2"; shift 2 ;;
    --ell) ELL="$2"; shift 2 ;;
    --n) N="$2"; shift 2 ;;
    --limit) LIMIT="$2"; shift 2 ;;
    --output) OUTDIR="$2"; shift 2 ;;
    --open) OPEN=true; shift ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

VENV="$REPO/.venv/bin/python"
if [ ! -x "$VENV" ]; then
  echo "Error: Python venv not found. Set it up with:"
  echo "  python3 -m venv .venv && .venv/bin/pip install numpy Pillow"
  exit 1
fi

TMPFILE=$(mktemp /tmp/minegraph_graphs_XXXXX.jsonl)
trap "rm -f $TMPFILE" EXIT

echo "Fetching top $LIMIT graphs from $SERVER for R($K,$ELL) n=$N..."
curl -sf "$SERVER/api/leaderboards/$K/$ELL/$N/graphs?limit=$LIMIT" | \
  "$VENV" -c "
import json, sys
data = json.loads(sys.stdin.read())
for i, g in enumerate(data.get('graphs', [])):
    g['name'] = f'rank_{i+1:03d}'
    print(json.dumps(g))
" > "$TMPFILE"

COUNT=$(wc -l < "$TMPFILE")
if [ "$COUNT" -eq 0 ]; then
  echo "No graphs found on the leaderboard."
  exit 0
fi

echo "Rendering $COUNT gems..."
"$VENV" "$REPO/minegraph_gem_v3.py" \
  --batch "$TMPFILE" \
  --gallery-dir "$OUTDIR" \
  --sheet "$OUTDIR/sheet.png"

echo ""
echo "Gems saved to $OUTDIR/"
echo "Sprite sheet: $OUTDIR/sheet.png"

if $OPEN; then
  xdg-open "$OUTDIR/sheet.png" 2>/dev/null || true
fi
