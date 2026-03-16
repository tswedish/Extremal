<script lang="ts">
	import { page } from '$app/state';
	import { getLeaderboard, getLeaderboardGraphs, type LeaderboardDetail, type RgxfJson } from '$lib/api';
	import MatrixView from '$lib/components/MatrixView.svelte';
	import CircleLayout from '$lib/components/CircleLayout.svelte';
	import GraphThumb from '$lib/components/GraphThumb.svelte';
	import GemView from '$lib/components/GemView.svelte';

	let detail = $state<LeaderboardDetail | null>(null);
	let loading = $state(true);
	let error = $state('');
	let flashCids = $state<Set<string>>(new Set());
	let graphs = $state<RgxfJson[]>([]);
	let now = $state(Date.now());

	// Pagination state
	const PAGE_SIZE = 50;
	let currentPage = $state(1);
	let totalPages = $derived(detail ? Math.max(1, Math.ceil(detail.total / PAGE_SIZE)) : 1);

	// Tick every 5s for relative timestamps (uses $effect cleanup, not onDestroy)
	$effect(() => {
		const tick = setInterval(() => { now = Date.now(); }, 5_000);
		return () => clearInterval(tick);
	});

	// Track previous CIDs to detect new entries on refresh
	let prevCids = new Set<string>();

	/** Format admitted time: relative if <8h, absolute otherwise */
	function formatAdmitted(admittedAt: string): string {
		const ageMs = now - new Date(admittedAt).getTime();
		if (ageMs < 0) return 'just now';
		if (ageMs < 60_000) return `${Math.floor(ageMs / 1000)}s ago`;
		if (ageMs < 3_600_000) return `${Math.floor(ageMs / 60_000)}m ago`;
		if (ageMs < 8 * 3_600_000) return `${(ageMs / 3_600_000).toFixed(1)}h ago`;
		return new Date(admittedAt).toLocaleString();
	}

	/**
	 * Distribution-based recency highlighting.
	 * Computes percentile rank of each entry's age among all entries,
	 * then maps top quantiles to green -> yellow -> transparent.
	 * Intensity decays with absolute age of the newest entry (fades fully after 24h of inactivity).
	 */
	const recencyMap = $derived.by(() => {
		const map = new Map<string, string>();
		if (!detail || detail.entries.length < 2) return map;

		const entries = detail.entries;
		const ages = entries.map(e => ({
			cid: e.graph_cid,
			age: now - new Date(e.admitted_at).getTime()
		}));

		// Sort ascending by age (newest first)
		const sorted = [...ages].sort((a, b) => a.age - b.age);
		const n = sorted.length;

		// Absolute freshness decay: if even the newest entry is old, fade everything
		const newestAge = sorted[0].age;
		const freshness = Math.max(0, 1 - newestAge / (24 * 3_600_000)); // -> 0 over 24h
		if (freshness < 0.02) return map;

		// Assign percentile rank: 0 = newest, 1 = oldest
		const rankOf = new Map<string, number>();
		for (let i = 0; i < n; i++) {
			rankOf.set(sorted[i].cid, i / (n - 1));
		}

		for (const { cid } of ages) {
			const t = rankOf.get(cid)!; // 0 = newest, 1 = oldest

			// Intensity: cubic dropoff emphasizing top quantiles
			const raw = (1 - t) * (1 - t) * (1 - t); // 1.0 -> 0.0
			const alpha = 0.35 * raw * freshness;
			if (alpha < 0.008) continue;

			// Hue: 140 (green) for top quantile -> 55 (yellow) -> 40 (warm) for bottom
			const hue = 140 - t * 100;
			map.set(cid, `background-color: hsla(${Math.round(hue)}, 75%, 50%, ${alpha.toFixed(3)})`);
		}

		return map;
	});

	function refresh(k: number, l: number, n: number, pg: number = currentPage) {
		const offset = (pg - 1) * PAGE_SIZE;
		Promise.all([
			getLeaderboard(k, l, n, offset, PAGE_SIZE),
			getLeaderboardGraphs(k, l, n, PAGE_SIZE, offset)
		])
			.then(([data, graphData]) => {
				// Detect new or rank-changed entries
				const newFlash = new Set<string>();
				for (const entry of data.entries) {
					if (!prevCids.has(entry.graph_cid)) {
						newFlash.add(entry.graph_cid);
					}
				}

				prevCids = new Set(data.entries.map((e) => e.graph_cid));
				detail = data;
				graphs = graphData;

				if (newFlash.size > 0) {
					flashCids = newFlash;
					setTimeout(() => { flashCids = new Set(); }, 1500);
				}
			})
			.catch((e) => {
				error = e instanceof Error ? e.message : 'Failed to load leaderboard';
			})
			.finally(() => {
				loading = false;
			});
	}

	/** Navigate to a specific page */
	function goToPage(pg: number) {
		if (pg < 1 || pg > totalPages || pg === currentPage) return;
		currentPage = pg;
		loading = true;
		const k = Number(page.params.k);
		const l = Number(page.params.l);
		const n = Number(page.params.n);
		refresh(k, l, n, pg);
	}

	/** Compute visible page numbers for pagination controls */
	function visiblePages(current: number, total: number): number[] {
		if (total <= 7) return Array.from({ length: total }, (_, i) => i + 1);
		const pages: number[] = [1];
		const start = Math.max(2, current - 1);
		const end = Math.min(total - 1, current + 1);
		if (start > 2) pages.push(-1); // ellipsis marker
		for (let i = start; i <= end; i++) pages.push(i);
		if (end < total - 1) pages.push(-1); // ellipsis marker
		pages.push(total);
		return pages;
	}

	/** Export leaderboard as CSV with full metadata + RGXF. */
	async function exportCsv() {
		if (!detail) return;
		// Fetch current page graphs for CSV
		const offset = (currentPage - 1) * PAGE_SIZE;
		const allGraphs = await getLeaderboardGraphs(detail.k, detail.ell, detail.n, PAGE_SIZE, offset);

		const header = 'rank,graph_cid,k,ell,n,c_max,c_min,goodman_gap,aut_order,admitted_at,encoding,bits_b64';
		const rows = detail.entries.map((e, i) => {
			const g = allGraphs[i];
			const enc = g?.encoding ?? '';
			const bits = g?.bits_b64 ?? '';
			return [
				e.rank,
				e.graph_cid,
				detail!.k,
				detail!.ell,
				detail!.n,
				e.tier1_max,
				e.tier1_min,
				e.goodman_gap,
				e.tier2_aut,
				e.admitted_at,
				enc,
				bits
			].join(',');
		});

		const csv = [header, ...rows].join('\n');
		const blob = new Blob([csv], { type: 'text/csv' });
		const url = URL.createObjectURL(blob);
		const a = document.createElement('a');
		a.href = url;
		a.download = `ramseynet-R(${detail.k},${detail.ell})-n${detail.n}-page${currentPage}.csv`;
		a.click();
		URL.revokeObjectURL(url);
	}

	// Initial load + reload on param change
	$effect(() => {
		const k = Number(page.params.k);
		const l = Number(page.params.l);
		const n = Number(page.params.n);
		loading = true;
		error = '';
		detail = null;
		prevCids = new Set();
		flashCids = new Set();
		graphs = [];
		currentPage = 1;

		refresh(k, l, n, 1);
	});

	// Smart polling: visibility-aware with error backoff
	$effect(() => {
		const k = Number(page.params.k);
		const l = Number(page.params.l);
		const n = Number(page.params.n);

		const BASE_DELAY = 10_000;
		const MAX_DELAY = 60_000;
		let delay = BASE_DELAY;
		let timer: ReturnType<typeof setTimeout>;
		let refreshing = false;

		function scheduleNext() {
			timer = setTimeout(poll, delay);
		}

		async function poll() {
			if (typeof document !== 'undefined' && document.hidden) {
				scheduleNext();
				return;
			}
			if (refreshing) {
				scheduleNext();
				return;
			}
			refreshing = true;
			try {
				const offset = (currentPage - 1) * PAGE_SIZE;
				await Promise.all([
					getLeaderboard(k, l, n, offset, PAGE_SIZE),
					getLeaderboardGraphs(k, l, n, PAGE_SIZE, offset)
				]).then(([data, graphData]) => {
					const newFlash = new Set<string>();
					for (const entry of data.entries) {
						if (!prevCids.has(entry.graph_cid)) {
							newFlash.add(entry.graph_cid);
						}
					}
					prevCids = new Set(data.entries.map((e) => e.graph_cid));
					detail = data;
					graphs = graphData;
					if (newFlash.size > 0) {
						flashCids = newFlash;
						setTimeout(() => { flashCids = new Set(); }, 1500);
					}
					error = '';
				});
				delay = BASE_DELAY; // reset on success
			} catch (e) {
				error = e instanceof Error ? e.message : 'Failed to refresh';
				delay = Math.min(delay * 2, MAX_DELAY); // backoff on error
			} finally {
				refreshing = false;
				scheduleNext();
			}
		}

		// Resume polling immediately when tab becomes visible
		function onVisible() {
			if (typeof document !== 'undefined' && !document.hidden) {
				clearTimeout(timer);
				delay = BASE_DELAY;
				poll();
			}
		}

		if (typeof document !== 'undefined') {
			document.addEventListener('visibilitychange', onVisible);
		}
		scheduleNext();

		return () => {
			clearTimeout(timer);
			if (typeof document !== 'undefined') {
				document.removeEventListener('visibilitychange', onVisible);
			}
		};
	});
</script>

<svelte:head>
	<title>{detail ? `R(${detail.k},${detail.ell}) n=${detail.n}` : 'Leaderboard'} — RamseyNet</title>
</svelte:head>

<div class="page">
	{#if loading && !detail}
		<div class="loading">Loading leaderboard...</div>
	{:else if error && !detail}
		<div class="error">{error}</div>
	{:else if detail}
		<a href="/leaderboards" class="back-link">Leaderboards</a>

		<h1>R({detail.k},{detail.ell}) <span class="n-label">n={detail.n}</span></h1>
		<div class="subtitle-row">
			{#if detail.total > 0}
				{@const startRank = detail.offset + 1}
				{@const endRank = detail.offset + detail.entries.length}
				<p class="subtitle">
					{#if detail.total <= PAGE_SIZE}
						{detail.total} ranked {detail.total === 1 ? 'entry' : 'entries'}
					{:else}
						Showing {startRank}–{endRank} of {detail.total.toLocaleString()} entries
					{/if}
				</p>
			{:else}
				<p class="subtitle">No entries yet</p>
			{/if}
			{#if detail.entries.length > 0}
				<button class="export-btn" onclick={exportCsv}>Export CSV</button>
			{/if}
			{#if loading}
				<span class="poll-indicator" title="Refreshing..."></span>
			{/if}
		</div>

		{#if detail.top_graph && currentPage === 1}
			<section class="viz-section">
				<h2>Top Graph</h2>
				<div class="viz-row">
					<GemView rgxf={detail.top_graph} size={360} label="MineGraph Gem" />
					<MatrixView rgxf={detail.top_graph} size={360} />
					<CircleLayout rgxf={detail.top_graph} size={360} />
				</div>
			</section>
		{/if}

		{#if detail.entries.length > 0}
			<section class="table-section">
				<table>
					<thead>
						<tr>
							<th>#</th>
							<th>Graph</th>
							<th>CID</th>
							<th>Submitter</th>
							<th>C<sub>max</sub></th>
							<th>C<sub>min</sub></th>
							<th title="Goodman gap: distance from minimum monochromatic triangles (0 = optimal)">Gap</th>
							<th>|Aut|</th>
							<th>Admitted</th>
						</tr>
					</thead>
					<tbody>
						{#each detail.entries as entry, i (entry.graph_cid)}
							<tr class:rank1={entry.rank === 1} class:flash={flashCids.has(entry.graph_cid)} style={recencyMap.get(entry.graph_cid) ?? ''}>
								<td class="rank">{entry.rank}</td>
								<td class="thumb">
									{#if graphs[i]}
										<a href="/submissions/{entry.graph_cid}">
											<GraphThumb rgxf={graphs[i]} size={48} />
										</a>
									{/if}
								</td>
								<td class="cid">
									<a href="/submissions/{entry.graph_cid}">{entry.graph_cid.slice(0, 16)}...</a>
								</td>
							<td class="submitter">
								{#if entry.key_id}
									<a href="/keys/{entry.key_id}" title={entry.key_id}>
										{entry.key_id.slice(0, 8)}...
									</a>
									{#if entry.commit_hash}
										<span class="commit" title="commit: {entry.commit_hash}">{entry.commit_hash.slice(0, 7)}</span>
									{/if}
								{:else}
									<span class="anon">anon</span>
								{/if}
							</td>
								<td class="score">{entry.tier1_max}</td>
								<td class="score">{entry.tier1_min}</td>
								<td class="score" class:gap-zero={entry.goodman_gap === 0}>{entry.goodman_gap}</td>
								<td class="score">{entry.tier2_aut}</td>
								<td class="timestamp" title={new Date(entry.admitted_at).toLocaleString()}>{formatAdmitted(entry.admitted_at)}</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</section>

			{#if totalPages > 1}
				<nav class="pagination">
					<button
						class="page-btn"
						disabled={currentPage === 1}
						onclick={() => goToPage(1)}
						title="First page"
					>&laquo;</button>
					<button
						class="page-btn"
						disabled={currentPage === 1}
						onclick={() => goToPage(currentPage - 1)}
						title="Previous page"
					>&lsaquo;</button>

					{#each visiblePages(currentPage, totalPages) as pg}
						{#if pg === -1}
							<span class="page-ellipsis">&hellip;</span>
						{:else}
							<button
								class="page-btn"
								class:active={pg === currentPage}
								onclick={() => goToPage(pg)}
							>{pg}</button>
						{/if}
					{/each}

					<button
						class="page-btn"
						disabled={currentPage === totalPages}
						onclick={() => goToPage(currentPage + 1)}
						title="Next page"
					>&rsaquo;</button>
					<button
						class="page-btn"
						disabled={currentPage === totalPages}
						onclick={() => goToPage(totalPages)}
						title="Last page"
					>&raquo;</button>
				</nav>
			{/if}
		{:else}
			<div class="empty">No entries yet. Submit a graph to be the first!</div>
		{/if}
	{/if}
</div>

<style>
	.page {
		max-width: 900px;
	}

	.loading, .error {
		padding: 2rem;
		text-align: center;
		color: var(--color-text-muted);
		font-size: 0.875rem;
	}

	.error {
		color: var(--color-rejected);
	}

	.back-link {
		font-size: 0.8125rem;
		color: var(--color-text-muted);
		display: inline-block;
		margin-bottom: 0.75rem;
	}

	.back-link::before {
		content: '\2190 ';
	}

	.back-link:hover {
		color: var(--color-accent);
	}

	h1 {
		font-family: var(--font-mono);
		font-size: 2rem;
		font-weight: 700;
	}

	.n-label {
		color: var(--color-accepted);
	}

	.subtitle-row {
		display: flex;
		align-items: center;
		gap: 1rem;
		margin-bottom: 1.5rem;
	}

	.subtitle {
		color: var(--color-text-muted);
		font-size: 0.875rem;
	}

	.poll-indicator {
		display: inline-block;
		width: 8px;
		height: 8px;
		border-radius: 50%;
		background: var(--color-accent);
		opacity: 0.6;
		animation: pulse 1s ease-in-out infinite;
	}

	@keyframes pulse {
		0%, 100% { opacity: 0.3; }
		50% { opacity: 0.8; }
	}

	.export-btn {
		font-family: var(--font-mono);
		font-size: 0.6875rem;
		color: var(--color-text-muted);
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-radius: 0.375rem;
		padding: 0.25rem 0.625rem;
		cursor: pointer;
		transition: border-color 0.2s, color 0.2s;
	}

	.export-btn:hover {
		color: var(--color-accent);
		border-color: var(--color-accent);
	}

	h2 {
		font-family: var(--font-mono);
		font-size: 1.125rem;
		font-weight: 600;
		margin-bottom: 1rem;
	}

	.viz-section {
		margin-bottom: 2rem;
		padding-bottom: 2rem;
		border-bottom: 1px solid var(--color-border);
	}

	.viz-row {
		display: flex;
		flex-wrap: wrap;
		gap: 1.5rem;
	}

	.table-section {
		margin-top: 1rem;
	}

	table {
		width: 100%;
		border-collapse: collapse;
	}

	th {
		text-align: left;
		font-family: var(--font-mono);
		font-size: 0.75rem;
		font-weight: 600;
		color: var(--color-text-muted);
		text-transform: uppercase;
		letter-spacing: 0.05em;
		padding: 0.5rem 0.75rem;
		border-bottom: 1px solid var(--color-border);
	}

	td {
		padding: 0.625rem 0.75rem;
		font-size: 0.875rem;
		border-bottom: 1px solid var(--color-border);
	}

	.rank {
		font-family: var(--font-mono);
		font-weight: 700;
		color: var(--color-text-muted);
		width: 2rem;
	}

	tr.rank1 .rank {
		color: var(--color-accepted);
	}

	.thumb {
		width: 48px;
		padding: 0.375rem 0.5rem;
	}

	.thumb a {
		display: block;
		line-height: 0;
	}

	.thumb a:hover {
		opacity: 0.8;
	}

	.cid {
		font-family: var(--font-mono);
		font-size: 0.75rem;
	}

	.cid a {
		color: var(--color-text);
		text-decoration: none;
	}

	.cid a:hover {
		color: var(--color-accent);
	}

	.submitter {
		font-family: var(--font-mono);
		font-size: 0.75rem;
	}

	.submitter a {
		color: var(--color-accent);
		text-decoration: none;
	}

	.submitter a:hover {
		text-decoration: underline;
	}

	.submitter .anon {
		color: var(--color-text-muted);
		font-style: italic;
	}

	.submitter .commit {
		display: block;
		font-size: 0.625rem;
		color: var(--color-text-muted);
		margin-top: 0.125rem;
	}

	.score {
		font-family: var(--font-mono);
		font-size: 0.8125rem;
	}

	.score.gap-zero {
		color: var(--color-accepted);
		font-weight: 700;
	}

	.timestamp {
		font-size: 0.8125rem;
		color: var(--color-text-muted);
	}

	.empty {
		padding: 2rem;
		text-align: center;
		color: var(--color-text-muted);
		font-size: 0.875rem;
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-radius: 0.75rem;
	}

	/* Flash animation for newly admitted entries */
	@keyframes flash-row {
		0% { background-color: transparent; }
		20% { background-color: rgba(46, 204, 113, 0.25); }
		100% { background-color: transparent; }
	}

	tr.flash {
		animation: flash-row 1.5s ease-out;
	}

	/* Pagination */
	.pagination {
		display: flex;
		justify-content: center;
		align-items: center;
		gap: 0.25rem;
		margin-top: 1.5rem;
		padding-top: 1rem;
		border-top: 1px solid var(--color-border);
	}

	.page-btn {
		font-family: var(--font-mono);
		font-size: 0.75rem;
		min-width: 2rem;
		height: 2rem;
		display: flex;
		align-items: center;
		justify-content: center;
		padding: 0 0.375rem;
		border: 1px solid var(--color-border);
		border-radius: 0.375rem;
		background: var(--color-surface);
		color: var(--color-text-muted);
		cursor: pointer;
		transition: border-color 0.2s, color 0.2s, background-color 0.2s;
	}

	.page-btn:hover:not(:disabled) {
		color: var(--color-accent);
		border-color: var(--color-accent);
	}

	.page-btn:disabled {
		opacity: 0.3;
		cursor: not-allowed;
	}

	.page-btn.active {
		background: var(--color-accent);
		border-color: var(--color-accent);
		color: var(--color-bg);
		font-weight: 700;
	}

	.page-ellipsis {
		font-family: var(--font-mono);
		font-size: 0.75rem;
		color: var(--color-text-muted);
		padding: 0 0.25rem;
	}
</style>
