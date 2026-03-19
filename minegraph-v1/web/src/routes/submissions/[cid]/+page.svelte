<script lang="ts">
	import { page } from '$app/stores';
	import GemView from '$lib/components/GemView.svelte';
	import { getSubmission, type SubmissionDetail } from '$lib/api';

	const cid = $derived($page.params.cid);
	let detail = $state<SubmissionDetail | null>(null);
	let loading = $state(true);
	let error = $state('');
	let metadataOpen = $state(false);

	$effect(() => {
		loading = true;
		error = '';
		getSubmission(cid).then(d => { detail = d; loading = false; })
			.catch(e => { error = e.message; loading = false; });
	});

	function copyToClipboard(text: string) {
		navigator.clipboard.writeText(text);
	}
</script>

<h1>Submission</h1>

{#if loading}
	<div class="shimmer" style="height: 200px; border-radius: 0.75rem;"></div>
{:else if error}
	<p class="error">{error}</p>
{:else if detail}
	<div class="detail-grid">
		<!-- Gem visualization -->
		{#if detail.graph}
			<div class="viz-col">
				<GemView
					graph6={detail.graph.graph6}
					n={detail.graph.n}
					size={320}
					cid={detail.submission.cid}
					goodmanGap={detail.score?.goodman_gap ?? 0}
					autOrder={detail.score?.aut_order ?? 1}
					histogram={detail.score?.histogram?.tiers ?? []}
					label={detail.submission.cid.slice(0, 24)}
				/>
			</div>
		{/if}

		<!-- Info panel -->
		<div class="info-col">
			<div class="card">
				<h3>Graph</h3>
				<dl>
					<dt>CID</dt>
					<dd class="mono">{detail.submission.cid}</dd>
					{#if detail.graph}
						<dt>Vertices</dt>
						<dd>{detail.graph.n}</dd>
						<dt>graph6</dt>
						<dd class="mono" style="word-break: break-all; font-size: 0.7rem;">{detail.graph.graph6}</dd>
					{/if}
				</dl>
			</div>

			{#if detail.score}
				<div class="card">
					<h3>Score</h3>
					<dl>
						<dt>Goodman Gap</dt>
						<dd>{detail.score.goodman_gap}</dd>
						<dt>|Aut(G)|</dt>
						<dd>{detail.score.aut_order}</dd>
						{#if detail.score.histogram?.tiers}
							<dt>Histogram</dt>
							<dd>
								{#each detail.score.histogram.tiers as tier}
									<div class="tier mono">k={tier.k}: red={tier.red} blue={tier.blue}</div>
								{/each}
							</dd>
						{/if}
					</dl>
				</div>
			{/if}

			<div class="card">
				<h3>Identity</h3>
				<dl>
					<dt>Key ID</dt>
					<dd class="mono"><a href="/identities/{detail.submission.key_id}">{detail.submission.key_id}</a></dd>
					<dt>Submitted</dt>
					<dd>{new Date(detail.submission.created_at).toLocaleString()}</dd>
				</dl>
			</div>

			{#if detail.receipt}
				<div class="card">
					<h3>Receipt</h3>
					<dl>
						<dt>Verdict</dt>
						<dd>
							<span class="badge badge-green">{detail.receipt.verdict}</span>
						</dd>
						<dt>Server Key</dt>
						<dd class="mono dim">{detail.receipt.server_key_id.slice(0, 16)}</dd>
					</dl>
				</div>
			{/if}

			{#if detail.submission.metadata}
				<div class="card">
					<button class="meta-toggle" onclick={() => { metadataOpen = !metadataOpen; }}>
						<span class="caret" class:open={metadataOpen}></span>
						<h3>Metadata</h3>
					</button>
					{#if metadataOpen}
						<div class="meta-body">
							{#each Object.entries(detail.submission.metadata) as [key, val]}
								<div class="meta-row">
									<span class="meta-key">{key}</span>
									<span class="meta-val mono">{typeof val === 'object' ? JSON.stringify(val) : val}</span>
								</div>
							{/each}
						</div>
					{/if}
				</div>
			{/if}

			<!-- Actions -->
			{#if detail.graph}
				<div class="actions">
					<button class="action-btn" onclick={() => copyToClipboard(detail!.graph!.graph6)}>Copy graph6</button>
					<button class="action-btn" onclick={() => copyToClipboard(detail!.submission.cid)}>Copy CID</button>
				</div>
			{/if}
		</div>
	</div>
{/if}

<style>
	h1 { font-family: var(--font-mono); font-size: 1.5rem; margin-bottom: 1.5rem; }
	.error { color: var(--color-red); }
	.detail-grid {
		display: grid;
		grid-template-columns: auto 1fr;
		gap: 2rem;
		align-items: start;
	}
	@media (max-width: 768px) {
		.detail-grid { grid-template-columns: 1fr; }
		.viz-col { display: flex; justify-content: center; }
	}
	.info-col { display: flex; flex-direction: column; gap: 1rem; }
	.card h3 {
		font-size: 0.8rem;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		color: var(--color-text-muted);
		margin-bottom: 0.75rem;
	}
	dl { display: grid; grid-template-columns: auto 1fr; gap: 0.3rem 1rem; }
	dt { font-size: 0.8rem; color: var(--color-text-muted); }
	dd { font-size: 0.85rem; }
	.tier { font-size: 0.75rem; color: var(--color-text-muted); }
	.dim { color: var(--color-text-muted); font-size: 0.8rem; }

	/* Metadata collapsible */
	.meta-toggle {
		display: flex; align-items: center; gap: 0.4rem;
		background: none; border: none; cursor: pointer; padding: 0; width: 100%;
	}
	.meta-toggle h3 { margin-bottom: 0; }
	.caret {
		display: inline-block; width: 0; height: 0;
		border-left: 5px solid var(--color-text-muted);
		border-top: 4px solid transparent;
		border-bottom: 4px solid transparent;
		transition: transform 0.2s;
	}
	.caret.open { transform: rotate(90deg); }
	.meta-body {
		margin-top: 0.5rem;
		display: flex; flex-direction: column; gap: 0.25rem;
	}
	.meta-row {
		display: flex; justify-content: space-between; align-items: baseline;
		padding: 0.2rem 0.5rem;
		background: var(--color-bg); border-radius: 0.25rem;
		font-size: 0.8rem;
	}
	.meta-key { color: var(--color-text-muted); font-size: 0.75rem; }
	.meta-val { font-size: 0.75rem; color: var(--color-text); }

	/* Actions */
	.actions { display: flex; gap: 0.4rem; flex-wrap: wrap; }
	.action-btn {
		font-size: 0.7rem; font-family: var(--font-mono);
		padding: 0.3rem 0.6rem; border-radius: 0.3rem;
		background: var(--color-surface); border: 1px solid var(--color-border);
		color: var(--color-text-muted); cursor: pointer;
		transition: border-color 0.15s, color 0.15s;
	}
	.action-btn:hover { border-color: var(--color-accent); color: var(--color-accent); }
</style>
