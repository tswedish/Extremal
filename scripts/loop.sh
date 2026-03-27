#!/usr/bin/env bash
# ── Extremal Experiment Agent Loop ──────────────────────────────────
#
# Runs Claude in a loop to autonomously manage a fleet of workers,
# observe leaderboard dynamics, and optimize graph scores.
#
# Usage:
#   ./scripts/loop.sh                          # Production server, default settings
#   ./scripts/loop.sh --local                  # Local dev server (http://localhost:3001)
#   ./scripts/loop.sh --interval 3m            # Custom observation interval
#   ./scripts/loop.sh --workers 8 --polish 200 # Override fleet params
#   ./scripts/loop.sh --no-fleet               # Observe only (fleet already running)
#   ./scripts/loop.sh --budget 2.00            # Max USD per cycle
#
# Prerequisites:
#   - Local dashboard relay running on :4000 (./run dashboard)
#   - For --local: local server running on :3001 (./run server)
#   - For production: internet access to api.extremal.online
#
# Stop: Ctrl-C (sends SIGTERM to fleet + exits cleanly)

set -euo pipefail
cd "$(dirname "$0")/.."

# ── Defaults ─────────────────────────────────────────────
SERVER="https://api.extremal.online"
RELAY="http://localhost:4000"
DASHBOARD="ws://localhost:4000/ws/worker"
N=25
WORKERS=4
POLISH=100
INTERVAL="5m"
BUDGET="1.00"
MODEL="opus"
LAUNCH_FLEET=true
FLEET_PIDS=""

# ── Parse args ───────────────────────────────────────────
while [[ $# -gt 0 ]]; do
    case "$1" in
        --local)       SERVER="http://localhost:3001"; shift ;;
        --server)      SERVER="$2"; shift 2 ;;
        --n)           N="$2"; shift 2 ;;
        --workers)     WORKERS="$2"; shift 2 ;;
        --polish)      POLISH="$2"; shift 2 ;;
        --interval)    INTERVAL="$2"; shift 2 ;;
        --budget)      BUDGET="$2"; shift 2 ;;
        --model)       MODEL="$2"; shift 2 ;;
        --no-fleet)    LAUNCH_FLEET=false; shift ;;
        *)             echo "Unknown arg: $1"; exit 1 ;;
    esac
done

# Convert interval to seconds
INTERVAL_SECS=$(python3 -c "
s='$INTERVAL'
n=int(s[:-1]); u=s[-1]
print(n*3600 if u=='h' else n*60 if u=='m' else n)
" 2>/dev/null || echo 300)

COMMIT=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
STARTED=$(date -u +%Y-%m-%dT%H:%M:%SZ)
LOG_DIR="logs/agent-loop-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$LOG_DIR"

echo "╔══════════════════════════════════════════════╗"
echo "║  Extremal Experiment Agent Loop              ║"
echo "╠══════════════════════════════════════════════╣"
echo "║  server:   $SERVER"
echo "║  relay:    $RELAY"
echo "║  n=$N  workers=$WORKERS  polish=$POLISH"
echo "║  interval: $INTERVAL  model: $MODEL"
echo "║  commit:   $COMMIT"
echo "║  logs:     $LOG_DIR"
echo "╚══════════════════════════════════════════════╝"
echo ""

# ── Cleanup ──────────────────────────────────────────────
STOPPED=0
cleanup() {
    if [[ "$STOPPED" -eq 1 ]]; then return; fi
    STOPPED=1
    echo ""
    echo "Shutting down..."
    if [[ -n "$FLEET_PIDS" ]]; then
        echo "Stopping fleet..."
        kill $FLEET_PIDS 2>/dev/null || true
        sleep 2
        kill -9 $FLEET_PIDS 2>/dev/null || true
    fi
    echo "Agent loop stopped. Logs: $LOG_DIR"
}
trap cleanup INT TERM EXIT

# ── Build + Fleet ────────────────────────────────────────
if [[ "$LAUNCH_FLEET" == "true" ]]; then
    echo "Building release binary..."
    cargo build --release -p extremal-worker 2>&1 | tail -1

    # Ensure signing key exists and is registered
    KEY_FILE=".config/extremal/key.json"
    if [[ ! -f "$KEY_FILE" ]]; then
        echo "Generating signing key..."
        cargo run -p extremal-cli -- keygen --name "agent-$(hostname)" 2>&1 | tail -3
    fi
    echo "Registering key with server..."
    cargo run -p extremal-cli -- register-key --server "$SERVER" 2>&1 | tail -1 || true

    BIN="target/release/extremal-worker"
    CONFIGS=(
        "wide-a:--beam-width 150 --max-depth 8 --noise-flips 2 --sample-bias 0.4"
        "wide-b:--beam-width 200 --max-depth 10 --noise-flips 1 --sample-bias 0.5"
        "focused:--beam-width 100 --max-depth 12 --focused true --noise-flips 1 --sample-bias 0.5"
        "deep:--beam-width 60 --max-depth 18 --noise-flips 1 --sample-bias 0.3"
        "wide-c:--beam-width 180 --max-depth 9 --noise-flips 2 --sample-bias 0.4"
        "focus-b:--beam-width 120 --max-depth 14 --focused true --noise-flips 2 --sample-bias 0.4"
        "explore:--beam-width 100 --max-depth 10 --noise-flips 3 --sample-bias 0.3"
        "wide-d:--beam-width 160 --max-depth 8 --noise-flips 1 --sample-bias 0.6"
    )

    echo "Launching $WORKERS workers..."
    for i in $(seq 0 $((WORKERS - 1))); do
        IDX=$((i % ${#CONFIGS[@]}))
        IFS=: read -r NAME ARGS <<< "${CONFIGS[$IDX]}"
        if [[ $i -ge ${#CONFIGS[@]} ]]; then
            NAME="${NAME}-$((i / ${#CONFIGS[@]} + 1))"
        fi

        META="{\"worker_id\":\"$NAME\",\"commit\":\"$COMMIT\",\"started\":\"$STARTED\",\"server\":\"$SERVER\"}"

        NO_COLOR=1 RUST_LOG=info $BIN --n "$N" \
            --polish-max-steps "$POLISH" --polish-tabu-tenure 25 --score-bias-threshold 3 \
            --server "$SERVER" --dashboard "$DASHBOARD" \
            --metadata "$META" \
            $ARGS \
            > "$LOG_DIR/$NAME.log" 2>&1 &
        FLEET_PIDS="$FLEET_PIDS $!"
        echo "  $NAME (PID $!)"
    done

    echo "$FLEET_PIDS" > "$LOG_DIR/pids"

    # Save config for status script
    cat > "$LOG_DIR/config.json" <<EOF
{
    "commit": "$COMMIT",
    "started": "$STARTED",
    "n": $N,
    "workers": $WORKERS,
    "polish_max_steps": $POLISH,
    "server": "$SERVER",
    "dashboard": "$DASHBOARD",
    "log_dir": "$LOG_DIR"
}
EOF

    echo "Fleet launched. Waiting 30s for first round..."
    sleep 30
fi

# ── Observation Loop ─────────────────────────────────────
CYCLE=0
while true; do
    CYCLE=$((CYCLE + 1))
    NOW=$(date '+%Y-%m-%d %H:%M')

    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  Cycle #$CYCLE  [$NOW]"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # Gather current status into a temp file for the prompt
    STATUS_FILE=$(mktemp)
    ./scripts/agent-status.sh "$LOG_DIR" > "$STATUS_FILE" 2>&1 || true

    # Build the prompt — the experiment skill is loaded via --append-system-prompt-file
    # so the agent already has the full protocol. We just need to give it the situation.
    PROMPT=$(cat <<PROMPT_EOF
Run one experiment agent observe-decide-act cycle per the experiment skill protocol.

## Current Fleet Status
$(cat "$STATUS_FILE")

## Session Context
- Cycle: #$CYCLE (of ongoing loop, ${INTERVAL} between cycles)
- Server: $SERVER
- Log dir: $LOG_DIR
- Dashboard relay: $RELAY

## Your Task
Run one observe-decide-act cycle per the experiment skill protocol.

Analyze the status above. If workers need adjustment, use direct HTTP API:
  curl -sf -X POST http://localhost:PORT/api/config -H "Content-Type: application/json" -d '{"param": value}'
(The CLI workers command times out — always use direct HTTP.)

Find worker API ports from the status output or: curl -sf $RELAY/api/workers

If you take action or find something notable, append to experiments/agent/journal.md.

Output a concise cycle report:
  ## Cycle #$CYCLE [$NOW]
  **Leaderboard**: [entry count, top score, how many are ours]
  **Fleet**: [total rounds, total admits, best worker rate]
  **Action**: [what you did, or "none"]
  **Next**: [what to watch for next cycle]
PROMPT_EOF
)

    # Run Claude with experiment skill loaded as system context
    # Using opus with max effort for deep analysis each cycle
    echo "$PROMPT" | claude \
        --print \
        --model "$MODEL" \
        --effort max \
        --append-system-prompt-file ".claude/skills/experiment.md" \
        --allowed-tools "Bash(*) Read(*) Edit(*) Write(*) Grep(*) Glob(*)" \
        --max-budget-usd "$BUDGET" \
        --no-session-persistence \
        2>&1 | tee "$LOG_DIR/cycle-$CYCLE.log"

    rm -f "$STATUS_FILE"

    echo ""
    echo "  Next cycle in $INTERVAL..."
    sleep "$INTERVAL_SECS"
done
