#!/usr/bin/env bash
# MineGraph fleet launcher
# Usage: ./scripts/fleet.sh [--workers N] [--n N] [--release]
set -euo pipefail
cd "$(dirname "$0")/.."

WORKERS=4
N=25
TARGET_K=5
TARGET_ELL=5
BEAM_WIDTH=80
MAX_DEPTH=12
SAMPLE_BIAS=0.8
MAX_ITERS=100000
SERVER="http://localhost:3001"
DASHBOARD=""
RELEASE=""
LOG_DIR="logs/fleet-$(date +%Y%m%d-%H%M%S)"

while [[ $# -gt 0 ]]; do
    case $1 in
        --workers) WORKERS=$2; shift 2 ;;
        --n) N=$2; shift 2 ;;
        --target-k) TARGET_K=$2; shift 2 ;;
        --target-ell) TARGET_ELL=$2; shift 2 ;;
        --beam-width) BEAM_WIDTH=$2; shift 2 ;;
        --max-depth) MAX_DEPTH=$2; shift 2 ;;
        --sample-bias) SAMPLE_BIAS=$2; shift 2 ;;
        --max-iters) MAX_ITERS=$2; shift 2 ;;
        --server) SERVER=$2; shift 2 ;;
        --dashboard) DASHBOARD=$2; shift 2 ;;
        --release) RELEASE="--release"; shift ;;
        *) echo "Unknown arg: $1"; exit 1 ;;
    esac
done

mkdir -p "$LOG_DIR"

echo "=== MineGraph Fleet ==="
echo "Workers:     $WORKERS"
echo "Target:      n=$N, R($TARGET_K,$TARGET_ELL)"
echo "Beam:        width=$BEAM_WIDTH depth=$MAX_DEPTH bias=$SAMPLE_BIAS"
echo "Max iters:   $MAX_ITERS"
echo "Server:      $SERVER"
echo "Dashboard:   ${DASHBOARD:-none}"
echo "Logs:        $LOG_DIR"
echo "========================"

# Build first
echo "Building worker..."
cargo build -p minegraph-worker $RELEASE 2>&1 | tail -1

WORKER_BIN="target/debug/minegraph-worker"
if [[ -n "$RELEASE" ]]; then
    WORKER_BIN="target/release/minegraph-worker"
fi

# Save config
cat > "$LOG_DIR/config.json" <<EOF
{
    "workers": $WORKERS,
    "n": $N,
    "target_k": $TARGET_K,
    "target_ell": $TARGET_ELL,
    "beam_width": $BEAM_WIDTH,
    "max_depth": $MAX_DEPTH,
    "sample_bias": $SAMPLE_BIAS,
    "max_iters": $MAX_ITERS,
    "server": "$SERVER",
    "started": "$(date -Iseconds)"
}
EOF

PIDS=()
STOPPED=0

cleanup() {
    # Prevent re-entry
    if [[ "$STOPPED" -eq 1 ]]; then return; fi
    STOPPED=1
    echo ""
    echo "Stopping fleet..."
    for pid in "${PIDS[@]}"; do
        kill "$pid" 2>/dev/null || true
    done
    wait 2>/dev/null || true
    rm -f "$LOG_DIR/pids"
    echo "Fleet stopped."
}
trap cleanup INT TERM

COMMIT_HASH=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
echo "Commit:      $COMMIT_HASH"

for i in $(seq 1 "$WORKERS"); do
    LOG_FILE="$LOG_DIR/worker-$i.log"
    echo "Starting worker $i -> $LOG_FILE"
    DASH_FLAG=""
    if [[ -n "$DASHBOARD" ]]; then
        DASH_FLAG="--dashboard $DASHBOARD"
    fi
    NO_COLOR=1 RUST_LOG=info "$WORKER_BIN" \
        --server "$SERVER" \
        --n "$N" \
        --target-k "$TARGET_K" \
        --target-ell "$TARGET_ELL" \
        --beam-width "$BEAM_WIDTH" \
        --max-depth "$MAX_DEPTH" \
        --sample-bias "$SAMPLE_BIAS" \
        --max-iters "$MAX_ITERS" \
        --metadata "{\"worker_id\":\"fleet-$i\",\"commit_hash\":\"$COMMIT_HASH\"}" \
        $DASH_FLAG \
        > "$LOG_FILE" 2>&1 &
    PIDS+=($!)
done

# Save PIDs to file for manual cleanup
printf '%s\n' "${PIDS[@]}" > "$LOG_DIR/pids"

echo ""
echo "Fleet running ($WORKERS workers). Ctrl+C to stop."
echo "PIDs saved to $LOG_DIR/pids"
echo "Monitor: tail -f $LOG_DIR/worker-1.log"
echo ""

# Wait for all workers (blocks until they exit or we get signaled)
wait "${PIDS[@]}" 2>/dev/null || true
cleanup
