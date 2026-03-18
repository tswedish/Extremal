#!/usr/bin/env bash
# E006: Focused Edge Flipping — Head-to-Head Comparison
#
# Split fleet: 8 focused workers vs 8 unfocused workers, all against the
# same production server. This directly tests the hypothesis that focused
# edge flipping (only mutating edges in violations) produces more admits/hr
# than the baseline (mutating all 300 edges).
#
# Based on Exoo & Tatarevic (2015) Algorithm 2.
#
# Usage:
#   # Make sure the production server is running (from ~/RamseyNet):
#   #   ./scripts/experiment-e005.sh server
#   # Or from this dev worktree:
#   #   ./scripts/experiment-e006.sh server
#
#   # Then start the split fleet:
#   ./scripts/experiment-e006.sh fleet
#
#   # Check progress:
#   cat logs/e006/status.txt
#
#   # For headless (overnight) runs:
#   nohup ./scripts/experiment-e006.sh fleet > /dev/null 2>&1 &

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"
source "$HOME/.cargo/env" 2>/dev/null || true

# ── Configuration ─────────────────────────────────────────────────────

EXPERIMENT="e006"
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

# Workers per group
FOCUSED_WORKERS=8
UNFOCUSED_WORKERS=8
NUM_WORKERS=$((FOCUSED_WORKERS + UNFOCUSED_WORKERS))

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
    echo "  E006 Server"
    echo "=========================================="
    echo ""
    echo "  Leaderboard capacity: $LEADERBOARD_CAPACITY"
    echo "  Database:             ramseynet.db"
    echo "  Log:                  $LOGDIR/server.log"
    echo ""
    echo "  Stop with Ctrl+C"
    echo "=========================================="
    echo ""

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
Workers:    $NUM_WORKERS ($FOCUSED_WORKERS focused + $UNFOCUSED_WORKERS unfocused)
Config:     beam=$BEAM_WIDTH depth=$MAX_DEPTH bias=$SAMPLE_BIAS
Init:       $INIT_MODE
Max iters:  $MAX_ITERS
Server:     $SERVER_URL (capacity=$LEADERBOARD_CAPACITY)
Base port:  $BASE_PORT
Snapshot:   every ${SNAPSHOT_MIN}m
Logs:       $FLEET_LOGDIR/

Hypothesis: Focused edge flipping (only mutating edges in violations)
            produces higher admits/hr than unfocused (all 300 edges).
            Based on Exoo & Tatarevic (2015) Algorithm 2.

Groups:
  Workers 0-$((FOCUSED_WORKERS - 1)):   focused=true
  Workers $FOCUSED_WORKERS-$((NUM_WORKERS - 1)): focused=false
EOF

    echo ""
    echo "=========================================="
    echo "  E006: Focused Edge Flipping"
    echo "=========================================="
    echo ""
    echo "  Target:   R($K,$ELL) n=$N"
    echo "  Strategy: $STRATEGY"
    echo "  Workers:  $NUM_WORKERS ($FOCUSED_WORKERS focused + $UNFOCUSED_WORKERS unfocused)"
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
        echo "    ./scripts/experiment-e006.sh server"
        echo "    # Or from production: ~/RamseyNet/scripts/experiment-e005.sh server"
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

        local focused_rounds=0 focused_disc=0 focused_admitted=0 focused_submitted=0
        local unfocused_rounds=0 unfocused_disc=0 unfocused_admitted=0 unfocused_submitted=0

        {
            echo "=========================================="
            echo "  E006 Status — $(date)"
            echo "  Elapsed: ${elapsed_min}m (${elapsed_hr}h)"
            echo "  Commit:  $COMMIT_HASH"
            echo "  Config:  beam=$BEAM_WIDTH depth=$MAX_DEPTH bias=$SAMPLE_BIAS"
            echo "=========================================="
            echo ""
            echo "  ── FOCUSED (workers 0-$((FOCUSED_WORKERS - 1))) ──"
            echo ""

            for i in $(seq 0 $((FOCUSED_WORKERS - 1))); do
                logfile="$FLEET_LOGDIR/worker-${i}.log"
                last=$(grep 'round_summary' "$logfile" 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g' | tail -1 || true)
                if [ -n "$last" ]; then
                    rounds=$(echo "$last" | grep -oP 'round=\K[0-9]+' || echo "0")
                    disc=$(echo "$last" | grep -oP 'total_discoveries=\K[0-9]+' || echo "0")
                    admit=$(echo "$last" | grep -oP 'total_admitted=\K[0-9]+' || echo "0")
                    submit=$(echo "$last" | grep -oP 'total_submitted=\K[0-9]+' || echo "0")
                    focused_rounds=$((focused_rounds + rounds))
                    focused_disc=$((focused_disc + disc))
                    focused_admitted=$((focused_admitted + admit))
                    focused_submitted=$((focused_submitted + submit))
                    printf "  Worker %2d: %6d rounds, %10d disc, %6d admitted  [focused]\n" "$i" "$rounds" "$disc" "$admit"
                else
                    printf "  Worker %2d: (no data)  [focused]\n" "$i"
                fi
            done

            echo ""
            echo "  ── UNFOCUSED (workers $FOCUSED_WORKERS-$((NUM_WORKERS - 1))) ──"
            echo ""

            for i in $(seq $FOCUSED_WORKERS $((NUM_WORKERS - 1))); do
                logfile="$FLEET_LOGDIR/worker-${i}.log"
                last=$(grep 'round_summary' "$logfile" 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g' | tail -1 || true)
                if [ -n "$last" ]; then
                    rounds=$(echo "$last" | grep -oP 'round=\K[0-9]+' || echo "0")
                    disc=$(echo "$last" | grep -oP 'total_discoveries=\K[0-9]+' || echo "0")
                    admit=$(echo "$last" | grep -oP 'total_admitted=\K[0-9]+' || echo "0")
                    submit=$(echo "$last" | grep -oP 'total_submitted=\K[0-9]+' || echo "0")
                    unfocused_rounds=$((unfocused_rounds + rounds))
                    unfocused_disc=$((unfocused_disc + disc))
                    unfocused_admitted=$((unfocused_admitted + admit))
                    unfocused_submitted=$((unfocused_submitted + submit))
                    printf "  Worker %2d: %6d rounds, %10d disc, %6d admitted  [unfocused]\n" "$i" "$rounds" "$disc" "$admit"
                else
                    printf "  Worker %2d: (no data)  [unfocused]\n" "$i"
                fi
            done

            echo ""
            echo "  ────────────────────────────────────"
            echo "  Comparison:"
            echo ""
            printf "  %-20s  %12s  %12s  %8s\n" "Metric" "Focused" "Unfocused" "Ratio"
            printf "  %-20s  %12s  %12s  %8s\n" "────────────────────" "────────────" "────────────" "────────"
            printf "  %-20s  %12d  %12d\n" "Rounds" "$focused_rounds" "$unfocused_rounds"
            printf "  %-20s  %12d  %12d\n" "Discoveries" "$focused_disc" "$unfocused_disc"
            printf "  %-20s  %12d  %12d\n" "Submitted" "$focused_submitted" "$unfocused_submitted"
            printf "  %-20s  %12d  %12d\n" "Admitted" "$focused_admitted" "$unfocused_admitted"

            if [ "$elapsed_sec" -gt 60 ]; then
                focused_admits_hr=$(awk "BEGIN {printf \"%.0f\", $focused_admitted / ($elapsed_sec / 3600.0)}")
                unfocused_admits_hr=$(awk "BEGIN {printf \"%.0f\", $unfocused_admitted / ($elapsed_sec / 3600.0)}")
                printf "  %-20s  %12s  %12s" "Admits/hr" "$focused_admits_hr" "$unfocused_admits_hr"
                if [ "$unfocused_admits_hr" -gt 0 ] 2>/dev/null; then
                    ratio=$(awk "BEGIN {printf \"%.2fx\", $focused_admits_hr / $unfocused_admits_hr}")
                    printf "  %8s" "$ratio"
                fi
                echo ""

                focused_rounds_hr=$(awk "BEGIN {printf \"%.0f\", $focused_rounds / ($elapsed_sec / 3600.0)}")
                unfocused_rounds_hr=$(awk "BEGIN {printf \"%.0f\", $unfocused_rounds / ($elapsed_sec / 3600.0)}")
                printf "  %-20s  %12s  %12s" "Rounds/hr" "$focused_rounds_hr" "$unfocused_rounds_hr"
                if [ "$unfocused_rounds_hr" -gt 0 ] 2>/dev/null; then
                    ratio=$(awk "BEGIN {printf \"%.2fx\", $focused_rounds_hr / $unfocused_rounds_hr}")
                    printf "  %8s" "$ratio"
                fi
                echo ""
            fi

            if [ "$focused_submitted" -gt 0 ]; then
                focused_rate=$(awk "BEGIN {printf \"%.1f%%\", ($focused_admitted / $focused_submitted) * 100}")
                printf "  %-20s  %12s" "Admit rate" "$focused_rate"
            else
                printf "  %-20s  %12s" "Admit rate" "n/a"
            fi
            if [ "$unfocused_submitted" -gt 0 ]; then
                unfocused_rate=$(awk "BEGIN {printf \"%.1f%%\", ($unfocused_admitted / $unfocused_submitted) * 100}")
                printf "  %12s" "$unfocused_rate"
            else
                printf "  %12s" "n/a"
            fi
            echo ""

            echo ""
            echo "  ────────────────────────────────────"
            echo "  Fleet totals:"
            local total_admitted=$((focused_admitted + unfocused_admitted))
            local total_submitted=$((focused_submitted + unfocused_submitted))
            echo "    Elapsed:      ${elapsed_min}m (${elapsed_hr}h)"
            echo "    Total admits:  $total_admitted"
            if [ "$elapsed_sec" -gt 60 ]; then
                total_admits_hr=$(awk "BEGIN {printf \"%.0f\", $total_admitted / ($elapsed_sec / 3600.0)}")
                echo "    Total admits/hr: $total_admits_hr"
            fi
            echo ""
            echo "  Logs: $FLEET_LOGDIR/"
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

    echo "  Focused workers (0-$((FOCUSED_WORKERS - 1))):"
    for i in $(seq 0 $((FOCUSED_WORKERS - 1))); do
        port=$((BASE_PORT + i))
        logfile="$FLEET_LOGDIR/worker-${i}.log"

        RUST_LOG=info "$WORKER_BIN" \
            --strategy "$STRATEGY" --k "$K" --ell "$ELL" --n "$N" \
            --server "$SERVER_URL" --init "$INIT_MODE" --port "$port" \
            --max-iters "$MAX_ITERS" \
            --beam-width "$BEAM_WIDTH" --max-depth "$MAX_DEPTH" --sample-bias "$SAMPLE_BIAS" \
            --focused true \
            --commit-hash "$COMMIT_HASH" --worker-id "$i" \
            > "$logfile" 2>&1 &
        PIDS+=($!)
        printf "    Worker %2d: http://localhost:%d  [focused]\n" "$i" "$port"
    done

    echo ""
    echo "  Unfocused workers ($FOCUSED_WORKERS-$((NUM_WORKERS - 1))):"
    for i in $(seq $FOCUSED_WORKERS $((NUM_WORKERS - 1))); do
        port=$((BASE_PORT + i))
        logfile="$FLEET_LOGDIR/worker-${i}.log"

        RUST_LOG=info "$WORKER_BIN" \
            --strategy "$STRATEGY" --k "$K" --ell "$ELL" --n "$N" \
            --server "$SERVER_URL" --init "$INIT_MODE" --port "$port" \
            --max-iters "$MAX_ITERS" \
            --beam-width "$BEAM_WIDTH" --max-depth "$MAX_DEPTH" --sample-bias "$SAMPLE_BIAS" \
            --focused false \
            --commit-hash "$COMMIT_HASH" --worker-id "$i" \
            > "$logfile" 2>&1 &
        PIDS+=($!)
        printf "    Worker %2d: http://localhost:%d  [unfocused]\n" "$i" "$port"
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
echo "E006: Focused Edge Flipping — Head-to-Head Comparison"
echo ""
echo "  Split fleet: 8 focused + 8 unfocused workers against prod server."
echo "  Tests Exoo-Tatarevic (2015) focused edge flipping vs baseline."
echo ""
echo "Usage:"
echo "  $0 server   — Start a local server (if not using prod)"
echo "  $0 fleet    — Start 16 workers (8 focused + 8 unfocused)"
echo ""
echo "For headless (overnight) runs:"
echo "  nohup $0 fleet > /dev/null 2>&1 &"
echo ""
echo "Check progress:  cat logs/$EXPERIMENT/status.txt"
echo ""
