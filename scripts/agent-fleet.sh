#!/usr/bin/env bash
# Launch a fleet for the experiment agent.
# Usage: ./scripts/agent-fleet.sh [--workers 4] [--n 25] [--polish 100] [--duration 30m]
#
# Creates a log directory, builds release binary, ensures signing key exists
# and is registered, then launches workers with unique IDs and full metadata.

set -euo pipefail

# ── Defaults ─────────────────────────────────────────────
WORKERS=8
N=25
TARGET_K=5
TARGET_ELL=5
POLISH_MAX_STEPS=100
POLISH_TABU_TENURE=25
SCORE_BIAS_THRESHOLD=3
MAX_ITERS=500000
SERVER=http://localhost:3001
DASHBOARD=ws://localhost:4000/ws/worker
DURATION=""

# ── Parse args ───────────────────────────────────────────
while [[ $# -gt 0 ]]; do
    case "$1" in
        --workers) WORKERS="$2"; shift 2 ;;
        --n) N="$2"; shift 2 ;;
        --target-k) TARGET_K="$2"; shift 2 ;;
        --target-ell) TARGET_ELL="$2"; shift 2 ;;
        --polish) POLISH_MAX_STEPS="$2"; shift 2 ;;
        --polish-tenure) POLISH_TABU_TENURE="$2"; shift 2 ;;
        --score-bias) SCORE_BIAS_THRESHOLD="$2"; shift 2 ;;
        --max-iters) MAX_ITERS="$2"; shift 2 ;;
        --server) SERVER="$2"; shift 2 ;;
        --dashboard) DASHBOARD="$2"; shift 2 ;;
        --duration) DURATION="$2"; shift 2 ;;
        *) echo "Unknown arg: $1"; exit 1 ;;
    esac
done

# ── Setup ────────────────────────────────────────────────
COMMIT=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
STARTED=$(date -u +%Y-%m-%dT%H:%M:%SZ)
LOG_DIR="logs/agent-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$LOG_DIR"

echo "=== Agent Fleet ==="
echo "  commit: $COMMIT"
echo "  n=$N workers=$WORKERS polish=$POLISH_MAX_STEPS"
echo "  logs: $LOG_DIR"

# ── Kill existing workers ────────────────────────────────
if pgrep -f "extremal-worker" > /dev/null 2>&1; then
    echo "Stopping existing workers..."
    pkill -f "extremal-worker" 2>/dev/null || true
    sleep 2
    pkill -9 -f "extremal-worker" 2>/dev/null || true
fi

# ── Build ────────────────────────────────────────────────
echo "Building release binary..."
cargo build --release -p extremal-worker 2>&1 | tail -1
BIN="target/release/extremal-worker"

# ── Signing key ──────────────────────────────────────────
KEY_FILE=".config/extremal/key.json"
if [[ ! -f "$KEY_FILE" ]]; then
    echo "Generating signing key..."
    cargo run -p extremal-cli -- keygen --name "agent-$(hostname)" 2>&1 | tail -3
fi

KEY_ID=$(python3 -c "import json; print(json.load(open('$KEY_FILE'))['key_id'])" 2>/dev/null || echo "unknown")
echo "  key_id: $KEY_ID"

# Register key with server (idempotent)
echo "Registering key with server..."
cargo run -p extremal-cli -- register-key --server "$SERVER" 2>&1 | tail -1 || true

# ── Worker configs ───────────────────────────────────────
# Diverse configs: wide, focused, deep, explore
CONFIGS=(
    "wide-a:--beam-width 150 --max-depth 15 --noise-flips 2 --sample-bias 0.4"
    "wide-b:--beam-width 200 --max-depth 12 --noise-flips 2 --sample-bias 0.4"
    "wide-c:--beam-width 180 --max-depth 12 --noise-flips 2 --sample-bias 0.4"
    "wide-d:--beam-width 150 --max-depth 15 --noise-flips 1 --sample-bias 0.5"
    "focused:--beam-width 120 --max-depth 14 --focused true --noise-flips 2 --sample-bias 0.4"
    "explore:--beam-width 100 --max-depth 12 --noise-flips 3 --sample-bias 0.3"
    "deep-polish:--beam-width 150 --max-depth 10 --noise-flips 2 --sample-bias 0.4 --polish-max-steps 500"
    "deep-ils:--beam-width 150 --max-depth 10 --noise-flips 2 --sample-bias 0.4 --polish-max-steps 200"
)

# ── Launch ───────────────────────────────────────────────
PIDS=()
STOPPED=0

cleanup() {
    if [[ "$STOPPED" -eq 1 ]]; then return; fi
    STOPPED=1
    echo ""
    echo "Stopping fleet..."
    for pid in "${PIDS[@]}"; do
        kill "$pid" 2>/dev/null || true
    done
    wait "${PIDS[@]}" 2>/dev/null || true
    echo "Fleet stopped. Logs in $LOG_DIR"
}
trap cleanup INT TERM

for i in $(seq 0 $((WORKERS - 1))); do
    IDX=$((i % ${#CONFIGS[@]}))
    IFS=: read -r NAME ARGS <<< "${CONFIGS[$IDX]}"

    # Make names unique if more workers than configs
    if [[ $WORKERS -gt ${#CONFIGS[@]} ]] && [[ $i -ge ${#CONFIGS[@]} ]]; then
        NAME="${NAME}-$((i / ${#CONFIGS[@]} + 1))"
    fi

    LOG="$LOG_DIR/$NAME.log"
    META="{\"worker_id\":\"$NAME\",\"commit\":\"$COMMIT\",\"started\":\"$STARTED\"}"

    # Build global defaults, skipping any flags that per-worker ARGS override
    GLOBAL_FLAGS=""
    [[ "$ARGS" != *"--max-iters"* ]] && GLOBAL_FLAGS="$GLOBAL_FLAGS --max-iters $MAX_ITERS"
    [[ "$ARGS" != *"--polish-max-steps"* ]] && GLOBAL_FLAGS="$GLOBAL_FLAGS --polish-max-steps $POLISH_MAX_STEPS"
    [[ "$ARGS" != *"--polish-tabu-tenure"* ]] && GLOBAL_FLAGS="$GLOBAL_FLAGS --polish-tabu-tenure $POLISH_TABU_TENURE"
    [[ "$ARGS" != *"--score-bias-threshold"* ]] && GLOBAL_FLAGS="$GLOBAL_FLAGS --score-bias-threshold $SCORE_BIAS_THRESHOLD"

    FULL_CMD="--n $N --target-k $TARGET_K --target-ell $TARGET_ELL$GLOBAL_FLAGS $ARGS"
    NO_COLOR=1 RUST_LOG=info $BIN \
        --n "$N" \
        --target-k "$TARGET_K" --target-ell "$TARGET_ELL" \
        $GLOBAL_FLAGS \
        --server "$SERVER" --dashboard "$DASHBOARD" \
        --metadata "$META" \
        $ARGS \
        > "$LOG" 2>&1 &
    PIDS+=($!)
    echo "  $NAME (PID $!):$FULL_CMD"
done

printf '%s\n' "${PIDS[@]}" > "$LOG_DIR/pids"

# ── Save config ──────────────────────────────────────────
cat > "$LOG_DIR/config.json" <<EOF
{
    "commit": "$COMMIT",
    "started": "$STARTED",
    "n": $N,
    "target_k": $TARGET_K,
    "target_ell": $TARGET_ELL,
    "workers": $WORKERS,
    "polish_max_steps": $POLISH_MAX_STEPS,
    "polish_tabu_tenure": $POLISH_TABU_TENURE,
    "score_bias_threshold": $SCORE_BIAS_THRESHOLD,
    "max_iters": $MAX_ITERS,
    "server": "$SERVER",
    "dashboard": "$DASHBOARD",
    "key_id": "$KEY_ID",
    "log_dir": "$LOG_DIR"
}
EOF

echo ""
echo "Fleet running ($WORKERS workers). Logs: $LOG_DIR"
echo "Monitor: ./scripts/agent-status.sh $LOG_DIR"
echo "Stop:    kill \$(cat $LOG_DIR/pids) or Ctrl-C"

# ── Wait (optional duration) ─────────────────────────────
if [[ -n "$DURATION" ]]; then
    SECS=$(python3 -c "
s='$DURATION'
n=int(s[:-1])
u=s[-1]
print(n*3600 if u=='h' else n*60 if u=='m' else n)
" 2>/dev/null || echo 600)
    echo "Will stop after $DURATION ($SECS seconds)..."
    sleep "$SECS" &
    SLEEP_PID=$!
    wait "$SLEEP_PID" 2>/dev/null || true
    cleanup
else
    # Wait forever (Ctrl-C to stop)
    wait "${PIDS[@]}" 2>/dev/null || true
fi
