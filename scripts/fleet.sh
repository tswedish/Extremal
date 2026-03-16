#!/usr/bin/env bash
# Launch a fleet of workers against a server.
#
# Usage:
#   ./scripts/fleet.sh [OPTIONS]
#
# Options:
#   --workers N        Number of workers (default: 16)
#   --strategy STR     Strategy for all workers (default: tree2)
#   --k K              Ramsey parameter k (default: 5)
#   --ell L            Ramsey parameter ell (default: 5)
#   --n N              Target vertex count (default: 25)
#   --server URL       Server URL (default: http://localhost:3001)
#   --init MODE        Init mode (default: leaderboard)
#   --base-port PORT   First dashboard port (default: 8080)
#   --max-iters N      Max iterations per round (default: 100000)
#   --sweep            Distribute workers across hyperparameter profiles
#   --beam-width N     Beam width (default: 100, ignored with --sweep)
#   --max-depth N      Max depth (default: 10, ignored with --sweep)
#   --sample-bias F    Sample bias (default: 0.5, ignored with --sweep)
#   --snapshot-min N   Write progress snapshot every N minutes (default: 10)
#
# Overnight usage:
#   nohup ./run fleet --sweep --base-port 9000 > /dev/null 2>&1 &
#
#   # Check progress without stopping:
#   cat logs/fleet-*/status.txt
#
#   # Full analysis after stopping:
#   ./scripts/analyze_experiment.sh logs/fleet-*/

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"

source "$HOME/.cargo/env" 2>/dev/null || true

# Defaults
NUM_WORKERS=16
STRATEGY="tree2"
K=5
ELL=5
N=25
SERVER_URL="http://localhost:3001"
INIT_MODE="leaderboard"
BASE_PORT=8080
MAX_ITERS=100000
SWEEP=false
BEAM_WIDTH=100
MAX_DEPTH=10
SAMPLE_BIAS=0.5
SNAPSHOT_MIN=10

# Parse args
while [[ $# -gt 0 ]]; do
  case $1 in
    --workers) NUM_WORKERS="$2"; shift 2 ;;
    --strategy) STRATEGY="$2"; shift 2 ;;
    --k) K="$2"; shift 2 ;;
    --ell) ELL="$2"; shift 2 ;;
    --n) N="$2"; shift 2 ;;
    --server) SERVER_URL="$2"; shift 2 ;;
    --init) INIT_MODE="$2"; shift 2 ;;
    --base-port) BASE_PORT="$2"; shift 2 ;;
    --max-iters) MAX_ITERS="$2"; shift 2 ;;
    --sweep) SWEEP=true; shift ;;
    --beam-width) BEAM_WIDTH="$2"; shift 2 ;;
    --max-depth) MAX_DEPTH="$2"; shift 2 ;;
    --sample-bias) SAMPLE_BIAS="$2"; shift 2 ;;
    --snapshot-min) SNAPSHOT_MIN="$2"; shift 2 ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

TIMESTAMP=$(date +%Y%m%d-%H%M%S)
START_EPOCH=$(date +%s)
LOGDIR="$REPO/logs/fleet-$TIMESTAMP"
mkdir -p "$LOGDIR"

# Profiles tuned from sweep experiments.
# focused (bias=0.8) consistently dominates. ultra-wide is dead.
PROFILES=(
  "focused:80:12:0.8"
  "focused-deep:60:18:0.8"
  "focused-wide:120:8:0.8"
  "standard:100:10:0.5"
  "narrow-deep:50:20:0.5"
  "explorer:120:8:0.2"
)
NUM_PROFILES=${#PROFILES[@]}

# Assign profiles to workers
declare -a WORKER_LABELS
declare -a WORKER_BEAMS
declare -a WORKER_DEPTHS
declare -a WORKER_BIASES

for i in $(seq 1 $NUM_WORKERS); do
  if $SWEEP; then
    pidx=$(( (i - 1) % NUM_PROFILES ))
    IFS=: read -r label bw md sb <<< "${PROFILES[$pidx]}"
    WORKER_LABELS[$i]="$label"
    WORKER_BEAMS[$i]="$bw"
    WORKER_DEPTHS[$i]="$md"
    WORKER_BIASES[$i]="$sb"
  else
    WORKER_LABELS[$i]="default"
    WORKER_BEAMS[$i]="$BEAM_WIDTH"
    WORKER_DEPTHS[$i]="$MAX_DEPTH"
    WORKER_BIASES[$i]="$SAMPLE_BIAS"
  fi
done

# Write metadata
MODE_DESC="uniform (beam=$BEAM_WIDTH depth=$MAX_DEPTH bias=$SAMPLE_BIAS)"
if $SWEEP; then
  MODE_DESC="sweep (${NUM_PROFILES} profiles)"
fi

META="$LOGDIR/fleet.txt"
cat > "$META" <<EOF
Fleet: $NUM_WORKERS x $STRATEGY ($MODE_DESC)
Started:    $(date)
Target:     R($K,$ELL) n=$N
Init:       $INIT_MODE
Server:     $SERVER_URL
Max iters:  $MAX_ITERS
Base port:  $BASE_PORT
Snapshot:   every ${SNAPSHOT_MIN}m
Logs:       $LOGDIR/
EOF

if $SWEEP; then
  echo "" >> "$META"
  echo "Profiles:" >> "$META"
  for i in $(seq 1 $NUM_WORKERS); do
    echo "  Worker $i: ${WORKER_LABELS[$i]} (beam=${WORKER_BEAMS[$i]} depth=${WORKER_DEPTHS[$i]} bias=${WORKER_BIASES[$i]})" >> "$META"
  done
fi

echo ""
echo "=========================================="
echo "  MineGraph Fleet: $NUM_WORKERS x $STRATEGY"
if $SWEEP; then
  echo "  Mode: hyperparameter sweep ($NUM_PROFILES profiles)"
else
  echo "  Mode: uniform (beam=$BEAM_WIDTH depth=$MAX_DEPTH bias=$SAMPLE_BIAS)"
fi
echo "=========================================="
echo ""
echo "  Target:     R($K,$ELL) n=$N"
echo "  Server:     $SERVER_URL"
echo "  Init:       $INIT_MODE"
echo "  Snapshots:  every ${SNAPSHOT_MIN}m → $LOGDIR/status.txt"
echo "  Logs:       $LOGDIR/"
echo ""

if $SWEEP; then
  echo "  Profiles:"
  for p in "${PROFILES[@]}"; do
    IFS=: read -r label bw md sb <<< "$p"
    printf "    %-14s beam=%-4s depth=%-3s bias=%s\n" "$label" "$bw" "$md" "$sb"
  done
  echo ""
fi

# ── Summary function (used by cleanup and periodic snapshots) ──

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

  declare -A profile_rounds profile_disc profile_admit profile_submit profile_count

  {
    echo "=========================================="
    echo "  Fleet Summary — $(date)"
    echo "  Elapsed: ${elapsed_min}m (${elapsed_hr}h)"
    echo "=========================================="
    echo ""

    for i in $(seq 1 $NUM_WORKERS); do
      label="${WORKER_LABELS[$i]}"
      logfile="$LOGDIR/${STRATEGY}-${i}-${label}.log"
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

        printf "  Worker %2d [%-14s]: %6d rounds, %10d disc, %6d admitted\n" "$i" "$label" "$rounds" "$disc" "$admit"

        profile_rounds[$label]=$(( ${profile_rounds[$label]:-0} + rounds ))
        profile_disc[$label]=$(( ${profile_disc[$label]:-0} + disc ))
        profile_admit[$label]=$(( ${profile_admit[$label]:-0} + admit ))
        profile_submit[$label]=$(( ${profile_submit[$label]:-0} + submit ))
        profile_count[$label]=$(( ${profile_count[$label]:-0} + 1 ))
      else
        printf "  Worker %2d [%-14s]: (no data)\n" "$i" "$label"
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
    if [ "$elapsed_min" -gt 0 ]; then
      admits_per_hr=$(awk "BEGIN {printf \"%.0f\", $total_admitted / ($elapsed_sec / 3600.0)}")
      disc_per_hr=$(awk "BEGIN {printf \"%.0f\", $total_discoveries / ($elapsed_sec / 3600.0)}")
      rounds_per_hr=$(awk "BEGIN {printf \"%.0f\", $total_rounds / ($elapsed_sec / 3600.0)}")
      echo "    Admits/hr:    $admits_per_hr"
      echo "    Disc/hr:      $disc_per_hr"
      echo "    Rounds/hr:    $rounds_per_hr"
    fi

    if $SWEEP; then
      echo ""
      echo "  ────────────────────────────────────"
      echo "  Per-profile results (sorted by Admit/Wk):"
      echo ""
      printf "  %-14s  %5s  %8s  %7s  %8s  %8s\n" "Profile" "Wkrs" "Rounds" "Admits" "Admit/Wk" "Disc/Rnd"
      printf "  %-14s  %5s  %8s  %7s  %8s  %8s\n" "──────────────" "─────" "────────" "───────" "────────" "────────"

      # Build sortable lines then sort by Admit/Wk descending
      local tmplines=""
      for p in "${PROFILES[@]}"; do
        IFS=: read -r label bw md sb <<< "$p"
        wkrs=${profile_count[$label]:-0}
        if [ "$wkrs" -gt 0 ]; then
          pr=${profile_rounds[$label]:-0}
          pa=${profile_admit[$label]:-0}
          pd=${profile_disc[$label]:-0}
          per_wk=$((pa / wkrs))
          disc_per_rnd=0
          if [ "$pr" -gt 0 ]; then
            disc_per_rnd=$((pd / pr))
          fi
          printf "  %-14s  %5d  %8d  %7d  %8d  %8d\n" "$label" "$wkrs" "$pr" "$pa" "$per_wk" "$disc_per_rnd"
        fi
      done
    fi

    echo ""
    echo "  Logs: $LOGDIR/"
    echo ""
    echo "  To analyze in detail:"
    echo "    ./scripts/analyze_experiment.sh $LOGDIR/"
    echo ""
    echo "=========================================="
  } > "$dest"
}

# Build
echo "--- Building release binaries ---"
cargo build --release -p ramseynet-worker --quiet 2>&1

# Health check
if curl -sf "$SERVER_URL/api/health" > /dev/null 2>&1; then
  echo "--- Server healthy at $SERVER_URL ---"
else
  echo "--- WARNING: Server at $SERVER_URL not responding ---"
  echo "    Start it first: ./run server --release"
  echo ""
fi

# Track PIDs
PIDS=()
SNAPSHOT_PID=""

cleanup() {
  # Kill snapshot loop first
  if [ -n "$SNAPSHOT_PID" ]; then
    kill "$SNAPSHOT_PID" 2>/dev/null || true
  fi

  echo ""
  echo "--- Stopping $NUM_WORKERS workers ---"
  for pid in "${PIDS[@]}"; do
    kill "$pid" 2>/dev/null || true
  done
  wait 2>/dev/null || true

  # Write final summary to file and stdout
  write_summary "$LOGDIR/results.txt"
  cat "$LOGDIR/results.txt"
  cp "$LOGDIR/results.txt" "$LOGDIR/status.txt"
}
trap cleanup EXIT INT TERM

# Launch workers
echo "--- Launching $NUM_WORKERS workers ---"
echo ""

# Resolve commit hash for provenance tracking
COMMIT_HASH=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")

for i in $(seq 1 $NUM_WORKERS); do
  port=$((BASE_PORT + i - 1))
  label="${WORKER_LABELS[$i]}"
  bw="${WORKER_BEAMS[$i]}"
  md="${WORKER_DEPTHS[$i]}"
  sb="${WORKER_BIASES[$i]}"
  logfile="$LOGDIR/${STRATEGY}-${i}-${label}.log"
  wid=$((i - 1))

  RUST_LOG=info cargo run --release -p ramseynet-worker -- \
    --strategy "$STRATEGY" --k "$K" --ell "$ELL" --n "$N" \
    --server "$SERVER_URL" --init "$INIT_MODE" --port "$port" \
    --max-iters "$MAX_ITERS" \
    --beam-width "$bw" --max-depth "$md" --sample-bias "$sb" \
    --commit-hash "$COMMIT_HASH" --worker-id "$wid" \
    > "$logfile" 2>&1 &
  PIDS+=($!)
done

echo "  Dashboards:"
echo ""
for i in $(seq 1 $NUM_WORKERS); do
  port=$((BASE_PORT + i - 1))
  label="${WORKER_LABELS[$i]}"
  printf "    Worker %2d [%-14s]: http://localhost:%d\n" "$i" "$label" "$port"
done

echo ""
echo "  Check progress without stopping:"
echo "    cat $LOGDIR/status.txt"
echo ""
echo "  Open all dashboards:"
echo "    for p in $(seq $BASE_PORT $((BASE_PORT + NUM_WORKERS - 1)) | tr '\n' ' '); do xdg-open http://localhost:\$p; done"
echo ""
echo "=========================================="
echo "  Fleet running. Press Ctrl+C to stop."
echo "  Snapshots every ${SNAPSHOT_MIN}m → status.txt"
echo "=========================================="
echo ""

# Periodic snapshot loop (background)
(
  while true; do
    sleep $((SNAPSHOT_MIN * 60))
    write_summary "$LOGDIR/status.txt" 2>/dev/null || true
  done
) &
SNAPSHOT_PID=$!

# Wait for workers
wait "${PIDS[@]}" 2>/dev/null || true
