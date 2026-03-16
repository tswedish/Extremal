#!/usr/bin/env bash
# Analyze a completed experiment's logs.
#
# Usage: ./scripts/analyze_experiment.sh <log_dir>
# Example: ./scripts/analyze_experiment.sh logs/experiment-20260315-213310

set -euo pipefail

LOGDIR="${1:?Usage: analyze_experiment.sh <log_dir>}"

if [ ! -d "$LOGDIR" ]; then
  echo "Error: $LOGDIR is not a directory"
  exit 1
fi

echo ""
echo "=========================================="
echo "  Experiment Analysis"
echo "=========================================="

if [ -f "$LOGDIR/experiment.txt" ]; then
  echo ""
  cat "$LOGDIR/experiment.txt"
fi

for logfile in "$LOGDIR"/*.log; do
  name=$(basename "$logfile" .log)
  [ "$name" = "server" ] && continue

  echo ""
  echo "────────────────────────────────────────"
  echo "  Strategy: $name"
  echo "────────────────────────────────────────"

  # Use awk to extract everything in one pass
  grep 'round_summary' "$logfile" 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g' | awk '
  BEGIN { n=0; total_ms=0; min_ms=999999999; max_ms=0 }
  {
    n++
    # Extract fields
    for(i=1;i<=NF;i++) {
      if ($i ~ /^elapsed_ms=/) { split($i, a, "="); ms=a[2]+0; total_ms+=ms; if(ms<min_ms)min_ms=ms; if(ms>max_ms)max_ms=ms }
      if ($i ~ /^total_discoveries=/) { split($i, a, "="); td=a[2]+0 }
      if ($i ~ /^total_admitted=/) { split($i, a, "="); ta=a[2]+0 }
      if ($i ~ /^total_submitted=/) { split($i, a, "="); ts=a[2]+0 }
      if ($i ~ /^discoveries=/) { split($i, a, "="); disc=a[2]+0 }
    }
    # Track early/late admits
    tenth = int(n/10)
    if (n == int(total_rounds_placeholder) ) {} # placeholder
    admits[n] = ta
    if (n == 1) first_ta = ta
  }
  END {
    if (n == 0) { print "  No round summaries found."; exit }
    avg_ms = int(total_ms / n)
    rate = (ts > 0) ? sprintf("%.1f%%", (ta/ts)*100) : "n/a"
    printf "\n"
    printf "  Rounds:             %d\n", n
    printf "  Total discoveries:  %d\n", td
    printf "  Total submitted:    %d\n", ts
    printf "  Total admitted:     %d\n", ta
    printf "  Admission rate:     %s\n", rate
    printf "\n"
    printf "  Round time (ms):    avg=%d  min=%d  max=%d\n", avg_ms, min_ms, max_ms
    # Admission trend
    tenth = int(n / 10)
    if (tenth > 1) {
      early = admits[tenth]
      late_start = admits[n - tenth]
      late_delta = ta - late_start
      printf "\n"
      printf "  Admission trend:\n"
      printf "    First 10%% (%d rounds): %d admissions\n", tenth, early
      printf "    Last 10%% (%d rounds):  %d new admissions\n", tenth, late_delta
      if (late_delta == 0) printf "    >> PLATEAU detected\n"
    }
    printf "\n"
  }
  '

done

echo "=========================================="
