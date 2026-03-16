#!/usr/bin/env bash
# Analyze a completed experiment or fleet run.
#
# Usage: ./scripts/analyze_experiment.sh <log_dir>
#
# Handles both experiment (two logs) and fleet (many logs with profile labels).
# Reads round_summary lines and computes per-worker and per-profile stats.

set -euo pipefail

LOGDIR="${1:?Usage: analyze_experiment.sh <log_dir>}"

if [ ! -d "$LOGDIR" ]; then
  echo "Error: $LOGDIR is not a directory"
  exit 1
fi

# If results.txt exists (fleet run), just show it
if [ -f "$LOGDIR/results.txt" ]; then
  echo ""
  echo "(Showing saved results from $LOGDIR/results.txt)"
  echo ""
  cat "$LOGDIR/results.txt"
  echo ""
fi

echo ""
echo "=========================================="
echo "  Detailed Per-Worker Analysis"
echo "=========================================="

if [ -f "$LOGDIR/fleet.txt" ]; then
  echo ""
  cat "$LOGDIR/fleet.txt"
fi
if [ -f "$LOGDIR/experiment.txt" ]; then
  echo ""
  cat "$LOGDIR/experiment.txt"
fi

# Per-profile accumulators
declare -A prof_rounds prof_disc prof_admit prof_submit prof_count prof_total_ms

for logfile in "$LOGDIR"/*.log; do
  name=$(basename "$logfile" .log)
  [ "$name" = "server" ] && continue

  # Extract profile label from filename (e.g., tree2-5-focused → focused)
  label=$(echo "$name" | sed 's/^[^-]*-[0-9]*-//' | sed 's/^[^-]*-[0-9]*$/default/')

  echo ""
  echo "────────────────────────────────────────"
  echo "  Worker: $name"
  echo "────────────────────────────────────────"

  grep 'round_summary' "$logfile" 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g' | awk -v label="$label" '
  BEGIN { n=0; total_ms=0; min_ms=999999999; max_ms=0 }
  {
    n++
    for(i=1;i<=NF;i++) {
      if ($i ~ /^elapsed_ms=/) { split($i, a, "="); ms=a[2]+0; total_ms+=ms; if(ms<min_ms)min_ms=ms; if(ms>max_ms)max_ms=ms }
      if ($i ~ /^total_discoveries=/) { split($i, a, "="); td=a[2]+0 }
      if ($i ~ /^total_admitted=/) { split($i, a, "="); ta=a[2]+0 }
      if ($i ~ /^total_submitted=/) { split($i, a, "="); ts=a[2]+0 }
      if ($i ~ /^discoveries=/) { split($i, a, "="); disc=a[2]+0 }
    }
    admits[n] = ta
  }
  END {
    if (n == 0) { print "  No round summaries found."; exit }
    avg_ms = int(total_ms / n)
    rate = (ts > 0) ? sprintf("%.1f%%", (ta/ts)*100) : "n/a"
    wall_hrs = total_ms / 3600000.0
    admits_per_hr = (wall_hrs > 0.001) ? sprintf("%.0f", ta / wall_hrs) : "n/a"
    disc_per_hr = (wall_hrs > 0.001) ? sprintf("%.0f", td / wall_hrs) : "n/a"

    printf "\n"
    printf "  Rounds:             %d\n", n
    printf "  Total discoveries:  %d\n", td
    printf "  Total submitted:    %d\n", ts
    printf "  Total admitted:     %d\n", ta
    printf "  Admission rate:     %s\n", rate
    printf "\n"
    printf "  Round time (ms):    avg=%d  min=%d  max=%d\n", avg_ms, min_ms, max_ms
    printf "  Wall time:          %.1f hours\n", wall_hrs
    printf "  Admits/hr:          %s\n", admits_per_hr
    printf "  Discoveries/hr:     %s\n", disc_per_hr

    # Admission trend: 5 time slices
    if (n >= 10) {
      slice = int(n / 5)
      printf "\n  Admission timeline (5 slices):\n"
      for (s = 1; s <= 5; s++) {
        idx = s * slice
        if (idx > n) idx = n
        prev_idx = (s-1) * slice
        if (prev_idx < 1) prev_idx = 1
        delta = admits[idx] - admits[prev_idx]
        printf "    Slice %d (rounds %5d-%5d): +%d admissions\n", s, prev_idx, idx, delta
      }
      # Detect plateau: last slice has 0 admissions
      last_delta = admits[n] - admits[n - slice]
      if (last_delta == 0) printf "    >> PLATEAU detected in last slice\n"
    }
    printf "\n"
  }
  '

  # Accumulate per-profile
  last=$(grep 'round_summary' "$logfile" 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g' | tail -1 || true)
  if [ -n "$last" ]; then
    rounds=$(echo "$last" | grep -oP 'round=\K[0-9]+' || echo "0")
    disc=$(echo "$last" | grep -oP 'total_discoveries=\K[0-9]+' || echo "0")
    admit=$(echo "$last" | grep -oP 'total_admitted=\K[0-9]+' || echo "0")
    submit=$(echo "$last" | grep -oP 'total_submitted=\K[0-9]+' || echo "0")
    total_ms=$(grep 'round_summary' "$logfile" 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g' | awk '{for(i=1;i<=NF;i++){if($i~/^elapsed_ms=/){split($i,a,"=");s+=a[2]+0}}}END{print s}')

    prof_rounds[$label]=$(( ${prof_rounds[$label]:-0} + rounds ))
    prof_disc[$label]=$(( ${prof_disc[$label]:-0} + disc ))
    prof_admit[$label]=$(( ${prof_admit[$label]:-0} + admit ))
    prof_submit[$label]=$(( ${prof_submit[$label]:-0} + submit ))
    prof_count[$label]=$(( ${prof_count[$label]:-0} + 1 ))
    prof_total_ms[$label]=$(awk "BEGIN {print ${prof_total_ms[$label]:-0} + $total_ms}")
  fi
done

# Per-profile summary if there are multiple profiles
unique_profiles=$(echo "${!prof_count[@]}" | tr ' ' '\n' | sort -u | wc -l)
if [ "$unique_profiles" -gt 1 ]; then
  echo ""
  echo "=========================================="
  echo "  Per-Profile Summary"
  echo "=========================================="
  echo ""
  printf "  %-14s  %5s  %8s  %7s  %8s  %8s  %8s\n" "Profile" "Wkrs" "Rounds" "Admits" "Admit/Wk" "Disc/Rnd" "Admit/hr"
  printf "  %-14s  %5s  %8s  %7s  %8s  %8s  %8s\n" "──────────────" "─────" "────────" "───────" "────────" "────────" "────────"
  for label in $(for k in "${!prof_admit[@]}"; do
    wkrs=${prof_count[$k]:-1}
    echo "$k $((${prof_admit[$k]} / wkrs))"
  done | sort -k2 -rn | awk '{print $1}'); do
    wkrs=${prof_count[$label]:-0}
    if [ "$wkrs" -gt 0 ]; then
      pr=${prof_rounds[$label]:-0}
      pa=${prof_admit[$label]:-0}
      pd=${prof_disc[$label]:-0}
      per_wk=$((pa / wkrs))
      disc_per_rnd=0
      if [ "$pr" -gt 0 ]; then
        disc_per_rnd=$((pd / pr))
      fi
      tms=${prof_total_ms[$label]:-0}
      wall_hrs=$(awk "BEGIN {printf \"%.3f\", $tms / 3600000.0}")
      admit_hr="n/a"
      if [ "$(awk "BEGIN {print ($wall_hrs > 0.001)}")" = "1" ]; then
        admit_hr=$(awk "BEGIN {printf \"%.0f\", $pa / $wall_hrs}")
      fi
      printf "  %-14s  %5d  %8d  %7d  %8d  %8d  %8s\n" "$label" "$wkrs" "$pr" "$pa" "$per_wk" "$disc_per_rnd" "$admit_hr"
    fi
  done
  echo ""
fi

echo "=========================================="
echo "  Paste this output to Claude for analysis"
echo "=========================================="
echo ""
