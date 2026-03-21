<script lang="ts">
	import { page } from '$app/stores';
	import GemView from '$lib/components/GemView.svelte';
	import { getIdentity, getIdentitySubmissions } from '$lib/api';

	const keyId = $derived($page.params.key_id);

	interface LeaderboardEntry {
		n: number;
		rank: number;
		cid: string;
		graph6: string;
		goodman_gap: number | null;
		aut_order: number | null;
	}

	interface LeaderboardSummary {
		total_entries: number;
		n_values: number[];
		best_rank: number | null;
		avg_rank: number | null;
	}

	interface IdentityData {
		key_id: string;
		public_key: string;
		display_name: string | null;
		github_repo: string | null;
		created_at: string;
		total_submissions: number;
		leaderboard_entries: LeaderboardEntry[];
		leaderboard_summary: LeaderboardSummary;
	}

	let identity = $state<IdentityData | null>(null);
	let submissions = $state<any[]>([]);
	let loading = $state(true);
	let error = $state('');

	$effect(() => {
		loading = true; error = '';
		Promise.all([
			getIdentity(keyId),
			getIdentitySubmissions(keyId, 50),
		]).then(([id, subs]) => {
			identity = id as unknown as IdentityData;
			submissions = subs.submissions || [];
			loading = false;
		}).catch(e => { error = e.message; loading = false; });
	});

	function copyToClipboard(text: string) {
		navigator.clipboard.writeText(text);
	}
</script>

<h1>Identity</h1>

{#if loading}
	<div class="shimmer" style="height: 150px; border-radius: 0.5rem;"></div>
{:else if error}
	<p class="error">{error}</p>
{:else if identity}
	<div class="card id-card">
		<dl>
			<dt>Key ID</dt>
			<dd class="mono">{identity.key_id}
				<button class="copy-btn" onclick={() => copyToClipboard(identity!.key_id)}>Copy</button>
			</dd>
			<dt>Public Key</dt>
			<dd class="mono pk">{identity.public_key}
				<button class="copy-btn" onclick={() => copyToClipboard(identity!.public_key)}>Copy</button>
			</dd>
			{#if identity.display_name}
				<dt>Name</dt><dd>{identity.display_name}</dd>
			{/if}
			{#if identity.github_repo}
				<dt>Repo</dt><dd><a href={identity.github_repo} target="_blank">{identity.github_repo}</a></dd>
			{/if}
			<dt>Registered</dt><dd class="dm">{new Date(identity.created_at).toLocaleString()}</dd>
			<dt>Total Submissions</dt><dd>{identity.total_submissions.toLocaleString()}</dd>
			<dt>On Leaderboards</dt><dd>{identity.leaderboard_summary.total_entries} entries across {new Set(identity.leaderboard_summary.n_values).size} n-values</dd>
		</dl>
	</div>

	<!-- Leaderboard summary + top entries -->
	{#if identity.leaderboard_summary.total_entries > 0}
		<section class="lb-section">
			<h2>Leaderboard Summary</h2>
			<div class="lb-summary card">
				<div class="summary-stats">
					<div class="stat">
						<span class="stat-value">{identity.leaderboard_summary.total_entries}</span>
						<span class="stat-label">entries</span>
					</div>
					<div class="stat">
						<span class="stat-value">{new Set(identity.leaderboard_summary.n_values).size}</span>
						<span class="stat-label">n-values</span>
					</div>
					{#if identity.leaderboard_summary.best_rank !== null}
						<div class="stat">
							<span class="stat-value">#{identity.leaderboard_summary.best_rank}</span>
							<span class="stat-label">best rank</span>
						</div>
					{/if}
					{#if identity.leaderboard_summary.avg_rank !== null}
						<div class="stat">
							<span class="stat-value">#{identity.leaderboard_summary.avg_rank.toFixed(1)}</span>
							<span class="stat-label">avg rank</span>
						</div>
					{/if}
				</div>
			</div>

			<h2>Top Entries{#if identity.leaderboard_summary.total_entries > identity.leaderboard_entries.length} (showing {identity.leaderboard_entries.length} of {identity.leaderboard_summary.total_entries}){/if}</h2>
			<div class="lb-grid">
				{#each identity.leaderboard_entries as entry}
					<a href="/submissions/{entry.cid}" class="lb-entry card">
						<div class="lb-header">
							<span class="lb-rank">#{entry.rank}</span>
							<span class="lb-n">n={entry.n}</span>
						</div>
						<div class="lb-gem">
							<GemView
								graph6={entry.graph6}
								n={entry.n}
								size={80}
								cid={entry.cid}
								goodmanGap={entry.goodman_gap ?? 0}
								autOrder={entry.aut_order ?? 1}
							/>
						</div>
						<div class="lb-scores">
							{#if entry.goodman_gap !== null}<span>gap: {entry.goodman_gap}</span>{/if}
							{#if entry.aut_order !== null}<span>|Aut|: {entry.aut_order}</span>{/if}
						</div>
						<div class="lb-cid mono">{entry.cid.slice(0, 16)}</div>
					</a>
				{/each}
			</div>
		</section>
	{/if}

	<!-- Recent submissions -->
	{#if submissions.length > 0}
		<section class="subs">
			<h2>Recent Submissions</h2>
			<table>
				<thead><tr><th>CID</th><th>When</th></tr></thead>
				<tbody>
					{#each submissions as sub}
						<tr>
							<td><a href="/submissions/{sub.cid}" class="mono">{sub.cid.slice(0, 24)}...</a></td>
							<td class="dm">{new Date(sub.created_at).toLocaleString()}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</section>
	{/if}
{/if}

<style>
	h1 { font-family: var(--font-mono); font-size: 1.3rem; margin-bottom: 1rem; }
	h2 { font-family: var(--font-mono); font-size: 0.9rem; color: var(--color-text-muted); margin-bottom: 0.75rem; }
	.error { color: var(--color-red); }
	.id-card { margin-bottom: 1.5rem; }
	dl { display: grid; grid-template-columns: auto 1fr; gap: 0.3rem 1rem; }
	dt { font-size: 0.75rem; color: var(--color-text-muted); }
	dd { font-size: 0.8rem; display: flex; align-items: center; gap: 0.5rem; }
	.pk { font-size: 0.6rem; word-break: break-all; }
	.dm { color: var(--color-text-muted); font-size: 0.8rem; }
	.copy-btn {
		font-size: 0.6rem; padding: 0.1rem 0.3rem; border-radius: 0.2rem;
		background: var(--color-bg); border: 1px solid var(--color-border);
		color: var(--color-text-dim); cursor: pointer; font-family: var(--font-mono);
	}
	.copy-btn:hover { border-color: var(--color-accent); color: var(--color-accent); }

	.lb-summary { margin-bottom: 1rem; }
	.summary-stats { display: flex; gap: 1.5rem; justify-content: center; padding: 0.5rem 0; }
	.stat { display: flex; flex-direction: column; align-items: center; gap: 0.1rem; }
	.stat-value { font-family: var(--font-mono); font-weight: 700; font-size: 1.1rem; color: var(--color-accent); }
	.stat-label { font-size: 0.65rem; color: var(--color-text-muted); text-transform: uppercase; }

	.lb-section { margin-bottom: 2rem; }
	.lb-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 0.75rem; }
	.lb-entry {
		text-decoration: none; color: inherit;
		transition: border-color 0.15s, transform 0.1s;
		display: flex; flex-direction: column; align-items: center; gap: 0.25rem;
		padding: 0.6rem;
	}
	.lb-entry:hover { border-color: var(--color-accent); transform: scale(1.02); }
	.lb-header { display: flex; justify-content: space-between; width: 100%; align-items: baseline; }
	.lb-rank { font-family: var(--font-mono); font-weight: 700; color: var(--color-accent); font-size: 0.9rem; }
	.lb-n { font-size: 0.7rem; color: var(--color-text-muted); font-family: var(--font-mono); }
	.lb-gem { margin: 0.2rem 0; }
	.lb-scores { font-size: 0.6rem; color: var(--color-text-muted); font-family: var(--font-mono); display: flex; gap: 0.5rem; }
	.lb-cid { font-size: 0.5rem; color: var(--color-text-dim); }

	.subs { margin-top: 1.5rem; }
	table { width: 100%; border-collapse: collapse; font-size: 0.8rem; }
	th { text-align: left; padding: 0.3rem 0.5rem; border-bottom: 1px solid var(--color-border); color: var(--color-text-muted); font-size: 0.7rem; text-transform: uppercase; }
	td { padding: 0.3rem 0.5rem; border-bottom: 1px solid rgba(42,42,58,0.2); }
</style>
