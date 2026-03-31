#!/usr/bin/env bash
# ── Extremal Meta-Agent Orchestrator ────────────────────────────────
#
# Decides whether to run strategy research or experiments, then
# dispatches the appropriate agent. Runs in a loop, alternating
# between research and experimentation based on current state.
#
# Usage:
#   ./scripts/orchestrator.sh                    # Production, auto-decide
#   ./scripts/orchestrator.sh --local            # Local dev server
#   ./scripts/orchestrator.sh --research         # Force research mode
#   ./scripts/orchestrator.sh --experiment       # Force experiment mode
#   ./scripts/orchestrator.sh --workers 8        # Pass through to experiment
#
# Architecture:
#   orchestrator.sh
#     ├── decides: research or experiment?
#     ├── research → Claude + strategy-research.md skill → code changes + git commit
#     └── experiment → loop.sh → fleet + Claude observe-decide-act cycles
#
# The orchestrator reads experiments/agent/strategies.json to find
# untested strategies, and experiments/agent/journal.md to gauge
# whether experiments are plateauing (time to research) or active
# (keep experimenting).

set -euo pipefail
cd "$(dirname "$0")/.."

# ── Defaults ─────────────────────────────────────────────
SERVER="https://api.extremal.online"
MODE="auto"          # auto, research, experiment
WORKERS=8
POLISH=100
EXPERIMENT_INTERVAL="10m"
EXPERIMENT_CYCLES=12  # how many experiment cycles before re-evaluating (~2 hours)
RESEARCH_BUDGET="5.00"
EXPERIMENT_BUDGET="1.50"
RESEARCH_MODEL="opus"
EXPERIMENT_MODEL="sonnet"

# ── Parse args ───────────────────────────────────────────
PASSTHROUGH_ARGS=()
while [[ $# -gt 0 ]]; do
    case "$1" in
        --local)       SERVER="http://localhost:3001"; PASSTHROUGH_ARGS+=("$1"); shift ;;
        --research)    MODE="research"; shift ;;
        --experiment)  MODE="experiment"; shift ;;
        --workers)     WORKERS="$2"; PASSTHROUGH_ARGS+=("$1" "$2"); shift 2 ;;
        --polish)      POLISH="$2"; PASSTHROUGH_ARGS+=("$1" "$2"); shift 2 ;;
        --interval)    EXPERIMENT_INTERVAL="$2"; PASSTHROUGH_ARGS+=("$1" "$2"); shift 2 ;;
        --cycles)      EXPERIMENT_CYCLES="$2"; shift 2 ;;
        --model)       RESEARCH_MODEL="$2"; EXPERIMENT_MODEL="$2"; shift 2 ;;
        --experiment-model) EXPERIMENT_MODEL="$2"; shift 2 ;;
        --research-model)   RESEARCH_MODEL="$2"; shift 2 ;;
        *)             PASSTHROUGH_ARGS+=("$1"); shift ;;
    esac
done

CLAUDE_TIMEOUT=1200  # 20 min max per claude invocation

BRANCH=$(git branch --show-current 2>/dev/null || echo "unknown")
COMMIT=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
LOG_DIR="logs/orchestrator-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$LOG_DIR"

echo "╔══════════════════════════════════════════════════╗"
echo "║  Extremal Meta-Agent Orchestrator                ║"
echo "╠══════════════════════════════════════════════════╣"
echo "║  server:   $SERVER"
echo "║  branch:   $BRANCH ($COMMIT)"
echo "║  mode:     $MODE"
echo "║  models:   research=$RESEARCH_MODEL experiment=$EXPERIMENT_MODEL"
echo "║  logs:     $LOG_DIR"
echo "╚══════════════════════════════════════════════════╝"
echo ""

# ── Cleanup ──────────────────────────────────────────────
STOPPED=0
cleanup() {
    if [[ "$STOPPED" -eq 1 ]]; then return; fi
    STOPPED=1
    echo ""
    echo "Orchestrator shutting down..."
    # Kill any experiment fleet
    pkill -f "extremal-worker" 2>/dev/null || true
    sleep 1
    echo "Done. Logs: $LOG_DIR"
}
trap cleanup INT TERM EXIT

# ── Decision function ────────────────────────────────────
decide_mode() {
    # Returns "research" or "experiment" based on current state
    python3 -c "
import json, sys, os

# Load strategy registry
try:
    reg = json.load(open('experiments/agent/strategies.json'))
except:
    print('research')  # No registry = need to set up
    sys.exit(0)

strategies = reg.get('strategies', [])
ideas = reg.get('ideas', [])

# Count untested strategies
untested = [s for s in strategies if s.get('status') == 'untested']

# Count high-priority ideas
high_ideas = [i for i in ideas if i.get('priority') == 'high']

# Check if experiments are plateauing (read journal)
plateau = False
try:
    journal = open('experiments/agent/journal.md').read()
    # Simple heuristic: if last 3 entries are all ADJUST with marginal improvements
    lines = [l for l in journal.split('\n') if 'marginal' in l.lower() or 'plateau' in l.lower()]
    if len(lines) >= 2:
        plateau = True
except:
    pass

# Check for explicit signal file from agent
signal = os.path.exists('experiments/agent/signal-research')

# Decision logic:
# 0. If agent explicitly signaled research AND no untested strategies → research
# 1. If untested strategies exist → experiment (test them first!)
# 2. If experiments are plateauing and there are high-priority ideas → research
# 3. If no ideas left → research (need new ideas)
# 4. Default: experiment (collecting data is usually more valuable)

if untested:
    print('experiment')  # Always test existing strategies before creating new ones
elif signal:
    print('research')
elif plateau and high_ideas:
    print('research')
elif not high_ideas and not untested:
    print('research')  # Need new ideas
else:
    print('experiment')
" 2>/dev/null || echo "experiment"
}

# ── Main loop ────────────────────────────────────────────
ROUND=0
FORCE_RESEARCH=false
while true; do
    ROUND=$((ROUND + 1))
    COMMIT=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
    NOW=$(date '+%Y-%m-%d %H:%M')

    echo ""
    echo "╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍"
    echo "  Orchestrator Round #$ROUND  [$NOW]  commit=$COMMIT"
    echo "╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍╍"

    # Decide mode
    if [[ "$FORCE_RESEARCH" == "true" ]]; then
        CHOSEN="research"
        FORCE_RESEARCH=false
    elif [[ "$MODE" == "auto" ]]; then
        CHOSEN=$(decide_mode)
    else
        CHOSEN="$MODE"
    fi

    echo "  Decision: $CHOSEN"
    echo ""

    if [[ "$CHOSEN" == "research" ]]; then
        # ── Research Phase ───────────────────────────────
        echo "  ▶ Running strategy research..."

        RESEARCH_PROMPT=$(cat <<'RESEARCH_EOF'
Run one strategy research cycle per the strategy-research skill protocol.

Read the current state:
1. experiments/agent/strategies.json — strategy registry with ideas and validated strategies
2. experiments/agent/findings.json — experimental findings
3. experiments/agent/journal.md — recent experiment activity

Then: assess → choose ONE idea → implement → run CI → commit → update registry → report.

Stay on the current git branch. One change only. CI must pass.
RESEARCH_EOF
)

        RESEARCH_OK=false
        for ATTEMPT in 1 2 3; do
            echo "  [$(date '+%H:%M:%S')] Research attempt $ATTEMPT/3 starting..."
            if timeout "$CLAUDE_TIMEOUT" bash -c 'echo "$1" | claude \
                --print \
                --model "$2" \
                --effort high \
                --append-system-prompt-file "skills/strategy-research.md" \
                --allowed-tools "Bash(*) Read(*) Edit(*) Write(*) Grep(*) Glob(*)" \
                --max-budget-usd "$3" \
                --no-session-persistence' \
                _ "$RESEARCH_PROMPT" "$RESEARCH_MODEL" "$RESEARCH_BUDGET" \
                2>&1 | tee "$LOG_DIR/research-$ROUND.log"; then
                RESEARCH_OK=true
                echo "  [$(date '+%H:%M:%S')] Research attempt $ATTEMPT succeeded."
                break
            else
                EXIT=$?
                if [[ $EXIT -eq 124 ]]; then
                    echo "  [$(date '+%H:%M:%S')] Research TIMED OUT after ${CLAUDE_TIMEOUT}s (attempt $ATTEMPT/3)"
                else
                    echo "  [$(date '+%H:%M:%S')] Research failed exit=$EXIT (attempt $ATTEMPT/3), retrying in 60s..."
                fi
                sleep 60
            fi
        done

        if [[ "$RESEARCH_OK" == "false" ]]; then
            echo "  Research cycle failed after 3 attempts, skipping to next round."
            continue
        fi

        echo ""
        echo "  Research cycle complete. New commit: $(git rev-parse --short HEAD)"
        echo "  Rebuilding worker binary..."
        cargo build --release -p extremal-worker 2>&1 | tail -1

    else
        # ── Experiment Phase ─────────────────────────────
        echo "  ▶ Running experiment loop ($EXPERIMENT_CYCLES cycles)..."

        # Build the experiment prompt with cycle limit
        # We run loop.sh but with a cycle limit by using timeout or a wrapper
        # Instead, let's run the loop inline for N cycles

        # First ensure fleet is running
        if ! pgrep -f "extremal-worker" > /dev/null 2>&1; then
            echo "  Launching fleet first..."
            # Launch fleet in background (without the loop part)
            ./scripts/agent-fleet.sh --workers "$WORKERS" --n 25 --polish "$POLISH" \
                --server "$SERVER" &
            FLEET_PID=$!
            sleep 35  # Wait for fleet warmup
        fi

        # Find the most recent log dir
        FLEET_LOG=$(ls -td logs/agent-* 2>/dev/null | head -1)

        PREV_CYCLE_LOG=""
        for CYCLE in $(seq 1 "$EXPERIMENT_CYCLES"); do
            echo ""
            echo "  ── Experiment Cycle $CYCLE/$EXPERIMENT_CYCLES ──"

            STATUS_FILE=$(mktemp)
            ./scripts/agent-status.sh "$FLEET_LOG" > "$STATUS_FILE" 2>&1 || true

            # Build previous cycle context (last cycle's output, truncated)
            PREV_CYCLE_SECTION=""
            if [[ -n "$PREV_CYCLE_LOG" && -f "$PREV_CYCLE_LOG" ]]; then
                PREV_OUTPUT=$(tail -80 "$PREV_CYCLE_LOG" 2>/dev/null || true)
                if [[ -n "$PREV_OUTPUT" ]]; then
                    PREV_CYCLE_SECTION="
## Previous Cycle Output
This is what you (a previous instance) reported last cycle. Use it for continuity.
\`\`\`
$PREV_OUTPUT
\`\`\`"
                fi
            fi

            # Include recent findings and journal for cross-run continuity
            FINDINGS_SECTION=""
            if [[ -f "experiments/agent/findings.json" ]]; then
                FINDINGS_SECTION="
## Recent Findings (from previous runs — READ for continuity)
$(cat experiments/agent/findings.json | python3 -c "
import json, sys
findings = json.load(sys.stdin)
# Show last 8 findings (most recent at bottom)
for f in findings[-8:]:
    print(f'- [{f.get(\"date\",\"?\")}] {f[\"finding\"]}')
" 2>/dev/null || echo '(could not parse findings.json)')"
            fi
            JOURNAL_SECTION=""
            if [[ -f "experiments/agent/journal.md" ]]; then
                JOURNAL_TAIL=$(tail -30 experiments/agent/journal.md 2>/dev/null || true)
                if [[ -n "$JOURNAL_TAIL" ]]; then
                    JOURNAL_SECTION="
## Recent Journal (last 30 lines from previous runs)
\`\`\`
$JOURNAL_TAIL
\`\`\`"
                fi
            fi

            # Collect inbox messages
            INBOX_DIR="experiments/agent/inbox"
            INBOX_CONTENT=""
            INBOX_FILES=()
            for f in "$INBOX_DIR"/*.md; do
                [[ -f "$f" ]] || continue
                [[ "$(basename "$f")" == "README.md" ]] && continue
                INBOX_FILES+=("$f")
                INBOX_CONTENT="$INBOX_CONTENT
--- $(basename "$f") ---
$(cat "$f")
"
            done
            INBOX_SECTION=""
            if [[ -n "$INBOX_CONTENT" ]]; then
                INBOX_SECTION="
## Operator Messages (READ THESE FIRST)
The operator left these messages for you. Address them in your response and actions.
$INBOX_CONTENT"
            fi

            EXPERIMENT_PROMPT=$(cat <<EXPERIMENT_EOF
Run one experiment agent observe-decide-act cycle per the experiment skill protocol.
$INBOX_SECTION
$FINDINGS_SECTION
$JOURNAL_SECTION
$PREV_CYCLE_SECTION

## Current Fleet Status
$(cat "$STATUS_FILE")

## Session Context
- Orchestrator round: #$ROUND, experiment cycle: $CYCLE/$EXPERIMENT_CYCLES
- Server: $SERVER
- Log dir: $FLEET_LOG

Analyze the status. Adjust workers via direct HTTP API if needed.
Find worker ports: curl -sf http://localhost:4000/api/workers
Config changes: curl -sf -X POST http://localhost:PORT/api/config -H "Content-Type: application/json" -d '{"param": value}'

Log actions to experiments/agent/journal.md.

## Output Format

Print your cycle report to stdout. This is what the operator sees in their terminal, so make
it engaging and insightful. Structure it EXACTLY like this:

---
## Cycle $CYCLE/$EXPERIMENT_CYCLES [$(date '+%H:%M')]

**Leaderboard**: [entry count, top score breakdown, trend direction]
**Fleet**: [total rounds, discoveries, admissions, best worker + why]
**Threshold**: [current admission bar, how far our best candidates are from it]

### What I'm Seeing
[2-3 sentences of genuine analysis. What patterns are emerging? What's surprising?
Are certain configs consistently outperforming? Is the search space exhausted or
is there signal that better graphs exist? What does the score distribution look like?]

### Strategy Thinking
[1-2 sentences on current theory. Why are we running these configs? What hypothesis
are we testing? What would change our approach?]

### Actions Taken
[Bullet list of changes made this cycle, or "None — observing" with reasoning]

### Next Cycle
[What to watch for. What would trigger a strategy change?]
---
EXPERIMENT_EOF
)

            # Move inbox files to processed
            if [[ ${#INBOX_FILES[@]} -gt 0 ]]; then
                mkdir -p "$INBOX_DIR/processed"
                for f in "${INBOX_FILES[@]}"; do
                    mv "$f" "$INBOX_DIR/processed/$(date +%Y%m%d-%H%M%S)-$(basename "$f")"
                done
                echo "  Processed ${#INBOX_FILES[@]} inbox message(s)"
            fi

            CYCLE_OK=false
            for ATTEMPT in 1 2 3; do
                echo "  [$(date '+%H:%M:%S')] Cycle $CYCLE attempt $ATTEMPT/3 starting..."
                if timeout "$CLAUDE_TIMEOUT" bash -c 'echo "$1" | claude \
                    --print \
                    --model "$2" \
                    --effort high \
                    --append-system-prompt-file "skills/experiment.md" \
                    --allowed-tools "Bash(*) Read(*) Edit(*) Write(*) Grep(*) Glob(*)" \
                    --max-budget-usd "$3" \
                    --no-session-persistence' \
                    _ "$EXPERIMENT_PROMPT" "$EXPERIMENT_MODEL" "$EXPERIMENT_BUDGET" \
                    2>&1 | tee "$LOG_DIR/experiment-${ROUND}-${CYCLE}.log"; then
                    CYCLE_OK=true
                    echo "  [$(date '+%H:%M:%S')] Cycle $CYCLE attempt $ATTEMPT succeeded."
                    break
                else
                    EXIT=$?
                    if [[ $EXIT -eq 124 ]]; then
                        echo "  [$(date '+%H:%M:%S')] Cycle $CYCLE TIMED OUT after ${CLAUDE_TIMEOUT}s (attempt $ATTEMPT/3)"
                    else
                        echo "  [$(date '+%H:%M:%S')] Cycle $CYCLE failed exit=$EXIT (attempt $ATTEMPT/3), retrying in 60s..."
                    fi
                    sleep 60
                fi
            done

            if [[ "$CYCLE_OK" == "false" ]]; then
                echo "  [$(date '+%H:%M:%S')] Cycle $CYCLE failed after 3 attempts, skipping."
            fi

            # Track this cycle's log for next cycle's context
            PREV_CYCLE_LOG="$LOG_DIR/experiment-${ROUND}-${CYCLE}.log"

            rm -f "$STATUS_FILE"

            # Check if agent signaled a switch to research mode
            if [[ -f "experiments/agent/signal-research" ]]; then
                echo "  Agent requested strategy-research phase!"
                cat "experiments/agent/signal-research"
                rm -f "experiments/agent/signal-research"
                FORCE_RESEARCH=true
                break
            fi

            if [[ $CYCLE -lt $EXPERIMENT_CYCLES ]]; then
                echo "  Next experiment cycle in $EXPERIMENT_INTERVAL..."
                INTERVAL_SECS=$(python3 -c "
s='$EXPERIMENT_INTERVAL'
n=int(s[:-1]); u=s[-1]
print(n*3600 if u=='h' else n*60 if u=='m' else n)
" 2>/dev/null || echo 300)
                sleep "$INTERVAL_SECS"
            fi
        done

        echo ""
        echo "  Experiment phase complete ($EXPERIMENT_CYCLES cycles)."
        echo "  Stopping fleet for potential research..."
        pkill -f "extremal-worker" 2>/dev/null || true
        sleep 2
    fi

    echo ""
    echo "  Round #$ROUND complete. Deciding next action..."
    sleep 5
done
