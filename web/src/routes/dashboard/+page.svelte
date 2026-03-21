<script lang="ts">
	import { subscribeEvents, type ServerEvent } from '$lib/api';

	let loading = $state(true);
	let recentAdmissions = $state<ServerEvent[]>([]);
	let recentSubmissions = $state<ServerEvent[]>([]);
	let admitsPerMinute = $state(0);
	let submitsPerMinute = $state(0);
	let admitTimestamps: number[] = [];
	let submitTimestamps: number[] = [];
	let activeKeys = $state<Map<string, { lastSeen: number; n: number; submissions: number }>>(new Map());

	$effect(() => {
		loading = false;

		const unsub = subscribeEvents((event) => {
			const now = Date.now();
			const cutoff = now - 60000;

			if (event.type === 'admission') {
				recentAdmissions = [event, ...recentAdmissions.slice(0, 49)];
				admitTimestamps.push(now);
				admitTimestamps = admitTimestamps.filter(t => t > cutoff);
				admitsPerMinute = admitTimestamps.length;
			}

			if (event.type === 'admission' || event.type === 'submission') {
				submitTimestamps.push(now);
				submitTimestamps = submitTimestamps.filter(t => t > cutoff);
				submitsPerMinute = submitTimestamps.length;

				if (event.type === 'submission') {
					recentSubmissions = [event, ...recentSubmissions.slice(0, 49)];
				}

				// Track active keys (inferred worker activity)
				if (event.key_id) {
					const existing = activeKeys.get(event.key_id) || { lastSeen: 0, n: 0, submissions: 0 };
					activeKeys.set(event.key_id, {
						lastSeen: now,
						n: event.n ?? existing.n,
						submissions: existing.submissions + 1,
					});
					activeKeys = new Map(activeKeys);
				}
			}
		});

		return unsub;
	});

	const activeKeysList = $derived(
		Array.from(activeKeys.entries())
			.filter(([_, v]) => Date.now() - v.lastSeen < 120000) // active in last 2 min
			.sort((a, b) => b[1].submissions - a[1].submissions)
	);
</script>

<h1>Activity Dashboard</h1>

<p class="subtitle">Worker activity is inferred from submissions. For detailed worker monitoring, use the dedicated dashboard server.</p>

<!-- Fleet summary -->
<div class="summary-grid">
	<div class="stat-card">
		<div class="stat-value">{activeKeysList.length}</div>
		<div class="stat-label">Active Keys</div>
	</div>
	<div class="stat-card">
		<div class="stat-value">{admitsPerMinute}</div>
		<div class="stat-label">Admits/min</div>
	</div>
	<div class="stat-card">
		<div class="stat-value">{submitsPerMinute}</div>
		<div class="stat-label">Submits/min</div>
	</div>
</div>

<!-- Active keys -->
{#if activeKeysList.length > 0}
	<section class="keys-section">
		<h2>Active Submitters</h2>
		<div class="key-grid">
			{#each activeKeysList as [keyId, info]}
				<div class="card key-card">
					<div class="key-header">
						<a href="/identities/{keyId}" class="mono key-id">{keyId.slice(0, 16)}</a>
						<span class="badge badge-green">active</span>
					</div>
					<div class="key-stats">
						<span>n={info.n}</span>
						<span class="dim">{info.submissions} submissions</span>
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
					<a href="/submissions/{event.cid}" class="mono cid-link">{event.cid?.slice(0, 16)}...</a>
					<a href="/leaderboards/{event.n}" class="dim">n={event.n}</a>
					<a href="/identities/{event.key_id}" class="dim mono">{event.key_id?.slice(0, 8)}</a>
				</div>
			{/each}
		</div>
	</section>
{/if}

{#if loading}
	<div class="shimmer" style="height: 200px; border-radius: 0.75rem; margin-top: 1.5rem;"></div>
{:else if recentAdmissions.length === 0 && activeKeysList.length === 0}
	<div class="card empty-state">
		<p>No activity yet.</p>
		<p class="dim">Start submitting graphs to see live activity here.</p>
	</div>
{/if}

<style>
	h1 { font-family: var(--font-mono); font-size: 1.5rem; margin-bottom: 0.5rem; }
	h2 {
		font-size: 1rem; color: var(--color-text-muted);
		font-family: var(--font-mono); margin-bottom: 0.75rem;
		display: flex; align-items: center; gap: 0.5rem;
	}
	.subtitle { font-size: 0.75rem; color: var(--color-text-dim); margin-bottom: 1.5rem; }
	.dim { color: var(--color-text-dim); font-size: 0.8rem; }

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

	.keys-section { margin-bottom: 2rem; }
	.key-grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(250px, 1fr));
		gap: 0.75rem;
	}
	.key-card { padding: 0.75rem; }
	.key-header {
		display: flex; justify-content: space-between; align-items: center;
		margin-bottom: 0.3rem;
	}
	.key-id { font-size: 0.8rem; font-weight: 700; }
	.key-stats {
		display: flex; gap: 0.75rem; font-size: 0.8rem; color: var(--color-text-muted);
	}

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
