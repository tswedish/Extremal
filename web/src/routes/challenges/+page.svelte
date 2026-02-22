<script lang="ts">
	import { getChallenges, getRecords, type Challenge, type Record } from '$lib/api';

	let challenges = $state<Challenge[]>([]);
	let records = $state<Record[]>([]);
	let loading = $state(true);
	let error = $state('');

	const recordMap = $derived(
		new Map(records.map((r) => [r.challenge_id, r]))
	);

	$effect(() => {
		let cancelled = false;
		Promise.all([getChallenges(), getRecords()])
			.then(([c, r]) => {
				if (cancelled) return;
				challenges = c;
				records = r;
			})
			.catch((e) => {
				if (cancelled) return;
				error = e instanceof Error ? e.message : 'Failed to load';
			})
			.finally(() => {
				if (!cancelled) loading = false;
			});
		return () => { cancelled = true; };
	});
</script>

<svelte:head>
	<title>Challenges — RamseyNet</title>
</svelte:head>

<div class="page">
	<h1>Challenges</h1>
	<p class="subtitle">Active Ramsey number challenges and their best-known bounds.</p>

	{#if loading}
		<div class="loading">Loading challenges...</div>
	{:else if error}
		<div class="error">{error}</div>
	{:else if challenges.length === 0}
		<div class="empty">No challenges yet. Create one via the API.</div>
	{:else}
		<div class="challenge-grid">
			{#each challenges as c (c.challenge_id)}
				{@const record = recordMap.get(c.challenge_id)}
				<a href="/challenges/{encodeURIComponent(c.challenge_id)}" class="challenge-card">
					<div class="card-header">
						<span class="ramsey-label">R({c.k},{c.ell})</span>
						{#if record}
							<span class="best-n">n = {record.best_n}</span>
						{:else}
							<span class="no-record">no submissions</span>
						{/if}
					</div>
					<div class="card-id">{c.challenge_id}</div>
					{#if c.description}
						<p class="card-desc">{c.description}</p>
					{/if}
					{#if record}
						<div class="card-cid" title={record.best_cid}>
							CID: {record.best_cid.slice(0, 16)}...
						</div>
					{/if}
				</a>
			{/each}
		</div>
	{/if}
</div>

<style>
	.page {
		max-width: 800px;
	}

	h1 {
		font-family: var(--font-mono);
		font-size: 1.75rem;
		font-weight: 700;
		margin-bottom: 0.5rem;
	}

	.subtitle {
		color: var(--color-text-muted);
		font-size: 0.875rem;
		margin-bottom: 2rem;
	}

	.loading, .error, .empty {
		padding: 2rem;
		text-align: center;
		color: var(--color-text-muted);
		font-size: 0.875rem;
	}

	.error {
		color: var(--color-rejected);
	}

	.challenge-grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(340px, 1fr));
		gap: 1rem;
	}

	.challenge-card {
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-radius: 0.75rem;
		padding: 1.25rem;
		display: block;
		color: var(--color-text);
		transition: border-color 0.15s;
	}

	.challenge-card:hover {
		border-color: var(--color-accent);
	}

	.card-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		margin-bottom: 0.5rem;
	}

	.ramsey-label {
		font-family: var(--font-mono);
		font-size: 1.25rem;
		font-weight: 700;
		color: var(--color-accent);
	}

	.best-n {
		font-family: var(--font-mono);
		font-size: 1rem;
		font-weight: 600;
		color: var(--color-accepted);
	}

	.no-record {
		font-size: 0.75rem;
		color: var(--color-text-muted);
	}

	.card-id {
		font-family: var(--font-mono);
		font-size: 0.75rem;
		color: var(--color-text-muted);
		margin-bottom: 0.5rem;
	}

	.card-desc {
		font-size: 0.8125rem;
		color: var(--color-text-muted);
		margin-bottom: 0.5rem;
	}

	.card-cid {
		font-family: var(--font-mono);
		font-size: 0.6875rem;
		color: var(--color-text-muted);
		opacity: 0.7;
	}
</style>
