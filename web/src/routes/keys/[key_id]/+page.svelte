<script lang="ts">
	import { page } from '$app/state';
	import { getKeyInfo, type KeyInfo } from '$lib/api';

	let info = $state<KeyInfo | null>(null);
	let loading = $state(true);
	let error = $state('');

	$effect(() => {
		const keyId = page.params.key_id!;
		loading = true;
		error = '';
		info = null;

		let cancelled = false;

		getKeyInfo(keyId)
			.then((data) => {
				if (cancelled) return;
				info = data;
			})
			.catch((e) => {
				if (cancelled) return;
				error = e instanceof Error ? e.message : 'Failed to load key info';
			})
			.finally(() => {
				if (!cancelled) loading = false;
			});

		return () => { cancelled = true; };
	});
</script>

<svelte:head>
	<title>{info ? (info.display_name ?? info.key_id.slice(0, 8)) : 'Key'} — RamseyNet</title>
</svelte:head>

<div class="page">
	{#if loading}
		<div class="loading">Loading identity...</div>
	{:else if error}
		<div class="error">{error}</div>
	{:else if info}
		<a href="/leaderboards" class="back-link">Leaderboards</a>

		<h1 class="key-header">
			{#if info.display_name}
				{info.display_name}
			{:else}
				{info.key_id.slice(0, 8)}...
			{/if}
		</h1>

		<div class="meta">
			<div class="meta-row">
				<span class="meta-label">Key ID</span>
				<span class="meta-value mono">{info.key_id}</span>
			</div>
			{#if info.display_name}
				<div class="meta-row">
					<span class="meta-label">Display Name</span>
					<span class="meta-value">{info.display_name}</span>
				</div>
			{/if}
			<div class="meta-row">
				<span class="meta-label">Public Key</span>
				<span class="meta-value mono pubkey">{info.public_key}</span>
			</div>
			{#if info.github_repo}
				<div class="meta-row">
					<span class="meta-label">Repository</span>
					<a href="https://github.com/{info.github_repo}" class="meta-link" target="_blank" rel="noopener">
						{info.github_repo}
					</a>
				</div>
			{/if}
			<div class="meta-row">
				<span class="meta-label">Registered</span>
				<span class="meta-value">{new Date(info.created_at).toLocaleString()}</span>
			</div>
		</div>

		{#if info.leaderboard_entries.length > 0}
			<section class="entries-section">
				<h2>Leaderboard Entries ({info.leaderboard_entries.length})</h2>
				<table>
					<thead>
						<tr>
							<th>Leaderboard</th>
							<th>#</th>
							<th>CID</th>
							<th>C<sub>max</sub></th>
							<th>C<sub>min</sub></th>
							<th title="Goodman gap">Gap</th>
							<th>|Aut|</th>
							<th>Admitted</th>
						</tr>
					</thead>
					<tbody>
						{#each info.leaderboard_entries as entry (entry.graph_cid)}
							<tr>
								<td class="params">
									<a href="/leaderboards/{entry.k}/{entry.ell}/{entry.n}">
										R({entry.k},{entry.ell}) n={entry.n}
									</a>
								</td>
								<td class="rank">{entry.rank}</td>
								<td class="cid">
									<a href="/submissions/{entry.graph_cid}">{entry.graph_cid.slice(0, 16)}...</a>
								</td>
								<td class="score">{entry.tier1_max}</td>
								<td class="score">{entry.tier1_min}</td>
								<td class="score" class:gap-zero={entry.goodman_gap === 0}>{entry.goodman_gap}</td>
								<td class="score">{entry.tier2_aut}</td>
								<td class="timestamp">{new Date(entry.admitted_at).toLocaleString()}</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</section>
		{:else}
			<div class="empty">No leaderboard entries for this key.</div>
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

	.key-header {
		font-family: var(--font-mono);
		font-size: 1.5rem;
		font-weight: 700;
		margin-bottom: 1.5rem;
	}

	.meta {
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
	}

	.meta-row {
		display: flex;
		align-items: baseline;
		gap: 1rem;
	}

	.meta-label {
		font-family: var(--font-mono);
		font-size: 0.6875rem;
		color: var(--color-text-muted);
		text-transform: uppercase;
		letter-spacing: 0.05em;
		min-width: 7rem;
		flex-shrink: 0;
	}

	.meta-value {
		font-size: 0.875rem;
	}

	.meta-value.mono {
		font-family: var(--font-mono);
		font-size: 0.8125rem;
	}

	.pubkey {
		word-break: break-all;
		line-height: 1.4;
	}

	.meta-link {
		font-family: var(--font-mono);
		font-size: 0.8125rem;
		color: var(--color-accent);
		text-decoration: none;
	}

	.meta-link:hover {
		text-decoration: underline;
	}

	/* ── Entries table ──────────────────────────────────── */

	.entries-section {
		margin-top: 2rem;
		padding-top: 2rem;
		border-top: 1px solid var(--color-border);
	}

	.entries-section h2 {
		font-family: var(--font-mono);
		font-size: 1.125rem;
		font-weight: 600;
		margin-bottom: 1rem;
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

	.params {
		font-family: var(--font-mono);
		font-size: 0.8125rem;
	}

	.params a {
		color: var(--color-accent);
		text-decoration: none;
	}

	.params a:hover {
		text-decoration: underline;
	}

	.rank {
		font-family: var(--font-mono);
		font-weight: 700;
		color: var(--color-text-muted);
		width: 2rem;
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
		margin-top: 2rem;
		padding: 2rem;
		text-align: center;
		color: var(--color-text-muted);
		font-size: 0.875rem;
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-radius: 0.75rem;
	}
</style>
