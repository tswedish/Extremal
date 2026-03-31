#!/usr/bin/env bash
# Trace leaderboard entries back to commits, configs, and campaigns.
# Usage: ./scripts/agent-provenance.sh [N] [LIMIT]
#
# Shows what code and config actually produced the top leaderboard entries.
# Essential for understanding what works vs what's untested complexity.

set -euo pipefail

N="${1:-25}"
LIMIT="${2:-20}"
SERVER="${SERVER:-https://api.extremal.online}"

echo "=== Leaderboard Provenance: n=$N (top $LIMIT) ==="
echo ""

curl -sf --max-time 10 "$SERVER/api/leaderboards/$N?limit=500" 2>/dev/null | python3 -c "
import json, sys, subprocess
from collections import Counter
from datetime import datetime, timezone

data = json.load(sys.stdin)
entries = data['entries']
total = data['total']

# Sort by admission date for recent activity
by_date = sorted(entries, key=lambda e: e['admitted_at'], reverse=True)

# Fetch metadata for top entries and recent entries
sample = entries[:$LIMIT]
commits = {}
workers = {}
campaigns = {}

for e in sample:
    cid = e['cid']
    try:
        resp = subprocess.check_output(
            ['curl', '-sf', '--max-time', '5', f'$SERVER/api/submissions/{cid}'],
            stderr=subprocess.DEVNULL)
        d = json.loads(resp)
        meta = d.get('submission', {}).get('metadata', {})
        commit = meta.get('commit', meta.get('commit_hash', '?'))
        worker = meta.get('worker_id', '?')
        campaign = meta.get('campaign', '?')
        commits[commit] = commits.get(commit, 0) + 1
        workers[worker] = workers.get(worker, 0) + 1
        if campaign != '?':
            campaigns[campaign] = campaigns.get(campaign, 0) + 1

        h = e['histogram']
        tiers = {t['k']: (t['red'], t['blue']) for t in h['tiers']}
        c4 = tiers.get(4, (0,0))
        print(f'  #{e[\"rank\"]:>3}: 4c=({c4[0]},{c4[1]}) gap={e[\"goodman_gap\"]} commit={commit[:8]} worker={worker} admitted={e[\"admitted_at\"][:19]}')
    except:
        print(f'  #{e[\"rank\"]:>3}: (metadata unavailable)')

print()
print(f'--- Summary (top {$LIMIT} of {total}) ---')
print(f'  Commits:   {dict(sorted(commits.items(), key=lambda x: -x[1]))}')
print(f'  Workers:   {dict(sorted(workers.items(), key=lambda x: -x[1]))}')
if campaigns:
    print(f'  Campaigns: {dict(sorted(campaigns.items(), key=lambda x: -x[1]))}')

# Recent activity
print()
print('--- Most Recent 5 Admissions ---')
now = datetime.now(timezone.utc)
for e in by_date[:5]:
    h = e['histogram']
    tiers = {t['k']: (t['red'], t['blue']) for t in h['tiers']}
    c4 = tiers.get(4, (0,0))
    t = datetime.fromisoformat(e['admitted_at'])
    hours = (now - t).total_seconds() / 3600
    age = f'{hours:.1f}h' if hours < 24 else f'{hours/24:.1f}d'
    print(f'  #{e[\"rank\"]:>3}: 4c=({c4[0]},{c4[1]}) {age} ago')

# Admissions by day
by_day = Counter(e['admitted_at'][:10] for e in entries)
print()
print('--- Admissions by Day ---')
for day, count in sorted(by_day.items(), reverse=True)[:7]:
    print(f'  {day}: {count}')

# Key distribution
keys = Counter(e['key_id'][:8] for e in entries)
print()
print('--- Keys ---')
for key, count in sorted(keys.items(), key=lambda x: -x[1]):
    print(f'  {key}: {count} entries')
" 2>/dev/null || echo "(server unreachable)"
