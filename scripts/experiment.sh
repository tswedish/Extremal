#!/usr/bin/env bash
# MineGraph experimental fleet — diverse worker configs to maximize discovery
# Usage: ./scripts/experiment.sh [--n N] [--dashboard URL]
set -euo pipefail
cd "$(dirname "$0")/.."

N=${1:-25}
DASHBOARD="${2:-ws://localhost:4000/ws/worker}"
SERVER="http://localhost:3001"
LOG_DIR="logs/experiment-$(date +%Y%m%d-%H%M%S)"

echo "=== MineGraph Experiment ==="
echo "Target:      n=$N, R(5,5)"
echo "Server:      $SERVER"
echo "Dashboard:   $DASHBOARD"
echo "Logs:        $LOG_DIR"
echo "============================"

# Build release
echo "Building worker (release)..."
cargo build -p minegraph-worker --release 2>&1 | tail -1
BIN="target/release/minegraph-worker"

mkdir -p "$LOG_DIR"
PIDS=()
STOPPED=0

cleanup() {
    if [[ "$STOPPED" -eq 1 ]]; then return; fi
    STOPPED=1
    echo ""
    echo "Stopping experiment..."
    for pid in "${PIDS[@]}"; do
        kill "$pid" 2>/dev/null || true
    done
    wait 2>/dev/null || true
    echo "Experiment stopped."
}
trap cleanup INT TERM

COMMIT=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
DASH="--dashboard $DASHBOARD"

launch() {
    local name="$1"; shift
    local log="$LOG_DIR/$name.log"
    echo "  $name -> $log"
    NO_COLOR=1 RUST_LOG=info "$BIN" \
        --server "$SERVER" \
        --n "$N" \
        --metadata "{\"worker_id\":\"$name\",\"commit_hash\":\"$COMMIT\"}" \
        $DASH \
        "$@" \
        > "$log" 2>&1 &
    PIDS+=($!)
}

echo ""
echo "Launching workers..."

# ── Wide beam, shallow depth — explores broadly ──────────
launch "wide-1" --beam-width 200 --max-depth 8  --sample-bias 0.5 --noise-flips 2
launch "wide-2" --beam-width 200 --max-depth 8  --sample-bias 0.9 --noise-flips 0

# ── Narrow beam, deep search — exploits promising paths ──
launch "deep-1" --beam-width 40  --max-depth 20 --sample-bias 0.8 --noise-flips 1
launch "deep-2" --beam-width 40  --max-depth 20 --sample-bias 0.3 --noise-flips 3

# ── Focused mode — only flips guilty edges ───────────────
launch "focus-1" --beam-width 100 --max-depth 12 --focused true --sample-bias 0.7 --noise-flips 0
launch "focus-2" --beam-width 100 --max-depth 12 --focused true --sample-bias 0.4 --noise-flips 2

# ── High perturbation — escapes local optima ─────────────
launch "noisy-1" --beam-width 80  --max-depth 10 --sample-bias 0.6 --noise-flips 5
launch "noisy-2" --beam-width 80  --max-depth 10 --sample-bias 0.2 --noise-flips 8

echo ""
echo "Experiment running (8 workers). Ctrl+C to stop."
echo "Workers: wide(2) + deep(2) + focused(2) + noisy(2)"
echo ""

# Save config
cat > "$LOG_DIR/config.json" <<EOF
{
    "n": $N,
    "workers": [
        {"name": "wide-1",  "beam_width": 200, "max_depth": 8,  "sample_bias": 0.5, "noise_flips": 2, "focused": false},
        {"name": "wide-2",  "beam_width": 200, "max_depth": 8,  "sample_bias": 0.9, "noise_flips": 0, "focused": false},
        {"name": "deep-1",  "beam_width": 40,  "max_depth": 20, "sample_bias": 0.8, "noise_flips": 1, "focused": false},
        {"name": "deep-2",  "beam_width": 40,  "max_depth": 20, "sample_bias": 0.3, "noise_flips": 3, "focused": false},
        {"name": "focus-1", "beam_width": 100, "max_depth": 12, "sample_bias": 0.7, "noise_flips": 0, "focused": true},
        {"name": "focus-2", "beam_width": 100, "max_depth": 12, "sample_bias": 0.4, "noise_flips": 2, "focused": true},
        {"name": "noisy-1", "beam_width": 80,  "max_depth": 10, "sample_bias": 0.6, "noise_flips": 5, "focused": false},
        {"name": "noisy-2", "beam_width": 80,  "max_depth": 10, "sample_bias": 0.2, "noise_flips": 8, "focused": false}
    ],
    "server": "$SERVER",
    "dashboard": "$DASHBOARD",
    "started": "$(date -Iseconds)",
    "commit": "$COMMIT"
}
EOF

printf '%s\n' "${PIDS[@]}" > "$LOG_DIR/pids"
wait "${PIDS[@]}" 2>/dev/null || true
cleanup
