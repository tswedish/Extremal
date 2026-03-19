#!/usr/bin/env bash
# 10-minute experiment to verify leaderboard dynamics with larger capacity
# Usage: ./scripts/experiment-10min.sh
set -euo pipefail
cd "$(dirname "$0")/.."

echo "=== MineGraph 10-Minute Experiment ==="
echo ""
echo "Goal: Verify workers can still improve the leaderboard with"
echo "      increased capacity (2000) and noise flips for diversity."
echo ""

# Step 1: Restart server with larger capacity
echo "Step 1: Start server with --leaderboard-capacity 2000"
echo "        (in another terminal, run:)"
echo ""
echo "  cargo run --release -p minegraph-server -- \\"
echo "    --server-key .config/minegraph/server-key.json \\"
echo "    --leaderboard-capacity 2000"
echo ""
read -p "Press Enter when server is running with capacity 2000..."

# Step 2: Run fleet with noise for diversity
echo ""
echo "Step 2: Launching 4 workers with noise-flips for exploration diversity"
echo "        Running for 10 minutes..."
echo ""

LOG_DIR="logs/experiment-10min-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$LOG_DIR"

# Save experiment config
cat > "$LOG_DIR/config.json" <<EOF
{
    "experiment": "10min-capacity-test",
    "leaderboard_capacity": 2000,
    "workers": 4,
    "n": 25,
    "beam_width": 80,
    "max_depth": 12,
    "noise_flips": 3,
    "sample_bias": 0.8,
    "max_iters": 100000,
    "duration_minutes": 10,
    "started": "$(date -Iseconds)"
}
EOF

# Build release
echo "Building..."
cargo build --release -p minegraph-worker 2>&1 | tail -1
WORKER_BIN="target/release/minegraph-worker"

# Snapshot leaderboard before
echo "Leaderboard before:"
curl -s http://localhost:3001/api/leaderboards/25/threshold | python3 -m json.tool
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

    echo ""
    echo "=== Results ==="
    echo ""

    # Snapshot leaderboard after
    echo "Leaderboard after:"
    curl -s http://localhost:3001/api/leaderboards/25/threshold | python3 -m json.tool
    echo ""

    # Summarize worker logs
    for i in 1 2 3 4; do
        LOG="$LOG_DIR/worker-$i.log"
        if [[ -f "$LOG" ]]; then
            # Strip ANSI codes before grepping
            CLEAN=$(sed 's/\x1b\[[0-9;]*m//g' "$LOG")
            ROUNDS=$(echo "$CLEAN" | grep -c "round complete" 2>/dev/null || echo 0)
            ADMITTED=$(echo "$CLEAN" | grep -o 'total_admitted=[0-9]*' | tail -1 | grep -o '[0-9]*' || echo 0)
            DISC=$(echo "$CLEAN" | grep -o 'total_discoveries=[0-9]*' | tail -1 | grep -o '[0-9]*' || echo 0)
            echo "  Worker $i: $ROUNDS rounds, $DISC discoveries, $ADMITTED admitted"
        fi
    done

    echo ""
    echo "Logs: $LOG_DIR"
    echo "Experiment complete."
}
trap cleanup INT TERM EXIT

COMMIT_HASH=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")

# Launch workers with noise flips for diversity
for i in 1 2 3 4; do
    NO_COLOR=1 RUST_LOG=info "$WORKER_BIN" \
        --server http://localhost:3001 \
        --n 25 \
        --target-k 5 --target-ell 5 \
        --beam-width 80 --max-depth 12 \
        --sample-bias 0.8 \
        --max-iters 100000 \
        --noise-flips 3 \
        --metadata "{\"worker_id\":\"exp10-$i\",\"commit_hash\":\"$COMMIT_HASH\"}" \
        > "$LOG_DIR/worker-$i.log" 2>&1 &
    PIDS+=($!)
    echo "  Started worker $i (PID ${PIDS[-1]})"
done

echo ""
echo "Running for 10 minutes. Watch the leaderboard at http://localhost:5173/leaderboards/25"
echo "Press Ctrl+C to stop early."
echo ""

# Wait 10 minutes
sleep 600

# Will trigger cleanup via EXIT trap
