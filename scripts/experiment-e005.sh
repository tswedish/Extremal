#!/usr/bin/env bash
# E005: Long-running production search with signed metadata
#
# 16 workers, best-known config (focused: beam=80, depth=12, bias=0.8).
# All submissions include commit hash and worker ID via metadata.
# Uses default signing key from .config/minegraph/key.json if available.
#
# Usage:
#   # Start server first (in a separate terminal or tmux pane):
#   ./scripts/experiment-e005.sh server
#
#   # Then start the fleet (in another terminal):
#   ./scripts/experiment-e005.sh fleet
#
#   # Or run headless with nohup:
#   nohup ./scripts/experiment-e005.sh server > /dev/null 2>&1 &
#   nohup ./scripts/experiment-e005.sh fleet  > /dev/null 2>&1 &
#
#   # Check progress:
#   cat logs/e005/status.txt
#
#   # Full analysis:
#   ./scripts/analyze_experiment.sh logs/e005/fleet/

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"
source "$HOME/.cargo/env" 2>/dev/null || true

# ── Configuration ─────────────────────────────────────────────────────

EXPERIMENT="e005"
NUM_WORKERS=16
STRATEGY="tree2"
K=5
ELL=5
N=25
SERVER_URL="http://localhost:3001"
LEADERBOARD_CAPACITY=2000
INIT_MODE="leaderboard"
MAX_ITERS=100000
BASE_PORT=9000
SNAPSHOT_MIN=10

# Best-known hyperparameters from E004
BEAM_WIDTH=80
MAX_DEPTH=12
SAMPLE_BIAS=0.8

# Provenance
COMMIT_HASH=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")

# ── Directory setup ───────────────────────────────────────────────────

LOGDIR="$REPO/logs/$EXPERIMENT"
FLEET_LOGDIR="$LOGDIR/fleet"
mkdir -p "$FLEET_LOGDIR"

cmd="${1:-help}"

# ── Server ────────────────────────────────────────────────────────────

if [ "$cmd" = "server" ]; then
    echo ""
    echo "=========================================="
    echo "  E005 Server"
    echo "=========================================="
    echo ""
    echo "  Leaderboard capacity: $LEADERBOARD_CAPACITY"
    echo "  Database:             ramseynet.db"
    echo "  Log:                  $LOGDIR/server.log"
    echo ""
    echo "  Stop with Ctrl+C"
    echo "=========================================="
    echo ""

    # Build server binary
    echo "--- Building release binary ---"
    cargo build --release -p ramseynet-server --quiet 2>&1

    RUST_LOG=ramseynet=info,tower_http=warn \
        cargo run --release -p ramseynet-server -- \
        --leaderboard-capacity "$LEADERBOARD_CAPACITY" \
        2>&1 | tee "$LOGDIR/server.log"

    exit 0
fi

# ── Fleet ─────────────────────────────────────────────────────────────

if [ "$cmd" = "fleet" ]; then
    START_EPOCH=$(date +%s)

    # Write experiment config
    cat > "$LOGDIR/config.txt" <<EOF
Experiment: $EXPERIMENT
Started:    $(date)
Commit:     $COMMIT_HASH
Target:     R($K,$ELL) n=$N
Strategy:   $STRATEGY
Workers:    $NUM_WORKERS
Config:     beam=$BEAM_WIDTH depth=$MAX_DEPTH bias=$SAMPLE_BIAS
Init:       $INIT_MODE
Max iters:  $MAX_ITERS
Server:     $SERVER_URL (capacity=$LEADERBOARD_CAPACITY)
Base port:  $BASE_PORT
Snapshot:   every ${SNAPSHOT_MIN}m
Logs:       $FLEET_LOGDIR/
EOF

    echo ""
    echo "=========================================="
    echo "  E005: Production Search Fleet"
    echo "=========================================="
    echo ""
    echo "  Target:   R($K,$ELL) n=$N"
    echo "  Strategy: $STRATEGY"
    echo "  Workers:  $NUM_WORKERS"
    echo "  Config:   beam=$BEAM_WIDTH depth=$MAX_DEPTH bias=$SAMPLE_BIAS"
    echo "  Commit:   $COMMIT_HASH"
    echo "  Init:     $INIT_MODE"
    echo "  Server:   $SERVER_URL"
    echo ""

    # Check signing key
    KEY_FILE="$REPO/.config/minegraph/key.json"
    if [ -f "$KEY_FILE" ]; then
        KEY_ID=$(python3 -c "import json; print(json.load(open('$KEY_FILE'))['key_id'])" 2>/dev/null || echo "?")
        echo "  Key:      $KEY_ID (from $KEY_FILE)"
    else
        echo "  Key:      anonymous (no key.json found)"
    fi
    echo ""

    # Build worker binary
    echo "--- Building release binary ---"
    cargo build --release -p ramseynet-worker --quiet 2>&1
    WORKER_BIN="$REPO/target/release/ramseynet-worker"

    # Health check
    if curl -sf "$SERVER_URL/api/health" > /dev/null 2>&1; then
        echo "--- Server healthy at $SERVER_URL ---"
    else
        echo ""
        echo "  ERROR: Server at $SERVER_URL not responding."
        echo "  Start the server first:"
        echo ""
        echo "    ./scripts/experiment-e005.sh server"
        echo ""
        exit 1
    fi

    # Track PIDs
    PIDS=()
    SNAPSHOT_PID=""

    # ── Summary function ──────────────────────────────────────────────

    write_summary() {
        local dest="$1"
        local now_epoch=$(date +%s)
        local elapsed_sec=$(( now_epoch - START_EPOCH ))
        local elapsed_min=$(( elapsed_sec / 60 ))
        local elapsed_hr=$(awk "BEGIN {printf \"%.1f\", $elapsed_sec / 3600}")

        local total_rounds=0
        local total_discoveries=0
        local total_admitted=0
        local total_submitted=0

        {
            echo "=========================================="
            echo "  E005 Status — $(date)"
            echo "  Elapsed: ${elapsed_min}m (${elapsed_hr}h)"
            echo "  Commit:  $COMMIT_HASH"
            echo "  Config:  beam=$BEAM_WIDTH depth=$MAX_DEPTH bias=$SAMPLE_BIAS"
            echo "=========================================="
            echo ""

            for i in $(seq 0 $((NUM_WORKERS - 1))); do
                logfile="$FLEET_LOGDIR/worker-${i}.log"
                last=$(grep 'round_summary' "$logfile" 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g' | tail -1 || true)
                if [ -n "$last" ]; then
                    rounds=$(echo "$last" | grep -oP 'round=\K[0-9]+' || echo "0")
                    disc=$(echo "$last" | grep -oP 'total_discoveries=\K[0-9]+' || echo "0")
                    admit=$(echo "$last" | grep -oP 'total_admitted=\K[0-9]+' || echo "0")
                    submit=$(echo "$last" | grep -oP 'total_submitted=\K[0-9]+' || echo "0")
                    total_rounds=$((total_rounds + rounds))
                    total_discoveries=$((total_discoveries + disc))
                    total_admitted=$((total_admitted + admit))
                    total_submitted=$((total_submitted + submit))
                    printf "  Worker %2d: %6d rounds, %10d disc, %6d admitted\n" "$i" "$rounds" "$disc" "$admit"
                else
                    printf "  Worker %2d: (no data)\n" "$i"
                fi
            done

            echo ""
            echo "  ────────────────────────────────────"
            echo "  Fleet totals:"
            echo "    Elapsed:      ${elapsed_min}m (${elapsed_hr}h)"
            echo "    Rounds:       $total_rounds"
            echo "    Discoveries:  $total_discoveries"
            echo "    Submitted:    $total_submitted"
            echo "    Admitted:     $total_admitted"
            if [ "$total_submitted" -gt 0 ]; then
                rate=$(awk "BEGIN {printf \"%.1f\", ($total_admitted / $total_submitted) * 100}")
                echo "    Admit rate:   ${rate}%"
            fi
            if [ "$elapsed_sec" -gt 60 ]; then
                admits_per_hr=$(awk "BEGIN {printf \"%.0f\", $total_admitted / ($elapsed_sec / 3600.0)}")
                disc_per_hr=$(awk "BEGIN {printf \"%.0f\", $total_discoveries / ($elapsed_sec / 3600.0)}")
                rounds_per_hr=$(awk "BEGIN {printf \"%.0f\", $total_rounds / ($elapsed_sec / 3600.0)}")
                echo "    Admits/hr:    $admits_per_hr"
                echo "    Disc/hr:      $disc_per_hr"
                echo "    Rounds/hr:    $rounds_per_hr"
            fi
            echo ""
            echo "  Logs: $FLEET_LOGDIR/"
            echo "  Analysis: ./scripts/analyze_experiment.sh $FLEET_LOGDIR/"
            echo "=========================================="
        } > "$dest"
    }

    # ── Cleanup ───────────────────────────────────────────────────────

    cleanup() {
        if [ -n "$SNAPSHOT_PID" ]; then
            kill "$SNAPSHOT_PID" 2>/dev/null || true
        fi
        echo ""
        echo "--- Stopping $NUM_WORKERS workers ---"
        for pid in "${PIDS[@]}"; do
            kill "$pid" 2>/dev/null || true
        done
        wait 2>/dev/null || true

        write_summary "$LOGDIR/results.txt"
        cat "$LOGDIR/results.txt"
        cp "$LOGDIR/results.txt" "$LOGDIR/status.txt"
    }
    trap cleanup EXIT INT TERM

    # ── Launch workers ────────────────────────────────────────────────

    echo "--- Launching $NUM_WORKERS workers ---"
    echo ""

    for i in $(seq 0 $((NUM_WORKERS - 1))); do
        port=$((BASE_PORT + i))
        logfile="$FLEET_LOGDIR/worker-${i}.log"

        RUST_LOG=info "$WORKER_BIN" \
            --strategy "$STRATEGY" --k "$K" --ell "$ELL" --n "$N" \
            --server "$SERVER_URL" --init "$INIT_MODE" --port "$port" \
            --max-iters "$MAX_ITERS" \
            --beam-width "$BEAM_WIDTH" --max-depth "$MAX_DEPTH" --sample-bias "$SAMPLE_BIAS" \
            --commit-hash "$COMMIT_HASH" --worker-id "$i" \
            > "$logfile" 2>&1 &
        PIDS+=($!)
    done

    echo "  Dashboards:"
    echo ""
    for i in $(seq 0 $((NUM_WORKERS - 1))); do
        port=$((BASE_PORT + i))
        printf "    Worker %2d: http://localhost:%d\n" "$i" "$port"
    done

    echo ""
    echo "  Check progress:"
    echo "    cat $LOGDIR/status.txt"
    echo ""
    echo "=========================================="
    echo "  Fleet running. Press Ctrl+C to stop."
    echo "  Snapshots every ${SNAPSHOT_MIN}m"
    echo "=========================================="
    echo ""

    # Periodic snapshot loop
    (
        while true; do
            sleep $((SNAPSHOT_MIN * 60))
            write_summary "$LOGDIR/status.txt" 2>/dev/null || true
        done
    ) &
    SNAPSHOT_PID=$!

    # Wait for workers
    wait "${PIDS[@]}" 2>/dev/null || true
    exit 0
fi

# ── Help ──────────────────────────────────────────────────────────────

echo ""
echo "E005: Long-running production search with signed metadata"
echo ""
echo "Usage:"
echo "  $0 server   — Start the server (Terminal 1)"
echo "  $0 fleet    — Start 16 workers (Terminal 2)"
echo ""
echo "For headless (overnight) runs:"
echo "  nohup $0 server > /dev/null 2>&1 &"
echo "  nohup $0 fleet  > /dev/null 2>&1 &"
echo ""
echo "Check progress:  cat logs/$EXPERIMENT/status.txt"
echo "Full analysis:   ./scripts/analyze_experiment.sh logs/$EXPERIMENT/fleet/"
echo ""
