#!/usr/bin/env bash
# MineGraph experimental fleet — tuned worker configs based on empirical results
# Usage: ./scripts/experiment.sh [N] [DASHBOARD_URL]
set -euo pipefail
cd "$(dirname "$0")/.."

N=${1:-25}
DASHBOARD="${2:-}"
API_PORT_BASE="${3:-0}"
SERVER="${MINEGRAPH_SERVER:-https://api.minegraph.net}"
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
DASH=""
if [[ -n "$DASHBOARD" ]]; then
    DASH="--dashboard $DASHBOARD"
fi
WORKER_NUM=0

# Launch a worker with full metadata including all strategy parameters
launch() {
    local name="$1"; shift
    local log="$LOG_DIR/$name.log"
    WORKER_NUM=$((WORKER_NUM + 1))

    # Parse args to build metadata
    local beam_width="" max_depth="" sample_bias="" noise_flips="" focused="false"
    local args=("$@")
    for ((i=0; i<${#args[@]}; i++)); do
        case "${args[$i]}" in
            --beam-width)  beam_width="${args[$((i+1))]}" ;;
            --max-depth)   max_depth="${args[$((i+1))]}" ;;
            --sample-bias) sample_bias="${args[$((i+1))]}" ;;
            --noise-flips) noise_flips="${args[$((i+1))]}" ;;
            --focused)     focused="${args[$((i+1))]}" ;;
        esac
    done

    local meta="{\"worker_id\":\"$name\",\"commit_hash\":\"$COMMIT\",\"strategy\":\"tree2\",\"beam_width\":${beam_width:-100},\"max_depth\":${max_depth:-12},\"sample_bias\":${sample_bias:-0.5},\"noise_flips\":${noise_flips:-0},\"focused\":${focused}}"

    local api_flag=""
    if [[ "$API_PORT_BASE" -gt 0 ]]; then
        api_flag="--api-port $((API_PORT_BASE + WORKER_NUM - 1))"
    fi

    echo "  $name -> $log"
    NO_COLOR=1 RUST_LOG=info "$BIN" \
        --server "$SERVER" \
        --n "$N" \
        --metadata "$meta" \
        $DASH \
        $api_flag \
        "$@" \
        > "$log" 2>&1 &
    PIDS+=($!)
}

echo ""
echo "Launching workers..."

# ── Best performers: wide beam + moderate noise + low bias ──
# wide-1 was the top performer (402 discoveries, 242 admitted)
launch "wide-a" --beam-width 200 --max-depth 8  --sample-bias 0.5 --noise-flips 2
launch "wide-b" --beam-width 200 --max-depth 10 --sample-bias 0.4 --noise-flips 1
launch "wide-c" --beam-width 150 --max-depth 10 --sample-bias 0.6 --noise-flips 2

# ── Focused + noise was second best (261 discoveries, 161 admitted) ──
launch "focus-a" --beam-width 100 --max-depth 12 --focused true --sample-bias 0.4 --noise-flips 2
launch "focus-b" --beam-width 120 --max-depth 14 --focused true --sample-bias 0.5 --noise-flips 1

# ── Deep search with moderate perturbation ──────────────────
launch "deep-a" --beam-width 50  --max-depth 18 --sample-bias 0.3 --noise-flips 2
launch "deep-b" --beam-width 60  --max-depth 16 --sample-bias 0.5 --noise-flips 1

# ── Mild noise explorer — good admission rate ──────────────
launch "explore" --beam-width 100 --max-depth 10 --sample-bias 0.3 --noise-flips 3

echo ""
echo "Experiment running (8 workers). Ctrl+C to stop."
echo "Workers: wide(3) + focused(2) + deep(2) + explore(1)"
echo ""

printf '%s\n' "${PIDS[@]}" > "$LOG_DIR/pids"
wait "${PIDS[@]}" 2>/dev/null || true
cleanup
