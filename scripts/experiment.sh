#!/usr/bin/env bash
# Run a head-to-head strategy experiment.
#
# Usage:
#   ./scripts/experiment.sh [OPTIONS]
#
# Options:
#   --a STRATEGY    Strategy A (default: tree)
#   --b STRATEGY    Strategy B (default: tree2)
#   --k K           Ramsey parameter k (default: 5)
#   --ell L         Ramsey parameter ell (default: 5)
#   --n N           Target vertex count (default: 25)
#   --port PORT     Server port (default: 3002)
#   --init MODE     Init mode (default: leaderboard)
#   --duration MIN  Suggested duration in minutes (default: 30)
#   --no-server     Don't start a server (use existing one)
#
# This script:
#   1. Builds release binaries
#   2. Starts the server (unless --no-server)
#   3. Starts two workers (strategy A and B) with logging
#   4. Prints instructions for analysis

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"

source "$HOME/.cargo/env" 2>/dev/null || true

# Defaults
STRATEGY_A="tree"
STRATEGY_B="tree2"
K=5
ELL=5
N=25
SERVER_PORT=3002
INIT_MODE="leaderboard"
DURATION=30
START_SERVER=true

# Parse args
while [[ $# -gt 0 ]]; do
  case $1 in
    --a) STRATEGY_A="$2"; shift 2 ;;
    --b) STRATEGY_B="$2"; shift 2 ;;
    --k) K="$2"; shift 2 ;;
    --ell) ELL="$2"; shift 2 ;;
    --n) N="$2"; shift 2 ;;
    --port) SERVER_PORT="$2"; shift 2 ;;
    --init) INIT_MODE="$2"; shift 2 ;;
    --duration) DURATION="$2"; shift 2 ;;
    --no-server) START_SERVER=false; shift ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

TIMESTAMP=$(date +%Y%m%d-%H%M%S)
LOGDIR="$REPO/logs/experiment-$TIMESTAMP"
mkdir -p "$LOGDIR"

SERVER_URL="http://localhost:$SERVER_PORT"
WORKER_A_PORT=$((SERVER_PORT + 100))
WORKER_B_PORT=$((WORKER_A_PORT + 1))

LOG_A="$LOGDIR/${STRATEGY_A}.log"
LOG_B="$LOGDIR/${STRATEGY_B}.log"
LOG_SERVER="$LOGDIR/server.log"
META="$LOGDIR/experiment.txt"

# Write experiment metadata
cat > "$META" <<EOF
Experiment: $STRATEGY_A vs $STRATEGY_B
Started:    $(date)
Target:     R($K,$ELL) n=$N
Init:       $INIT_MODE
Server:     $SERVER_URL
Duration:   ${DURATION}m (suggested)
Worker A:   $STRATEGY_A (dashboard :$WORKER_A_PORT, log: $LOG_A)
Worker B:   $STRATEGY_B (dashboard :$WORKER_B_PORT, log: $LOG_B)
EOF

echo ""
echo "=========================================="
echo "  Strategy Experiment: $STRATEGY_A vs $STRATEGY_B"
echo "=========================================="
echo ""
echo "  Target:     R($K,$ELL) n=$N"
echo "  Init:       $INIT_MODE"
echo "  Server:     $SERVER_URL"
echo "  Logs:       $LOGDIR/"
echo ""

# Step 1: Build
echo "--- Building release binaries ---"
cargo build --release --all --quiet 2>&1

# Track PIDs for cleanup
PIDS=()
cleanup() {
  echo ""
  echo "--- Stopping processes ---"
  for pid in "${PIDS[@]}"; do
    kill "$pid" 2>/dev/null || true
  done
  wait 2>/dev/null || true
  print_analysis
}
trap cleanup EXIT INT TERM

# Step 2: Server
if $START_SERVER; then
  echo "--- Starting server on :$SERVER_PORT ---"
  RUST_LOG=info cargo run --release -p ramseynet-server -- \
    --port "$SERVER_PORT" > "$LOG_SERVER" 2>&1 &
  PIDS+=($!)
  sleep 2

  # Health check
  if curl -sf "$SERVER_URL/api/health" > /dev/null 2>&1; then
    echo "  Server healthy"
  else
    echo "  WARNING: Server health check failed, continuing anyway..."
  fi
fi

# Step 3: Workers
echo "--- Starting worker A: $STRATEGY_A (dashboard :$WORKER_A_PORT) ---"
RUST_LOG=info cargo run --release -p ramseynet-worker -- \
  --strategy "$STRATEGY_A" --k "$K" --ell "$ELL" --n "$N" \
  --server "$SERVER_URL" --init "$INIT_MODE" --port "$WORKER_A_PORT" \
  > "$LOG_A" 2>&1 &
PIDS+=($!)

echo "--- Starting worker B: $STRATEGY_B (dashboard :$WORKER_B_PORT) ---"
RUST_LOG=info cargo run --release -p ramseynet-worker -- \
  --strategy "$STRATEGY_B" --k "$K" --ell "$ELL" --n "$N" \
  --server "$SERVER_URL" --init "$INIT_MODE" --port "$WORKER_B_PORT" \
  > "$LOG_B" 2>&1 &
PIDS+=($!)

echo ""
echo "=========================================="
echo "  Experiment running!"
echo "=========================================="
echo ""
echo "  Dashboards:"
echo "    $STRATEGY_A:  http://localhost:$WORKER_A_PORT"
echo "    $STRATEGY_B:  http://localhost:$WORKER_B_PORT"
echo ""
echo "  Let it run for ~${DURATION} minutes, then press Ctrl+C."
echo ""

print_analysis() {
  echo ""
  echo "=========================================="
  echo "  Experiment Results"
  echo "=========================================="
  echo ""

  # Strategy A stats
  local a_rounds=$(grep -c 'round_summary' "$LOG_A" 2>/dev/null || echo "0")
  local a_admitted=$(grep -c 'admitted to leaderboard' "$LOG_A" 2>/dev/null || echo "0")
  local a_last=$(grep 'round_summary' "$LOG_A" 2>/dev/null | tail -1 || echo "(no rounds)")

  # Strategy B stats
  local b_rounds=$(grep -c 'round_summary' "$LOG_B" 2>/dev/null || echo "0")
  local b_admitted=$(grep -c 'admitted to leaderboard' "$LOG_B" 2>/dev/null || echo "0")
  local b_last=$(grep 'round_summary' "$LOG_B" 2>/dev/null | tail -1 || echo "(no rounds)")

  echo "  $STRATEGY_A:"
  echo "    Rounds completed:   $a_rounds"
  echo "    Leaderboard admits: $a_admitted"
  echo "    Last round:         $a_last"
  echo ""
  echo "  $STRATEGY_B:"
  echo "    Rounds completed:   $b_rounds"
  echo "    Leaderboard admits: $b_admitted"
  echo "    Last round:         $b_last"
  echo ""
  echo "  Logs saved to: $LOGDIR/"
  echo ""
  echo "=========================================="
  echo "  To analyze further, send Claude:"
  echo "=========================================="
  echo ""
  echo "  Experiment logs are in $LOGDIR/"
  echo "  Run these commands and paste the output:"
  echo ""
  echo "    grep 'round_summary' $LOG_A"
  echo "    grep 'round_summary' $LOG_B"
  echo ""
  echo "  For tree2 depth detail (if available):"
  echo "    grep 'depth complete' $LOG_B"
  echo ""
}

# Wait for Ctrl+C
wait
