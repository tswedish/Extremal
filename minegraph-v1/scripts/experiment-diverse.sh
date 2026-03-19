#!/usr/bin/env bash
# Diverse exploration experiment: push past convergence plateau on n=25
# Uses 12 workers with varied strategies to escape local optima
#
# Usage: ./scripts/experiment-diverse.sh [--duration MINS]
set -euo pipefail
cd "$(dirname "$0")/.."

DURATION_MINS=60
while [[ $# -gt 0 ]]; do
    case $1 in
        --duration) DURATION_MINS=$2; shift 2 ;;
        *) echo "Unknown: $1"; exit 1 ;;
    esac
done

DURATION_SECS=$((DURATION_MINS * 60))
COMMIT_HASH=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
LOG_DIR="logs/experiment-diverse-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$LOG_DIR"

echo "══════════════════════════════════════════════════════════"
echo "  MineGraph Diverse Exploration Experiment (n=25)"
echo "══════════════════════════════════════════════════════════"
echo ""
echo "  Duration:   ${DURATION_MINS} minutes"
echo "  Workers:    12 (4 groups x 3 workers)"
echo "  Commit:     $COMMIT_HASH"
echo ""
echo "  Group A (3 workers): Focused exploitation"
echo "    beam=80, depth=12, bias=0.8, noise=0"
echo "    Seeded from leaderboard top. Tries to improve best entries."
echo ""
echo "  Group B (3 workers): Noisy exploration"
echo "    beam=80, depth=12, bias=0.3, noise=8"
echo "    Heavy perturbation + low bias = explore diverse regions."
echo ""
echo "  Group C (3 workers): Deep narrow search"
echo "    beam=20, depth=30, bias=0.5, noise=3"
echo "    Narrow beam but very deep = find different local optima."
echo ""
echo "  Group D (3 workers): Wide shallow search"
echo "    beam=200, depth=5, bias=0.6, noise=5"
echo "    Very wide beam, shallow = broad coverage per round."
echo ""

# Verify server
if ! curl -s http://localhost:3001/api/health > /dev/null 2>&1; then
    echo "ERROR: Server not reachable at localhost:3001"
    exit 1
fi

THRESHOLD_BEFORE=$(curl -s http://localhost:3001/api/leaderboards/25/threshold 2>/dev/null)
echo "Leaderboard before:"
echo "$THRESHOLD_BEFORE" | python3 -m json.tool
echo ""

echo "Building..."
cargo build --release -p minegraph-worker 2>&1 | tail -1
WORKER_BIN="target/release/minegraph-worker"

cat > "$LOG_DIR/config.json" <<EOF
{
    "experiment": "diverse-exploration",
    "duration_mins": $DURATION_MINS,
    "commit_hash": "$COMMIT_HASH",
    "groups": {
        "A": {"desc": "focused exploitation", "beam": 80, "depth": 12, "bias": 0.8, "noise": 0},
        "B": {"desc": "noisy exploration", "beam": 80, "depth": 12, "bias": 0.3, "noise": 8},
        "C": {"desc": "deep narrow", "beam": 20, "depth": 30, "bias": 0.5, "noise": 3},
        "D": {"desc": "wide shallow", "beam": 200, "depth": 5, "bias": 0.6, "noise": 5}
    },
    "threshold_before": $THRESHOLD_BEFORE,
    "started": "$(date -Iseconds)"
}
EOF

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
    echo "══════════════════════════════════════════════════════════"
    echo "  RESULTS"
    echo "══════════════════════════════════════════════════════════"
    echo ""
    echo "Leaderboard after:"
    echo "$THRESHOLD_AFTER" | python3 -m json.tool
    echo ""

    cat > "$LOG_DIR/results.json" <<REOF
{
    "ended": "$(date -Iseconds)",
    "threshold_after": $THRESHOLD_AFTER
}
REOF

    echo "Per-worker results:"
    echo ""
    printf "  %-12s %-8s %-10s %-10s %-10s\n" "Worker" "Group" "Rounds" "Discovered" "Admitted"
    printf "  %-12s %-8s %-10s %-10s %-10s\n" "------" "-----" "------" "----------" "--------"
    TOTAL_R=0; TOTAL_D=0; TOTAL_A=0
    for LOG in "$LOG_DIR"/worker-*.log; do
        NAME=$(basename "$LOG" .log)
        GROUP=$(echo "$NAME" | sed 's/worker-\([A-D]\).*/\1/')
        CLEAN=$(sed 's/\x1b\[[0-9;]*m//g' "$LOG" 2>/dev/null)
        ROUNDS=$(echo "$CLEAN" | grep -c "round complete" 2>/dev/null || echo 0)
        DISC=$(echo "$CLEAN" | grep -o 'total_discoveries=[0-9]*' | tail -1 | grep -o '[0-9]*' || echo 0)
        ADMIT=$(echo "$CLEAN" | grep -o 'total_admitted=[0-9]*' | tail -1 | grep -o '[0-9]*' || echo 0)
        printf "  %-12s %-8s %-10s %-10s %-10s\n" "$NAME" "$GROUP" "$ROUNDS" "$DISC" "$ADMIT"
        TOTAL_R=$((TOTAL_R + ROUNDS))
        TOTAL_D=$((TOTAL_D + DISC))
        TOTAL_A=$((TOTAL_A + ADMIT))
    done
    echo ""
    printf "  %-12s %-8s %-10s %-10s %-10s\n" "TOTAL" "" "$TOTAL_R" "$TOTAL_D" "$TOTAL_A"

    echo ""
    echo "Logs: $LOG_DIR"
}
trap cleanup INT TERM EXIT

# ── Launch workers ───────────────────────────────

launch_worker() {
    local NAME=$1 BEAM=$2 DEPTH=$3 BIAS=$4 NOISE=$5 ITERS=$6 GROUP=$7
    NO_COLOR=1 RUST_LOG=info "$WORKER_BIN" \
        --server http://localhost:3001 \
        --n 25 --target-k 5 --target-ell 5 \
        --beam-width "$BEAM" --max-depth "$DEPTH" \
        --sample-bias "$BIAS" --max-iters "$ITERS" \
        --noise-flips "$NOISE" \
        --metadata "{\"worker_id\":\"$NAME\",\"commit_hash\":\"$COMMIT_HASH\",\"group\":\"$GROUP\",\"beam\":$BEAM,\"depth\":$DEPTH,\"bias\":$BIAS,\"noise\":$NOISE}" \
        > "$LOG_DIR/worker-$NAME.log" 2>&1 &
    PIDS+=($!)
    echo "  Started $NAME (group=$GROUP beam=$BEAM depth=$DEPTH bias=$BIAS noise=$NOISE)"
}

echo "Launching 12 workers..."
echo ""

# Group A: Focused exploitation (leaderboard top, no noise)
launch_worker "A1" 80 12 0.8 0 100000 "A-focused"
launch_worker "A2" 80 12 0.9 0 100000 "A-focused"
launch_worker "A3" 80 12 0.7 0 100000 "A-focused"

# Group B: Noisy exploration (heavy perturbation, low bias)
launch_worker "B1" 80 12 0.3 8  100000 "B-noisy"
launch_worker "B2" 80 12 0.2 10 100000 "B-noisy"
launch_worker "B3" 80 12 0.4 6  100000 "B-noisy"

# Group C: Deep narrow search
launch_worker "C1" 20 30 0.5 3 200000 "C-deep"
launch_worker "C2" 15 40 0.5 4 200000 "C-deep"
launch_worker "C3" 25 25 0.6 2 200000 "C-deep"

# Group D: Wide shallow search
launch_worker "D1" 200 5 0.6 5 80000 "D-wide"
launch_worker "D2" 250 4 0.5 6 80000 "D-wide"
launch_worker "D3" 150 6 0.7 4 80000 "D-wide"

printf '%s\n' "${PIDS[@]}" > "$LOG_DIR/pids"

echo ""
echo "Running for ${DURATION_MINS}m. Ctrl+C to stop early."
echo "  Monitor: tail -f $LOG_DIR/worker-A1.log"
echo "  Dashboard: http://localhost:5173/dashboard"
echo ""

# ── Status loop ──────────────────────────────────
ELAPSED=0
INTERVAL=60
while [[ $ELAPSED -lt $DURATION_SECS ]]; do
    sleep "$INTERVAL"
    ELAPSED=$((ELAPSED + INTERVAL))
    MINS=$((ELAPSED / 60))

    ALIVE=0
    for pid in "${PIDS[@]}"; do
        if kill -0 "$pid" 2>/dev/null; then ALIVE=$((ALIVE + 1)); fi
    done

    COUNT=$(curl -s "http://localhost:3001/api/leaderboards/25/threshold" 2>/dev/null \
        | python3 -c "import sys,json; print(json.load(sys.stdin).get('count',0))" 2>/dev/null || echo "?")

    # Quick per-group admit summary
    A_ADMIT=0; B_ADMIT=0; C_ADMIT=0; D_ADMIT=0
    for LOG in "$LOG_DIR"/worker-A*.log; do
        V=$(sed 's/\x1b\[[0-9;]*m//g' "$LOG" 2>/dev/null | grep -o 'total_admitted=[0-9]*' | tail -1 | grep -o '[0-9]*' || echo 0)
        A_ADMIT=$((A_ADMIT + V))
    done
    for LOG in "$LOG_DIR"/worker-B*.log; do
        V=$(sed 's/\x1b\[[0-9;]*m//g' "$LOG" 2>/dev/null | grep -o 'total_admitted=[0-9]*' | tail -1 | grep -o '[0-9]*' || echo 0)
        B_ADMIT=$((B_ADMIT + V))
    done
    for LOG in "$LOG_DIR"/worker-C*.log; do
        V=$(sed 's/\x1b\[[0-9;]*m//g' "$LOG" 2>/dev/null | grep -o 'total_admitted=[0-9]*' | tail -1 | grep -o '[0-9]*' || echo 0)
        C_ADMIT=$((C_ADMIT + V))
    done
    for LOG in "$LOG_DIR"/worker-D*.log; do
        V=$(sed 's/\x1b\[[0-9;]*m//g' "$LOG" 2>/dev/null | grep -o 'total_admitted=[0-9]*' | tail -1 | grep -o '[0-9]*' || echo 0)
        D_ADMIT=$((D_ADMIT + V))
    done

    echo "[${MINS}m] Workers: $ALIVE/12 | LB: $COUNT | A:$A_ADMIT B:$B_ADMIT C:$C_ADMIT D:$D_ADMIT"

    if [[ "$ALIVE" -eq 0 ]]; then
        echo "All workers exited."
        break
    fi
done
