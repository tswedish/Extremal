#!/usr/bin/env bash
# Run a quick A/B comparison between two worker configs.
# Usage: ./scripts/agent-ab-test.sh --duration 10m \
#   --a "--beam-width 150 --noise-flips 2" \
#   --b "--beam-width 150 --noise-flips 4 --polish-max-steps 500"
#
# Runs 2 workers (A and B) for the specified duration, then compares:
# - Rounds completed
# - Discoveries found
# - Best local score
# - Discovery rate curve
#
# Results saved to logs/ab-test-<timestamp>/

set -euo pipefail
cd "$(dirname "$0")/.."

# Defaults
N=25
DURATION="10m"
SERVER="https://api.extremal.online"
ARGS_A=""
ARGS_B=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --n) N="$2"; shift 2 ;;
        --duration) DURATION="$2"; shift 2 ;;
        --server) SERVER="$2"; shift 2 ;;
        --a) ARGS_A="$2"; shift 2 ;;
        --b) ARGS_B="$2"; shift 2 ;;
        *) echo "Unknown arg: $1"; exit 1 ;;
    esac
done

if [[ -z "$ARGS_A" || -z "$ARGS_B" ]]; then
    echo "Usage: $0 --a '<worker-a-args>' --b '<worker-b-args>' [--duration 10m] [--n 25]"
    exit 1
fi

# Convert duration to seconds
DURATION_SECS=$(python3 -c "
s='$DURATION'
n=int(s[:-1]); u=s[-1]
print(n*3600 if u=='h' else n*60 if u=='m' else n)
" 2>/dev/null || echo 600)

COMMIT=$(git rev-parse --short HEAD 2>/dev/null || echo "?")
LOG_DIR="logs/ab-test-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$LOG_DIR"
BIN="target/release/extremal-worker"

echo "╔══════════════════════════════════════════════════╗"
echo "║  A/B Test                                        ║"
echo "╠══════════════════════════════════════════════════╣"
echo "║  n=$N  duration=$DURATION  commit=$COMMIT"
echo "║  A: $ARGS_A"
echo "║  B: $ARGS_B"
echo "║  logs: $LOG_DIR"
echo "╚══════════════════════════════════════════════════╝"
echo ""

# Build
echo "Building..."
cargo build --release -p extremal-worker 2>&1 | tail -1

# Launch both
echo "Launching workers..."
NO_COLOR=1 RUST_LOG=info $BIN --n "$N" --server "$SERVER" \
    --metadata '{"worker_id":"test-A","ab_test":true}' \
    $ARGS_A > "$LOG_DIR/worker-A.log" 2>&1 &
PID_A=$!

NO_COLOR=1 RUST_LOG=info $BIN --n "$N" --server "$SERVER" \
    --metadata '{"worker_id":"test-B","ab_test":true}' \
    $ARGS_B > "$LOG_DIR/worker-B.log" 2>&1 &
PID_B=$!

echo "  A: PID $PID_A"
echo "  B: PID $PID_B"
echo ""
echo "Running for $DURATION..."

# Wait
sleep "$DURATION_SECS"

# Kill
kill $PID_A $PID_B 2>/dev/null || true
sleep 2
kill -9 $PID_A $PID_B 2>/dev/null || true

echo ""
echo "=== Results ==="
echo ""

# Compare
for label in A B; do
    LOG="$LOG_DIR/worker-$label.log"
    ROUNDS=$(sed 's/\x1b\[[0-9;]*m//g' "$LOG" | grep -c "round complete" || echo 0)
    TOTAL_DISC=$(sed 's/\x1b\[[0-9;]*m//g' "$LOG" | grep "round complete" | tail -1 | grep -oP 'total_discoveries=\K[0-9]+' || echo 0)
    TOTAL_ADMIT=$(sed 's/\x1b\[[0-9;]*m//g' "$LOG" | grep "round complete" | tail -1 | grep -oP 'total_admitted=\K[0-9]+' || echo 0)
    LAST_MS=$(sed 's/\x1b\[[0-9;]*m//g' "$LOG" | grep "round complete" | tail -1 | grep -oP ' ms=\K[0-9]+' || echo 0)

    echo "Worker $label:"
    echo "  Rounds:      $ROUNDS"
    echo "  Discoveries: $TOTAL_DISC"
    echo "  Admitted:    $TOTAL_ADMIT"
    echo "  Last round:  ${LAST_MS}ms"
    echo ""
done

# Save config
cat > "$LOG_DIR/config.json" <<EOF
{
    "type": "ab-test",
    "n": $N,
    "duration": "$DURATION",
    "commit": "$COMMIT",
    "args_a": "$ARGS_A",
    "args_b": "$ARGS_B"
}
EOF

echo "Logs saved to $LOG_DIR"
