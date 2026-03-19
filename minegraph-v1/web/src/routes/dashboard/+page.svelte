<script lang="ts">
	import { subscribeEvents, type ServerEvent } from '$lib/api';

	interface WorkerInfo {
		worker_id: string;
		key_id: string;
		strategy: string;
		n: number;
		stats: {
			round: number;
			total_discoveries: number;
			total_submitted: number;
			total_admitted: number;
			buffered: number;
			last_round_ms: number;
			new_unique_last_round: number;
			uptime_secs: number;
		};
		last_seen: string;
		stale: boolean;
	}

	let workers = $state<WorkerInfo[]>([]);
	let loading = $state(true);
	let lastRefresh = $state('');
	let recentAdmissions = $state<ServerEvent[]>([]);
	let admitsPerMinute = $state(0);
	let admitTimestamps: number[] = [];

	async function fetchWorkers() {
		try {
			const res = await fetch('/api/workers');
			const data = await res.json();
			workers = data.workers || [];
			lastRefresh = new Date().toLocaleTimeString();
		} catch { /* server offline */ }
		loading = false;
	}

	$effect(() => {
		fetchWorkers();
		const interval = setInterval(fetchWorkers, 5000);

		const unsub = subscribeEvents((event) => {
			if (event.type === 'admission') {
				recentAdmissions = [event, ...recentAdmissions.slice(0, 49)];
				admitTimestamps.push(Date.now());
				// Trim to last 60 seconds
				const cutoff = Date.now() - 60000;
				admitTimestamps = admitTimestamps.filter(t => t > cutoff);
				admitsPerMinute = admitTimestamps.length;
			}
		});

		return () => { clearInterval(interval); unsub(); };
	});

	function formatUptime(secs: number): string {
		if (secs < 60) return `${secs}s`;
		if (secs < 3600) return `${Math.floor(secs / 60)}m ${secs % 60}s`;
		const h = Math.floor(secs / 3600);
		const m = Math.floor((secs % 3600) / 60);
		return `${h}h ${m}m`;
	}

	function formatRate(discoveries: number, secs: number): string {
		if (secs === 0) return '0';
		const rate = discoveries / secs * 3600;
		if (rate > 1000) return `${(rate / 1000).toFixed(1)}K/hr`;
		return `${rate.toFixed(0)}/hr`;
	}

	const activeWorkers = $derived(workers.filter(w => !w.stale));
	const totalDiscoveries = $derived(activeWorkers.reduce((s, w) => s + w.stats.total_discoveries, 0));
	const totalAdmitted = $derived(activeWorkers.reduce((s, w) => s + w.stats.total_admitted, 0));
	const totalSubmitted = $derived(activeWorkers.reduce((s, w) => s + w.stats.total_submitted, 0));
</script>

<h1>Worker Dashboard</h1>

<!-- Fleet summary -->
<div class="summary-grid">
	<div class="stat-card">
		<div class="stat-value">{activeWorkers.length}</div>
		<div class="stat-label">Active Workers</div>
	</div>
	<div class="stat-card">
		<div class="stat-value">{admitsPerMinute}</div>
		<div class="stat-label">Admits/min</div>
	</div>
	<div class="stat-card">
		<div class="stat-value">{totalAdmitted.toLocaleString()}</div>
		<div class="stat-label">Total Admitted</div>
	</div>
	<div class="stat-card">
		<div class="stat-value">{totalDiscoveries.toLocaleString()}</div>
		<div class="stat-label">Total Discoveries</div>
	</div>
</div>

<!-- Worker table -->
{#if loading}
	<div class="shimmer" style="height: 200px; border-radius: 0.75rem; margin-top: 1.5rem;"></div>
{:else if workers.length === 0}
	<div class="card empty-state">
		<p>No workers connected.</p>
		<p class="dim">Start a worker fleet:</p>
		<code>./scripts/fleet.sh --workers 4 --n 25 --release</code>
	</div>
{:else}
	<section class="workers-section">
		<h2>Workers <span class="dim">refreshed {lastRefresh}</span></h2>
		<div class="worker-grid">
			{#each workers as w}
				<div class="card worker-card" class:stale={w.stale}>
					<div class="worker-header">
						<span class="worker-id mono">{w.worker_id}</span>
						{#if w.stale}
							<span class="badge badge-red">offline</span>
						{:else}
							<span class="badge badge-green">active</span>
						{/if}
					</div>
					<div class="worker-meta">
						<span>n={w.n}</span>
						<span class="dim">{w.strategy}</span>
						<span class="dim">key:{w.key_id.slice(0, 8)}</span>
					</div>
					<div class="worker-stats">
						<div class="ws-row">
							<span class="ws-label">Round</span>
							<span class="ws-value mono">{w.stats.round}</span>
						</div>
						<div class="ws-row">
							<span class="ws-label">Uptime</span>
							<span class="ws-value mono">{formatUptime(w.stats.uptime_secs)}</span>
						</div>
						<div class="ws-row">
							<span class="ws-label">Discoveries</span>
							<span class="ws-value mono">{w.stats.total_discoveries.toLocaleString()}</span>
						</div>
						<div class="ws-row">
							<span class="ws-label">Admitted</span>
							<span class="ws-value mono accent">{w.stats.total_admitted}</span>
						</div>
						<div class="ws-row">
							<span class="ws-label">Submitted</span>
							<span class="ws-value mono">{w.stats.total_submitted}</span>
						</div>
						<div class="ws-row">
							<span class="ws-label">Buffered</span>
							<span class="ws-value mono">{w.stats.buffered}</span>
						</div>
						<div class="ws-row">
							<span class="ws-label">Last round</span>
							<span class="ws-value mono">{w.stats.last_round_ms}ms</span>
						</div>
						<div class="ws-row">
							<span class="ws-label">Unique/round</span>
							<span class="ws-value mono">{w.stats.new_unique_last_round.toLocaleString()}</span>
						</div>
						<div class="ws-row">
							<span class="ws-label">Disc rate</span>
							<span class="ws-value mono">{formatRate(w.stats.total_discoveries, w.stats.uptime_secs)}</span>
						</div>
					</div>
				</div>
			{/each}
		</div>
	</section>
{/if}

<!-- Recent admissions stream -->
{#if recentAdmissions.length > 0}
	<section class="admissions-section">
		<h2>Live Admissions <span class="badge badge-amber">{admitsPerMinute}/min</span></h2>
		<div class="admission-list">
			{#each recentAdmissions.slice(0, 20) as event}
				<div class="admission-row">
					<span class="badge badge-green">#{event.rank}</span>
					<a href="/submissions/{event.cid}" class="mono cid-link">{event.cid.slice(0, 16)}...</a>
					<span class="dim">n={event.n}</span>
					<span class="dim mono">{event.key_id.slice(0, 8)}</span>
				</div>
			{/each}
		</div>
	</section>
{/if}

<style>
	h1 { font-family: var(--font-mono); font-size: 1.5rem; margin-bottom: 1.5rem; }
	h2 {
		font-size: 1rem; color: var(--color-text-muted);
		font-family: var(--font-mono); margin-bottom: 0.75rem;
		display: flex; align-items: center; gap: 0.5rem;
	}
	.dim { color: var(--color-text-dim); font-size: 0.8rem; }
	.accent { color: var(--color-accent); }

	.summary-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
		gap: 0.75rem;
		margin-bottom: 2rem;
	}
	.stat-card {
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-radius: 0.75rem;
		padding: 1rem;
		text-align: center;
	}
	.stat-value {
		font-family: var(--font-mono);
		font-size: 1.8rem;
		font-weight: 700;
		color: var(--color-accent);
	}
	.stat-label { font-size: 0.75rem; color: var(--color-text-muted); margin-top: 0.25rem; }

	.empty-state { text-align: center; padding: 3rem; margin-top: 1.5rem; }
	.empty-state p { margin-bottom: 0.5rem; }
	.empty-state code {
		display: inline-block;
		background: var(--color-surface-2);
		padding: 0.5rem 1rem;
		border-radius: 0.4rem;
		font-family: var(--font-mono);
		font-size: 0.8rem;
		color: var(--color-accent);
		margin-top: 0.5rem;
	}

	.workers-section { margin-top: 1rem; }
	.worker-grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
		gap: 1rem;
	}
	.worker-card { transition: border-color 0.15s; }
	.worker-card.stale { opacity: 0.5; }
	.worker-header {
		display: flex; justify-content: space-between; align-items: center;
		margin-bottom: 0.5rem;
	}
	.worker-id { font-weight: 700; font-size: 0.9rem; }
	.worker-meta {
		display: flex; gap: 0.75rem; font-size: 0.8rem; color: var(--color-text-muted);
		margin-bottom: 0.75rem;
		padding-bottom: 0.5rem;
		border-bottom: 1px solid var(--color-border);
	}
	.worker-stats { display: flex; flex-direction: column; gap: 0.2rem; }
	.ws-row { display: flex; justify-content: space-between; font-size: 0.8rem; }
	.ws-label { color: var(--color-text-muted); }
	.ws-value { font-size: 0.8rem; }

	.admissions-section { margin-top: 2rem; }
	.admission-list { display: flex; flex-direction: column; gap: 0.25rem; }
	.admission-row {
		display: flex; align-items: center; gap: 0.75rem;
		padding: 0.35rem 0.75rem;
		background: var(--color-surface);
		border-radius: 0.4rem;
		font-size: 0.8rem;
	}
	.cid-link { color: var(--color-accent); font-size: 0.75rem; }
</style>
