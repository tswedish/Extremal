<script lang="ts">
	import GemView from '$lib/components/GemView.svelte';
	import { getHealth, getLeaderboards, getLeaderboard, subscribeEvents, type ServerEvent } from '$lib/api';

	interface TopGem {
		graph6: string;
		n: number;
		cid: string;
		goodmanGap: number;
		autOrder: number;
		histogram: { k: number; red: number; blue: number }[];
	}

	let health = $state<{ status: string; version: string; server_key_id: string } | null>(null);
	let topGem = $state<TopGem | null>(null);
	let recentEvents = $state<ServerEvent[]>([]);
	let totalEntries = $state(0);
	let topN = $state(0);

	async function fetchTopGem() {
		try {
			const data = await getLeaderboards();
			totalEntries = data.leaderboards.reduce((s, l) => s + l.entry_count, 0);
			if (data.leaderboards.length > 0) {
				const best = data.leaderboards.reduce((a, b) => a.entry_count > b.entry_count ? a : b);
				topN = best.n;
				const detail = await getLeaderboard(best.n, 1);
				const entry = detail.entries[0];
				if (entry?.graph6) {
					topGem = {
						graph6: entry.graph6,
						n: best.n,
						cid: entry.cid,
						goodmanGap: entry.goodman_gap ?? 0,
						autOrder: entry.aut_order ?? 1,
						histogram: entry.histogram?.tiers ?? [],
					};
				}
			}
		} catch { /* ignore */ }
	}

	$effect(() => {
		getHealth().then(h => { health = h; }).catch(() => { health = null; });
		fetchTopGem();

		const unsub = subscribeEvents((event) => {
			recentEvents = [event, ...recentEvents.slice(0, 19)];
			// Refresh top gem when a new #1 is admitted
			if (event.type === 'admission' && event.rank === 1) {
				fetchTopGem();
			}
		});
		return unsub;
	});
</script>

<div class="hero">
	<h1 class="title">Extremal</h1>
	<p class="subtitle">Competitive graph search. Discover, score, rank.</p>
	<div class="badges">
		{#if health}
			<span class="badge badge-green">{health.status}</span>
		{:else}
			<span class="badge badge-red">offline</span>
		{/if}
		{#if totalEntries > 0}
			<span class="badge badge-amber">{totalEntries} graphs</span>
		{/if}
	</div>
</div>

<div class="showcase">
	{#if topGem}
		<a href="/submissions/{topGem.cid}" class="gem-link">
			<GemView
				graph6={topGem.graph6}
				n={topGem.n}
				size={320}
				cid={topGem.cid}
				goodmanGap={topGem.goodmanGap}
				autOrder={topGem.autOrder}
				histogram={topGem.histogram}
				label="#1 on n={topGem.n}"
			/>
		</a>
	{:else}
		<div class="gem-placeholder shimmer" style="width: 320px; height: 320px; border-radius: 0.75rem;"></div>
	{/if}
</div>

{#if recentEvents.length > 0}
	<section class="feed">
		<h2>Live Activity</h2>
		<div class="events">
			{#each recentEvents as event (event.cid)}
				<div class="event-row event-enter">
					{#if event.type === 'admission'}
						<span class="badge badge-green">#{event.rank}</span>
					{:else}
						<span class="badge badge-amber">sub</span>
					{/if}
					<a href="/submissions/{event.cid}" class="mono event-cid">{event.cid?.slice(0, 12)}...</a>
					<a href="/leaderboards/{event.n}" class="event-meta">n={event.n}</a>
					<a href="/identities/{event.key_id}" class="event-key mono">{event.key_id?.slice(0, 8)}</a>
				</div>
			{/each}
		</div>
	</section>
{/if}

<nav class="grid">
	<a href="/leaderboards" class="card nav-card">
		<h3>Leaderboards</h3>
		<p>Browse ranked graphs by vertex count</p>
	</a>
	<a href="/dashboard" class="card nav-card">
		<h3>Dashboard</h3>
		<p>Monitor search workers in real time</p>
	</a>
</nav>

<style>
	.hero { text-align: center; margin-bottom: 2rem; }
	.title {
		font-size: 3rem;
		font-weight: 800;
		font-family: var(--font-mono);
		background: linear-gradient(135deg, #6366f1, #a855f7, #ec4899);
		-webkit-background-clip: text;
		-webkit-text-fill-color: transparent;
		background-clip: text;
		margin-bottom: 0.5rem;
	}
	.subtitle { color: var(--color-text-muted); font-size: 1.1rem; margin-bottom: 1rem; }
	.badges { display: flex; gap: 0.5rem; justify-content: center; }
	.showcase { display: flex; justify-content: center; margin: 2.5rem 0; }
	.gem-link { transition: transform 0.2s; display: inline-block; }
	.gem-link:hover { transform: scale(1.02); }
	.feed { margin: 2rem 0; }
	.feed h2 { font-size: 1rem; color: var(--color-text-muted); margin-bottom: 0.75rem; font-family: var(--font-mono); }
	.events { display: flex; flex-direction: column; gap: 0.3rem; }
	.event-row {
		display: flex; align-items: center; gap: 0.75rem;
		padding: 0.4rem 0.75rem;
		background: var(--color-surface);
		border-radius: 0.4rem;
		font-size: 0.8rem;
	}
	.event-enter { animation: event-in 0.3s ease-out; }
	@keyframes event-in {
		from { opacity: 0; transform: translateY(-8px); }
		to { opacity: 1; transform: translateY(0); }
	}
	.event-cid { color: var(--color-accent); font-size: 0.75rem; }
	.event-meta { color: var(--color-text-muted); font-size: 0.75rem; }
	.event-key { color: var(--color-text-dim); font-size: 0.7rem; margin-left: auto; }
	.grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(250px, 1fr)); gap: 1rem; margin-top: 2rem; }
	.nav-card { transition: border-color 0.15s; }
	.nav-card:hover { border-color: var(--color-accent); }
	.nav-card h3 { font-size: 1rem; margin-bottom: 0.3rem; color: var(--color-text); }
	.nav-card p { font-size: 0.85rem; color: var(--color-text-muted); }
</style>
