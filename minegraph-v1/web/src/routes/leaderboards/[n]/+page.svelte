<script lang="ts">
	import { page } from '$app/stores';
	import GemView from '$lib/components/GemView.svelte';
	import { getLeaderboard, getHistory, subscribeEvents, type ServerEvent, type HistorySnapshot } from '$lib/api';

	interface RichEntry {
		rank: number;
		cid: string;
		key_id: string;
		graph6: string;
		goodman_gap: number | null;
		aut_order: number | null;
		histogram: { tiers: { k: number; red: number; blue: number }[] } | null;
		admitted_at: string;
	}

	const n = $derived(Number($page.params.n));
	let entries = $state<RichEntry[]>([]);
	let total = $state(0);
	let loading = $state(true);
	let selectedCid = $state<string | null>(null);
	let recentAdmissions = $state<ServerEvent[]>([]);
	let admitsPerMinute = $state(0);
	let admitTimestamps: number[] = [];
	let now = $state(Date.now());
	let currentPage = $state(0);
	let gemScroller = $state<HTMLDivElement | null>(null);
	let canScrollLeft = $state(false);
	let canScrollRight = $state(true);
	let historyOpen = $state(false);
	let historyData = $state<HistorySnapshot[]>([]);
	let historyRange = $state('all'); // 'all', '1h', '6h', '24h'

	const PAGE_SIZE = 50;
	const totalPages = $derived(Math.max(1, Math.ceil(total / PAGE_SIZE)));
	const canPrev = $derived(currentPage > 0);
	const canNext = $derived((currentPage + 1) * PAGE_SIZE < total);

	$effect(() => {
		const interval = setInterval(() => { now = Date.now(); }, 10000);
		return () => clearInterval(interval);
	});

	async function loadPage(page: number) {
		loading = true;
		try {
			const d = await getLeaderboard(n, PAGE_SIZE, page * PAGE_SIZE);
			entries = d.entries as RichEntry[];
			total = d.total;
			currentPage = page;
		} catch { /* ignore */ }
		loading = false;
	}

	function goNext() { if (canNext) loadPage(currentPage + 1); }
	function goPrev() { if (canPrev) loadPage(currentPage - 1); }
	function goFirst() { loadPage(0); }
	function goLast() { loadPage(totalPages - 1); }

	// Initial load
	$effect(() => { loadPage(0); });

	// SSE: refresh top of leaderboard on admission (don't append)
	$effect(() => {
		const unsub = subscribeEvents((event) => {
			if (event.n === n && event.type === 'admission') {
				recentAdmissions = [event, ...recentAdmissions.slice(0, 29)];
				admitTimestamps.push(Date.now());
				admitTimestamps = admitTimestamps.filter(t => t > Date.now() - 60000);
				admitsPerMinute = admitTimestamps.length;
				// Refresh current page
				loadPage(currentPage);
			}
		});
		return unsub;
	});

	const selected = $derived(entries.find(e => e.cid === selectedCid) ?? entries[0] ?? null);

	const cumulativeScore = $derived(() => {
		let sum = 0;
		for (const e of entries) {
			if (e.goodman_gap !== null) sum += e.goodman_gap;
			if (e.histogram?.tiers) {
				for (const t of e.histogram.tiers) sum += t.red + t.blue;
			}
		}
		return sum;
	});

	function timeAgo(isoStr: string): string {
		const diff = now - new Date(isoStr).getTime();
		const secs = Math.floor(diff / 1000);
		if (secs < 5) return 'just now';
		if (secs < 60) return `${secs}s ago`;
		const mins = Math.floor(secs / 60);
		if (mins < 60) return `${mins}m ago`;
		const hrs = Math.floor(mins / 60);
		if (hrs < 24) return `${hrs}h ${mins % 60}m ago`;
		return `${Math.floor(hrs / 24)}d ago`;
	}

	function recencyOpacity(isoStr: string): number {
		const mins = (now - new Date(isoStr).getTime()) / 60000;
		if (mins < 1) return 0.35;
		if (mins < 30) return 0.35 * (1 - mins / 30);
		return 0;
	}

	async function loadHistory() {
		const sinceMap: Record<string, string | undefined> = {
			'1h': new Date(Date.now() - 3600000).toISOString(),
			'6h': new Date(Date.now() - 21600000).toISOString(),
			'24h': new Date(Date.now() - 86400000).toISOString(),
			'all': undefined,
		};
		try {
			const d = await getHistory(n, sinceMap[historyRange]);
			historyData = d.snapshots || [];
		} catch { historyData = []; }
	}

	function copyToClipboard(text: string) {
		navigator.clipboard.writeText(text);
	}

	function updateScrollArrows() {
		if (!gemScroller) return;
		canScrollLeft = gemScroller.scrollLeft > 5;
		canScrollRight = gemScroller.scrollLeft < gemScroller.scrollWidth - gemScroller.clientWidth - 5;
	}

	function scrollGems(dir: number) {
		if (!gemScroller) return;
		gemScroller.scrollBy({ left: dir * 300, behavior: 'smooth' });
		// Update arrows after animation
		setTimeout(updateScrollArrows, 350);
	}
</script>

<div class="lb-page">
<!-- Top bar -->
<div class="top-bar">
	<div class="top-left">
		<h1>n = {n}</h1>
		<span class="total-badge">{total}</span>
	</div>
	<div class="top-right">
		{#if admitsPerMinute > 0}
			<span class="badge badge-amber">{admitsPerMinute}/min</span>
		{/if}
		<div class="cumulative-score" title="Cumulative score. Lower = better.">
			<span class="cs-label">Score</span>
			<span class="cs-value">{cumulativeScore().toLocaleString()}</span>
		</div>
		<!-- Tools: history + export -->
		<button class="tool-btn" onclick={() => { historyOpen = !historyOpen; if (historyOpen) loadHistory(); }}>
			{historyOpen ? 'Hide' : 'History'}
		</button>
		<a href="/api/leaderboards/{n}/export" download="leaderboard-n{n}.g6" class="tool-btn">Export .g6</a>
		<a href="/api/leaderboards/{n}/export/csv" download="leaderboard-n{n}.csv" class="tool-btn">CSV</a>
	</div>
</div>

<!-- History chart (collapsible) -->
{#if historyOpen && historyData.length > 0}
	<div class="history-panel">
		<div class="history-header">
			<span class="history-title">Score History</span>
			<div class="range-btns">
				{#each ['1h', '6h', '24h', 'all'] as range}
					<button class="range-btn" class:active={historyRange === range}
						onclick={() => { historyRange = range; loadHistory(); }}>{range}</button>
				{/each}
			</div>
		</div>
		<div class="history-chart">
			<table class="history-table">
				<thead><tr>
					<th>Time</th><th>Count</th><th>Best Gap</th><th>Median Gap</th><th>Avg Gap</th><th>Best |Aut|</th>
				</tr></thead>
				<tbody>
					{#each historyData as snap}
						<tr>
							<td class="mono tc">{new Date(snap.t).toLocaleTimeString()}</td>
							<td class="mono sc">{snap.count}</td>
							<td class="mono sc">{snap.best_gap ?? '—'}</td>
							<td class="mono sc">{snap.median_gap?.toFixed(1) ?? '—'}</td>
							<td class="mono sc">{snap.avg_gap?.toFixed(1) ?? '—'}</td>
							<td class="mono sc">{snap.best_aut ?? '—'}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	</div>
{:else if historyOpen}
	<div class="history-panel"><p class="empty">No snapshots yet. Snapshots are captured every 10 minutes.</p></div>
{/if}

<!-- Gem strip with scroll arrows -->
{#if entries.length > 0}
	<div class="gem-strip-wrap">
		<button class="scroll-arrow left" class:hidden={!canScrollLeft}
			onclick={() => scrollGems(-1)} aria-label="Scroll left">
			<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M15 18l-6-6 6-6"/></svg>
		</button>
		<div class="gem-strip" bind:this={gemScroller} onscroll={updateScrollArrows}>
			{#each entries as entry (entry.cid)}
				<button class="gem-btn" class:sel={entry.cid === (selectedCid ?? entries[0]?.cid)}
					onclick={() => { selectedCid = entry.cid; }}>
					<GemView graph6={entry.graph6} {n} size={72} cid={entry.cid}
						goodmanGap={entry.goodman_gap ?? 0} autOrder={entry.aut_order ?? 1}
						histogram={entry.histogram?.tiers ?? []} label="#{entry.rank}" />
				</button>
			{/each}
		</div>
		<button class="scroll-arrow right" class:hidden={!canScrollRight}
			onclick={() => scrollGems(1)} aria-label="Scroll right">
			<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M9 18l6-6-6-6"/></svg>
		</button>
	</div>
{/if}

<!-- Split: scrollable table + fixed detail -->
<div class="split">
	<div class="table-pane">
		<div class="toolbar">
			<div class="pager">
				<button class="pg-btn" onclick={goFirst} disabled={!canPrev} title="First page">&laquo;</button>
				<button class="pg-btn" onclick={goPrev} disabled={!canPrev} title="Previous page">&lsaquo;</button>
				<span class="pg-info">{currentPage * PAGE_SIZE + 1}–{Math.min((currentPage + 1) * PAGE_SIZE, total)} of {total}</span>
				<button class="pg-btn" onclick={goNext} disabled={!canNext} title="Next page">&rsaquo;</button>
				<button class="pg-btn" onclick={goLast} disabled={!canNext} title="Last page">&raquo;</button>
			</div>
		</div>

		{#if loading}
			<div class="shimmer" style="height: 200px; margin: 0.5rem;"></div>
		{:else}
			<table>
				<thead><tr>
					<th>Rank</th><th>CID</th><th>Histogram</th><th>Gap</th><th>|Aut|</th><th>Identity</th><th>When</th>
				</tr></thead>
				<tbody>
					{#each entries as entry (entry.cid)}
						{@const glow = recencyOpacity(entry.admitted_at)}
						<tr class:sel={entry.cid === (selectedCid ?? entries[0]?.cid)}
							onclick={() => { selectedCid = entry.cid; }}
							style={glow > 0 ? `background: rgba(99,102,241,${glow})` : ''}>
							<td class="rank">#{entry.rank}</td>
							<td><a href="/submissions/{entry.cid}" class="mono cc">{entry.cid.slice(0, 12)}</a></td>
							<td class="hist-cell">
								{#if entry.histogram?.tiers}
									{#each entry.histogram.tiers as t}
										<span class="hist-tier" title="k={t.k}: red={t.red} blue={t.blue}">
											<span class="hist-k">k{t.k}</span>
											<span class="red">{t.red}</span><span class="hist-sep">/</span><span class="blue">{t.blue}</span>
										</span>
									{/each}
								{:else}
									<span class="sc">—</span>
								{/if}
							</td>
							<td class="mono sc">{entry.goodman_gap ?? '—'}</td>
							<td class="mono sc">{entry.aut_order ?? '—'}</td>
							<td class="mono dm">{entry.key_id.slice(0, 8)}</td>
							<td class="mono tc" title={new Date(entry.admitted_at).toLocaleString()}>{timeAgo(entry.admitted_at)}</td>
						</tr>
					{/each}
				</tbody>
			</table>
			{#if currentPage >= totalPages - 1 && entries.length > 0}<div class="end-msg">End of leaderboard</div>{/if}
		{/if}
	</div>

	<!-- Detail panel -->
	<aside class="detail">
		{#if selected}
			<a href="/submissions/{selected.cid}" class="gem-link">
				<GemView graph6={selected.graph6} {n} size={240} cid={selected.cid}
					goodmanGap={selected.goodman_gap ?? 0} autOrder={selected.aut_order ?? 1}
					histogram={selected.histogram?.tiers ?? []} />
			</a>
			<div class="info card">
				<div class="info-rank">#{selected.rank}</div>
				<dl>
					<dt>CID</dt><dd class="mono" style="font-size:0.6rem;word-break:break-all">{selected.cid}</dd>
					{#if selected.goodman_gap !== null}<dt>Gap</dt><dd class="mono">{selected.goodman_gap}</dd>{/if}
					{#if selected.aut_order !== null}<dt>|Aut|</dt><dd class="mono">{selected.aut_order}</dd>{/if}
					{#if selected.histogram?.tiers}
						<dt>Hist</dt>
						<dd>{#each selected.histogram.tiers as t}
							<div class="mono" style="font-size:0.68rem;color:var(--color-text-muted)">k={t.k}: <span style="color:#ef4444">{t.red}</span>/<span style="color:#60a5fa">{t.blue}</span></div>
						{/each}</dd>
					{/if}
					<dt>Key</dt><dd class="mono dm"><a href="/identities/{selected.key_id}">{selected.key_id}</a></dd>
					<dt>When</dt><dd class="dm" title={new Date(selected.admitted_at).toLocaleString()}>{timeAgo(selected.admitted_at)}</dd>
				</dl>
				<div class="detail-actions">
					<button class="copy-btn" onclick={() => copyToClipboard(selected.graph6)} title="Copy graph6">Copy g6</button>
					<button class="copy-btn" onclick={() => copyToClipboard(selected.cid)} title="Copy CID">Copy CID</button>
					<a href="/submissions/{selected.cid}" class="copy-btn">Details</a>
				</div>
			</div>
			{#if recentAdmissions.length > 0}
				<div class="feed card">
					<h3>Live</h3>
					{#each recentAdmissions.slice(0, 8) as ev (ev.cid)}
						<div class="fr"><span class="badge badge-green" style="font-size:0.55rem">#{ev.rank}</span><span class="mono" style="font-size:0.6rem;color:var(--color-text-dim)">{ev.cid.slice(0, 10)}</span></div>
					{/each}
				</div>
			{/if}
		{/if}
	</aside>
</div>
</div><!-- .lb-page -->

<style>
	.lb-page {
		flex: 1;
		display: flex;
		flex-direction: column;
		min-height: 0;
		overflow: hidden;
	}

	/* Top bar */
	.top-bar { display: flex; justify-content: space-between; align-items: center; margin-bottom: 0.5rem; flex-shrink: 0; }
	.top-left { display: flex; align-items: baseline; gap: 0.5rem; }
	h1 { font-family: var(--font-mono); font-size: 1.3rem; color: var(--color-accent); }
	.total-badge { font-size: 0.75rem; color: var(--color-text-muted); font-family: var(--font-mono); }
	.top-right { display: flex; align-items: center; gap: 0.4rem; flex-wrap: wrap; }
	.tool-btn {
		font-size: 0.65rem; font-family: var(--font-mono);
		padding: 0.2rem 0.5rem; border-radius: 0.25rem;
		background: var(--color-surface); border: 1px solid var(--color-border);
		color: var(--color-text-muted); cursor: pointer; text-decoration: none;
		transition: border-color 0.15s, color 0.15s;
	}
	.tool-btn:hover { border-color: var(--color-accent); color: var(--color-accent); }
	.cumulative-score { background: var(--color-surface); border: 1px solid var(--color-border); border-radius: 0.4rem; padding: 0.2rem 0.6rem; display: flex; flex-direction: column; align-items: flex-end; }
	.cs-label { font-size: 0.55rem; text-transform: uppercase; letter-spacing: 0.05em; color: var(--color-text-dim); }
	.cs-value { font-family: var(--font-mono); font-size: 1rem; font-weight: 700; color: var(--color-accent); }

	/* Gem strip */
	.gem-strip-wrap {
		position: relative;
		flex-shrink: 0;
		border-bottom: 1px solid var(--color-border);
		margin-bottom: 0.5rem;
	}
	.gem-strip {
		display: flex; gap: 0.35rem;
		overflow-x: auto; padding: 0.25rem 2rem 0.5rem;
		scrollbar-width: none;
	}
	.gem-strip::-webkit-scrollbar { display: none; }
	.scroll-arrow {
		position: absolute; top: 0; bottom: 0;
		width: 2rem; z-index: 3;
		display: flex; align-items: center; justify-content: center;
		background: none; border: none; cursor: pointer;
		color: var(--color-text-muted);
		transition: color 0.15s, opacity 0.2s;
		opacity: 0.7;
	}
	.scroll-arrow:hover { color: var(--color-accent); opacity: 1; }
	.scroll-arrow.hidden { opacity: 0; pointer-events: none; }
	.scroll-arrow.left {
		left: 0;
		background: linear-gradient(90deg, var(--color-bg) 40%, transparent);
	}
	.scroll-arrow.right {
		right: 0;
		background: linear-gradient(-90deg, var(--color-bg) 40%, transparent);
	}
	.scroll-arrow svg { width: 20px; height: 20px; }
	.gem-btn { background: none; border: 2px solid transparent; border-radius: 0.4rem; cursor: pointer; padding: 1px; transition: border-color 0.15s, transform 0.1s; flex-shrink: 0; }
	.gem-btn:hover { border-color: rgba(99,102,241,0.3); transform: scale(1.06); }
	.gem-btn.sel { border-color: var(--color-accent); }

	/* Split */
	.split { display: grid; grid-template-columns: 1fr 280px; gap: 1rem; flex: 1; min-height: 0; }
	@media (max-width: 860px) { .split { grid-template-columns: 1fr; } }

	/* Table pane */
	.table-pane { overflow-y: auto; border: 1px solid var(--color-border); border-radius: 0.5rem; background: var(--color-surface); min-height: 0; scrollbar-width: thin; scrollbar-color: var(--color-border) transparent; }
	.toolbar { display: flex; justify-content: center; align-items: center; padding: 0.3rem 0.6rem; border-bottom: 1px solid var(--color-border); position: sticky; top: 0; z-index: 2; background: var(--color-surface); }
	.pager { display: flex; align-items: center; gap: 0.25rem; }
	.pg-btn {
		background: none; border: 1px solid var(--color-border); border-radius: 0.25rem;
		color: var(--color-text-muted); font-size: 0.85rem; padding: 0.1rem 0.45rem;
		cursor: pointer; transition: border-color 0.15s, color 0.15s;
		font-family: var(--font-mono); line-height: 1.2;
	}
	.pg-btn:hover:not(:disabled) { border-color: var(--color-accent); color: var(--color-accent); }
	.pg-btn:disabled { opacity: 0.25; cursor: default; }
	.pg-info { font-size: 0.68rem; color: var(--color-text-muted); font-family: var(--font-mono); padding: 0 0.4rem; white-space: nowrap; }
	table { width: 100%; border-collapse: collapse; font-size: 0.78rem; }
	thead { position: sticky; top: 30px; z-index: 1; background: var(--color-surface); }
	th { text-align: left; padding: 0.3rem 0.4rem; border-bottom: 1px solid var(--color-border); color: var(--color-text-muted); font-weight: 500; font-size: 0.65rem; text-transform: uppercase; letter-spacing: 0.04em; }
	td { padding: 0.25rem 0.4rem; border-bottom: 1px solid rgba(42,42,58,0.2); transition: background 0.3s; }
	tr { cursor: pointer; transition: background 0.2s; }
	tr:hover { background: rgba(99,102,241,0.06) !important; }
	tr.sel { background: var(--color-accent-glow) !important; }
	.rank { font-family: var(--font-mono); font-weight: 700; color: var(--color-accent); width: 2.5rem; }
	.cc { font-size: 0.7rem; }
	.sc { font-size: 0.7rem; color: var(--color-text-muted); }
	.tc { font-size: 0.65rem; color: var(--color-text-dim); white-space: nowrap; }
	.dm { color: var(--color-text-muted); font-size: 0.7rem; }
	.hist-cell { font-family: var(--font-mono); font-size: 0.65rem; white-space: nowrap; }
	.hist-tier { display: inline-flex; align-items: center; gap: 0.1rem; margin-right: 0.4rem; }
	.hist-k { color: var(--color-text-dim); font-size: 0.55rem; margin-right: 0.15rem; }
	.hist-sep { color: var(--color-text-dim); }
	.red { color: #ef4444; }
	.blue { color: #60a5fa; }
	.end-msg { text-align: center; padding: 0.5rem; color: var(--color-text-dim); font-size: 0.7rem; font-style: italic; }

	/* Detail */
	.detail { overflow-y: auto; display: flex; flex-direction: column; gap: 0.6rem; scrollbar-width: thin; scrollbar-color: var(--color-border) transparent; }
	.gem-link { display: flex; justify-content: center; transition: transform 0.15s; }
	.gem-link:hover { transform: scale(1.02); }
	.info-rank { font-family: var(--font-mono); font-size: 1.3rem; font-weight: 800; color: var(--color-accent); margin-bottom: 0.4rem; }
	dl { display: grid; grid-template-columns: auto 1fr; gap: 0.15rem 0.5rem; }
	dt { font-size: 0.7rem; color: var(--color-text-muted); }
	dd { font-size: 0.75rem; }
	.detail-actions { display: flex; gap: 0.3rem; margin-top: 0.5rem; flex-wrap: wrap; }
	.copy-btn {
		font-size: 0.65rem; font-family: var(--font-mono);
		padding: 0.2rem 0.5rem; border-radius: 0.25rem;
		background: var(--color-bg); border: 1px solid var(--color-border);
		color: var(--color-text-muted); cursor: pointer; text-decoration: none;
		transition: border-color 0.15s, color 0.15s;
	}
	.copy-btn:hover { border-color: var(--color-accent); color: var(--color-accent); }
	.feed h3 { font-size: 0.65rem; text-transform: uppercase; letter-spacing: 0.04em; color: var(--color-text-muted); margin-bottom: 0.3rem; font-family: var(--font-mono); }
	.fr { display: flex; align-items: center; gap: 0.4rem; padding: 0.1rem 0; }

	/* History panel */
	.history-panel {
		flex-shrink: 0; margin-bottom: 0.5rem; padding: 0.5rem 0.75rem;
		background: var(--color-surface); border: 1px solid var(--color-border);
		border-radius: 0.5rem; max-height: 200px; overflow-y: auto;
	}
	.history-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 0.4rem; }
	.history-title { font-family: var(--font-mono); font-size: 0.75rem; color: var(--color-text-muted); }
	.range-btns { display: flex; gap: 0.2rem; }
	.range-btn {
		font-size: 0.6rem; font-family: var(--font-mono);
		padding: 0.1rem 0.35rem; border-radius: 0.2rem;
		background: none; border: 1px solid var(--color-border);
		color: var(--color-text-dim); cursor: pointer;
	}
	.range-btn:hover { color: var(--color-text); border-color: var(--color-text-muted); }
	.range-btn.active { background: var(--color-accent); color: white; border-color: var(--color-accent); }
	.history-table { width: 100%; border-collapse: collapse; font-size: 0.7rem; }
	.history-table th { padding: 0.2rem 0.3rem; text-align: left; color: var(--color-text-dim); font-size: 0.6rem; text-transform: uppercase; border-bottom: 1px solid var(--color-border); }
	.history-table td { padding: 0.15rem 0.3rem; }
	.empty { color: var(--color-text-dim); font-size: 0.75rem; font-style: italic; text-align: center; padding: 1rem; }
</style>
