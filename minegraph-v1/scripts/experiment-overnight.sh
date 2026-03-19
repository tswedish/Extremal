#!/usr/bin/env bash
# Overnight experiment: sustained search with large leaderboard
# Usage: ./scripts/experiment-overnight.sh [--hours N]
set -euo pipefail
cd "$(dirname "$0")/.."

HOURS=8
WORKERS=8

while [[ $# -gt 0 ]]; do
    case $1 in
        --hours) HOURS=$2; shift 2 ;;
        --workers) WORKERS=$2; shift 2 ;;
        *) echo "Unknown: $1"; exit 1 ;;
    esac
done

DURATION_SECS=$((HOURS * 3600))

echo "=== MineGraph Overnight Experiment ==="
echo ""
echo "Duration:    ${HOURS}h"
echo "Workers:     $WORKERS"
echo "Capacity:    2000 (server must be started with --leaderboard-capacity 2000)"
echo ""
echo "Prerequisites:"
echo "  1. Server running with: cargo run --release -p minegraph-server -- \\"
echo "       --server-key .config/minegraph/server-key.json \\"
echo "       --leaderboard-capacity 2000"
echo "  2. Web UI at http://localhost:5173 (optional, for monitoring)"
echo ""

# Verify server is up
if ! curl -s http://localhost:3001/api/health > /dev/null 2>&1; then
    echo "ERROR: Server not reachable at localhost:3001"
    exit 1
fi

echo "Server is up. Starting experiment..."
echo ""

LOG_DIR="logs/experiment-overnight-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$LOG_DIR"

# Save config
THRESHOLD_BEFORE=$(curl -s http://localhost:3001/api/leaderboards/25/threshold 2>/dev/null)
cat > "$LOG_DIR/config.json" <<EOF
{
    "experiment": "overnight",
    "hours": $HOURS,
    "workers": $WORKERS,
    "n": 25,
    "beam_width": 80,
    "max_depth": 12,
    "noise_flips": 3,
    "sample_bias": 0.8,
    "max_iters": 100000,
    "started": "$(date -Iseconds)",
    "threshold_before": $THRESHOLD_BEFORE
}
EOF

# Build
echo "Building..."
cargo build --release -p minegraph-worker 2>&1 | tail -1
WORKER_BIN="target/release/minegraph-worker"

echo "Leaderboard before:"
echo "$THRESHOLD_BEFORE" | python3 -m json.tool
echo ""

PIDS=()
STOPPED=0

cleanup() {
    if [[ "$STOPPED" -eq 1 ]]; then return; fi
    STOPPED=1
    echo ""
    echo "Stopping workers..."
    for pid in "${PIDS[@]}"; do kill "$pid" 2>/dev/null || true; done
    wait 2>/dev/null || true

    THRESHOLD_AFTER=$(curl -s http://localhost:3001/api/leaderboards/25/threshold 2>/dev/null)

    echo ""
    echo "══════════════════════════════════════════"
    echo "  EXPERIMENT RESULTS"
    echo "══════════════════════════════════════════"
    echo ""
    echo "Leaderboard after:"
    echo "$THRESHOLD_AFTER" | python3 -m json.tool
    echo ""

    # Save results
    cat > "$LOG_DIR/results.json" <<REOF
{
    "ended": "$(date -Iseconds)",
    "threshold_after": $THRESHOLD_AFTER
}
REOF

    # Summarize per worker
    echo "Worker summary:"
    TOTAL_ROUNDS=0
    TOTAL_DISC=0
    TOTAL_ADMIT=0
    for i in $(seq 1 "$WORKERS"); do
        LOG="$LOG_DIR/worker-$i.log"
        if [[ -f "$LOG" ]]; then
            # Strip ANSI codes before grepping
            CLEAN=$(sed 's/\x1b\[[0-9;]*m//g' "$LOG")
            ROUNDS=$(echo "$CLEAN" | grep -c "round complete" 2>/dev/null || echo 0)
            ADMITTED=$(echo "$CLEAN" | grep -o 'total_admitted=[0-9]*' | tail -1 | grep -o '[0-9]*' || echo 0)
            DISC=$(echo "$CLEAN" | grep -o 'total_discoveries=[0-9]*' | tail -1 | grep -o '[0-9]*' || echo 0)
            echo "  Worker $i: $ROUNDS rounds, $DISC discoveries, $ADMITTED admitted"
            TOTAL_ROUNDS=$((TOTAL_ROUNDS + ROUNDS))
            TOTAL_DISC=$((TOTAL_DISC + DISC))
            TOTAL_ADMIT=$((TOTAL_ADMIT + ADMITTED))
        fi
    done
    echo ""
    echo "  Total: $TOTAL_ROUNDS rounds, $TOTAL_DISC discoveries, $TOTAL_ADMIT admitted"
    echo ""
    echo "Logs: $LOG_DIR"
}
trap cleanup INT TERM EXIT

COMMIT_HASH=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
echo "Commit: $COMMIT_HASH"

# Launch workers: mix of strategies
# Workers 1-N/2: default (leaderboard seeding)
# Workers N/2+1-N: with noise flips (exploration)
for i in $(seq 1 "$WORKERS"); do
    NOISE=0
    if [[ $i -gt $((WORKERS / 2)) ]]; then NOISE=5; fi

    NO_COLOR=1 RUST_LOG=info "$WORKER_BIN" \
        --server http://localhost:3001 \
        --n 25 \
        --target-k 5 --target-ell 5 \
        --beam-width 80 --max-depth 12 \
        --sample-bias 0.8 \
        --max-iters 100000 \
        --noise-flips "$NOISE" \
        --metadata "{\"worker_id\":\"overnight-$i\",\"commit_hash\":\"$COMMIT_HASH\",\"noise_flips\":$NOISE}" \
        > "$LOG_DIR/worker-$i.log" 2>&1 &
    PIDS+=($!)
    echo "  Started worker $i (noise=$NOISE, PID ${PIDS[-1]})"
done

printf '%s\n' "${PIDS[@]}" > "$LOG_DIR/pids"

echo ""
echo "Running for ${HOURS}h ($WORKERS workers). Ctrl+C to stop early."
echo "PIDs: $LOG_DIR/pids"
echo "Monitor: tail -f $LOG_DIR/worker-1.log"
echo "Dashboard: http://localhost:5173/dashboard"
echo "Rain: http://localhost:5173/rain"
echo ""

# Periodic status every 5 minutes
ELAPSED=0
while [[ $ELAPSED -lt $DURATION_SECS ]]; do
    sleep 300
    ELAPSED=$((ELAPSED + 300))
    HOURS_ELAPSED=$(echo "scale=1; $ELAPSED / 3600" | bc)

    # Check if workers are still alive
    ALIVE=0
    for pid in "${PIDS[@]}"; do
        if kill -0 "$pid" 2>/dev/null; then ALIVE=$((ALIVE + 1)); fi
    done

    COUNT=$(curl -s http://localhost:3001/api/leaderboards/25/threshold 2>/dev/null | python3 -c "import sys,json; print(json.load(sys.stdin).get('count','?'))" 2>/dev/null || echo "?")
    echo "[${HOURS_ELAPSED}h] Workers: $ALIVE/$WORKERS | Leaderboard: $COUNT entries"

    if [[ "$ALIVE" -eq 0 ]]; then
        echo "All workers exited."
        break
    fi
done

# EXIT trap handles cleanup
