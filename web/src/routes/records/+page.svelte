<script lang="ts">
	import { getRecords, type Record } from '$lib/api';

	let records = $state<Record[]>([]);
	let loading = $state(true);
	let error = $state('');

	$effect(() => {
		let cancelled = false;
		getRecords()
			.then((r) => {
				if (cancelled) return;
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
	<title>Records — RamseyNet</title>
</svelte:head>

<div class="page">
	<h1>Records</h1>
	<p class="subtitle">Best-known Ramsey number lower bounds across all challenges.</p>

	{#if loading}
		<div class="loading">Loading records...</div>
	{:else if error}
		<div class="error">{error}</div>
	{:else if records.length === 0}
		<div class="empty">No records yet. Submit a graph to set the first record.</div>
	{:else}
		<div class="records-table">
			<table>
				<thead>
					<tr>
						<th>Challenge</th>
						<th>Best n</th>
						<th>CID</th>
						<th>Updated</th>
					</tr>
				</thead>
				<tbody>
					{#each records as r (r.challenge_id)}
						<tr>
							<td>
								<a href="/challenges/{encodeURIComponent(r.challenge_id)}" class="challenge-link">
									{r.challenge_id}
								</a>
							</td>
							<td class="best-n">{r.best_n}</td>
							<td class="cid" title={r.best_cid}>{r.best_cid.slice(0, 16)}...</td>
							<td class="timestamp">{new Date(r.updated_at).toLocaleDateString()}</td>
						</tr>
					{/each}
				</tbody>
			</table>
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
		padding: 0.75rem;
		font-size: 0.875rem;
		border-bottom: 1px solid var(--color-border);
	}

	.challenge-link {
		font-family: var(--font-mono);
		font-size: 0.8125rem;
	}

	.best-n {
		font-family: var(--font-mono);
		font-weight: 600;
		color: var(--color-accepted);
	}

	.cid {
		font-family: var(--font-mono);
		font-size: 0.75rem;
		color: var(--color-text-muted);
	}

	.timestamp {
		font-size: 0.8125rem;
		color: var(--color-text-muted);
	}
</style>
