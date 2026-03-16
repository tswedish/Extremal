<script lang="ts">
	import { page } from '$app/state';
	import { getSubmission, type SubmissionDetail, type RgxfJson } from '$lib/api';
	import MatrixView from '$lib/components/MatrixView.svelte';
	import CircleLayout from '$lib/components/CircleLayout.svelte';
	import GemView from '$lib/components/GemView.svelte';

	let detail = $state<SubmissionDetail | null>(null);
	let loading = $state(true);
	let error = $state('');
	let showRgxf = $state(false);
	let includeMetadata = $state(false);
	let copied = $state(false);

	$effect(() => {
		const cid = page.params.cid!;
		loading = true;
		error = '';
		detail = null;

		let cancelled = false;

		getSubmission(cid)
			.then((data) => {
				if (cancelled) return;
				detail = data;
			})
			.catch((e) => {
				if (cancelled) return;
				error = e instanceof Error ? e.message : 'Failed to load submission';
			})
			.finally(() => {
				if (!cancelled) loading = false;
			});

		return () => { cancelled = true; };
	});

	function rgxfString(rgxf: RgxfJson): string {
		return JSON.stringify(rgxf, null, 2);
	}

	/** Compute Goodman's minimum for n vertices.
	 *  g(n) = C(n,3) - floor(n * floor((n-1)^2 / 4) / 2)
	 *  NOTE: This must match goodman_minimum() in ramseynet-verifier/src/scoring.rs.
	 *  The server also includes goodman_minimum in score_json for verification. */
	function goodmanMinimum(n: number): number {
		if (n < 3) return 0;
		const c_n_3 = n * (n - 1) * (n - 2) / 6;
		const floorTerm = Math.floor(n * Math.floor((n - 1) * (n - 1) / 4) / 2);
		return c_n_3 - floorTerm;
	}

	function csvLine(): string {
		if (!detail || !detail.rgxf) return '';
		const score = detail.score as Record<string, unknown> | null;
		return [
			detail.graph_cid,
			detail.k,
			detail.ell,
			detail.n,
			detail.verdict ?? '',
			detail.reason ?? '',
			detail.leaderboard_rank ?? '',
			score?.c_omega ?? '',
			score?.c_alpha ?? '',
			score?.goodman ?? '',
			score?.goodman_gap ?? '',
			score?.aut_order ?? '',
			detail.submitted_at,
			detail.rgxf.encoding,
			detail.rgxf.bits_b64,
		].join(',');
	}

	async function copyToClipboard() {
		if (!detail?.rgxf) return;
		const text = includeMetadata
			? 'graph_cid,k,ell,n,verdict,reason,rank,c_omega,c_alpha,goodman,goodman_gap,aut_order,submitted_at,encoding,bits_b64\n' + csvLine()
			: rgxfString(detail.rgxf);
		await navigator.clipboard.writeText(text);
		copied = true;
		setTimeout(() => { copied = false; }, 1500);
	}
</script>

<svelte:head>
	<title>Submission {detail?.graph_cid?.slice(0, 12) ?? ''} — RamseyNet</title>
</svelte:head>

<div class="page">
	{#if loading}
		<div class="loading">Loading submission...</div>
	{:else if error}
		<div class="error">{error}</div>
	{:else if detail}
		<button class="back-link" onclick={() => history.back()}>Back</button>

		<h1 class="cid-header">{detail.graph_cid}</h1>

		<div class="meta">
			<div class="meta-row">
				<span class="meta-label">Ramsey Params</span>
				<a href="/leaderboards/{detail.k}/{detail.ell}/{detail.n}" class="meta-link">
					R({detail.k},{detail.ell}) n={detail.n}
				</a>
			</div>
			<div class="meta-row">
				<span class="meta-label">Graph Size</span>
				<span class="meta-value">n = {detail.n}</span>
			</div>
			<div class="meta-row">
				<span class="meta-label">Verdict</span>
				<span class="verdict-badge" class:accepted={detail.verdict === 'accepted'} class:rejected={detail.verdict === 'rejected'}>
					{detail.verdict ?? 'pending'}
				</span>
				{#if detail.leaderboard_rank}
					<span class="rank-badge">Rank #{detail.leaderboard_rank}</span>
				{/if}
			</div>
			{#if detail.reason}
				<div class="meta-row">
					<span class="meta-label">Reason</span>
					<span class="meta-value">{detail.reason}</span>
				</div>
			{/if}
			{#if detail.witness && detail.witness.length > 0}
				<div class="meta-row">
					<span class="meta-label">Witness</span>
					<span class="meta-value mono">[{detail.witness.join(', ')}]</span>
				</div>
			{/if}
			<div class="meta-row">
				<span class="meta-label">Submitter</span>
				<span class="meta-value mono">
					{#if detail.key_id}
						<a href="/keys/{detail.key_id}" class="key-link" title={detail.key_id}>
							{detail.key_id.slice(0, 8)}...
						</a>
						{#if detail.sig_status === 'verified'}
							<span class="sig-badge sig-verified" title="Signature verified">verified</span>
						{:else if detail.sig_status === 'unregistered'}
							<span class="sig-badge sig-unregistered" title="Key not registered on server">unregistered</span>
						{:else if detail.sig_status === 'invalid'}
							<span class="sig-badge sig-invalid" title="Invalid signature">invalid</span>
						{/if}
						{#if detail.commit_hash}
							<span style="color: var(--color-text-muted); font-size: 0.75rem"> @ {detail.commit_hash.slice(0, 7)}</span>
						{/if}
					{:else}
						<span style="color: var(--color-text-muted); font-style: italic">anonymous</span>
					{/if}
				</span>
			</div>
			<div class="meta-row">
				<span class="meta-label">Submitted</span>
				<span class="meta-value">{new Date(detail.submitted_at).toLocaleString()}</span>
			</div>
			{#if detail.verified_at}
				<div class="meta-row">
					<span class="meta-label">Verified</span>
					<span class="meta-value">{new Date(detail.verified_at).toLocaleString()}</span>
				</div>
			{/if}
		</div>

		{#if detail.score}
			{@const score = detail.score as Record<string, unknown>}
			<section class="score-section">
				<h2>Score Details</h2>
				<div class="score-grid">
					<div class="score-item">
						<span class="score-label">&omega; (clique #)</span>
						<span class="score-val">{score.omega ?? '?'}</span>
					</div>
					<div class="score-item">
						<span class="score-label">&alpha; (indep #)</span>
						<span class="score-val">{score.alpha ?? '?'}</span>
					</div>
					<div class="score-item">
						<span class="score-label">C<sub>&omega;</sub> (max cliques)</span>
						<span class="score-val">{score.c_omega ?? '?'}</span>
					</div>
					<div class="score-item">
						<span class="score-label">C<sub>&alpha;</sub> (max indep)</span>
						<span class="score-val">{score.c_alpha ?? '?'}</span>
					</div>
					<div class="score-item">
						<span class="score-label">Triangles (G)</span>
						<span class="score-val">{score.triangles ?? '?'}</span>
					</div>
					<div class="score-item">
						<span class="score-label">Triangles (complement)</span>
						<span class="score-val">{score.triangles_complement ?? '?'}</span>
					</div>
					<div class="score-item">
						<span class="score-label">Goodman #</span>
						<span class="score-val">{score.goodman ?? '?'}</span>
					</div>
					<div class="score-item">
						<span class="score-label">Goodman minimum</span>
						<span class="score-val">{score.goodman_min ?? goodmanMinimum(detail.n)}</span>
					</div>
					<div class="score-item">
						<span class="score-label">Goodman gap</span>
						<span class="score-val" class:gap-optimal={score.goodman_gap === 0}>{score.goodman_gap ?? '?'}</span>
					</div>
					<div class="score-item">
						<span class="score-label">|Aut(G)|</span>
						<span class="score-val">{score.aut_order ?? '?'}</span>
					</div>
				</div>
			</section>
		{/if}

		{#if detail.rgxf}
			<section class="rgxf-section">
				<div class="rgxf-header">
					<button class="toggle-btn" onclick={() => showRgxf = !showRgxf}>
						{showRgxf ? 'Hide' : 'Show'} RGXF
					</button>
					{#if showRgxf}
						<label class="meta-checkbox">
							<input type="checkbox" bind:checked={includeMetadata} />
							Include metadata
						</label>
						<button class="copy-btn" onclick={copyToClipboard} aria-label="Copy to clipboard">
							{#if copied}
								<svg width="16" height="16" viewBox="0 0 16 16" fill="none"><path d="M3 8.5l3 3 7-7" stroke="var(--color-accepted)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>
							{:else}
								<svg width="16" height="16" viewBox="0 0 16 16" fill="none"><rect x="5" y="2" width="8" height="10" rx="1" stroke="currentColor" stroke-width="1.5"/><rect x="3" y="4" width="8" height="10" rx="1" stroke="currentColor" stroke-width="1.5" fill="var(--color-surface)"/></svg>
							{/if}
						</button>
					{/if}
				</div>
				{#if showRgxf}
					<pre class="rgxf-code">{rgxfString(detail.rgxf)}</pre>
				{/if}
			</section>

			<section class="viz-section">
				<h2>Graph Visualization</h2>
				<div class="viz-row">
					<GemView rgxf={detail.rgxf} size={360} label="MineGraph Gem" />
					<MatrixView rgxf={detail.rgxf} witness={detail.witness ?? []} size={360} />
					<CircleLayout rgxf={detail.rgxf} witness={detail.witness ?? []} size={360} />
				</div>
			</section>
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
		cursor: pointer;
		background: none;
		border: none;
		padding: 0;
		font: inherit;
	}

	.back-link::before {
		content: '\2190 ';
	}

	.back-link:hover {
		color: var(--color-accent);
	}

	.cid-header {
		font-family: var(--font-mono);
		font-size: 1rem;
		font-weight: 700;
		word-break: break-all;
		line-height: 1.4;
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

	.key-link {
		color: var(--color-accent);
		text-decoration: none;
	}

	.key-link:hover {
		text-decoration: underline;
	}

	.sig-badge {
		font-family: var(--font-mono);
		font-size: 0.625rem;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.04em;
		padding: 0.1rem 0.4rem;
		border-radius: 0.25rem;
		margin-left: 0.375rem;
		vertical-align: middle;
	}

	.sig-verified {
		color: var(--color-accepted);
		background: color-mix(in srgb, var(--color-accepted) 15%, transparent);
	}

	.sig-unregistered {
		color: #f59e0b;
		background: color-mix(in srgb, #f59e0b 15%, transparent);
	}

	.sig-invalid {
		color: var(--color-rejected);
		background: color-mix(in srgb, var(--color-rejected) 15%, transparent);
	}

	.meta-link {
		font-family: var(--font-mono);
		font-size: 0.8125rem;
	}

	.verdict-badge {
		font-family: var(--font-mono);
		font-size: 0.75rem;
		font-weight: 700;
		text-transform: uppercase;
		padding: 0.2rem 0.6rem;
		border-radius: 0.25rem;
		letter-spacing: 0.05em;
	}

	.verdict-badge.accepted {
		color: var(--color-accepted);
		background: color-mix(in srgb, var(--color-accepted) 15%, transparent);
	}

	.rank-badge {
		font-family: var(--font-mono);
		font-size: 0.75rem;
		font-weight: 700;
		text-transform: uppercase;
		padding: 0.2rem 0.6rem;
		border-radius: 0.25rem;
		letter-spacing: 0.05em;
		color: #f59e0b;
		background: color-mix(in srgb, #f59e0b 15%, transparent);
	}

	.verdict-badge.rejected {
		color: var(--color-rejected);
		background: color-mix(in srgb, var(--color-rejected) 15%, transparent);
	}

	/* ── Score section ───────────────────────────────────────── */

	.score-section {
		margin-top: 1.5rem;
		padding-top: 1.5rem;
		border-top: 1px solid var(--color-border);
	}

	.score-section h2 {
		font-family: var(--font-mono);
		font-size: 1rem;
		font-weight: 600;
		margin-bottom: 0.75rem;
	}

	.score-grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
		gap: 0.5rem;
	}

	.score-item {
		display: flex;
		justify-content: space-between;
		align-items: center;
		padding: 0.375rem 0.625rem;
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-radius: 0.375rem;
	}

	.score-label {
		font-size: 0.75rem;
		color: var(--color-text-muted);
	}

	.score-val {
		font-family: var(--font-mono);
		font-size: 0.8125rem;
		font-weight: 600;
	}

	.score-val.gap-optimal {
		color: var(--color-accepted);
	}

	/* ── RGXF section ────────────────────────────────────────── */

	.rgxf-section {
		margin-top: 1.5rem;
		padding-top: 1.5rem;
		border-top: 1px solid var(--color-border);
	}

	.rgxf-header {
		display: flex;
		align-items: center;
		gap: 0.75rem;
	}

	.toggle-btn {
		font-family: var(--font-mono);
		font-size: 0.75rem;
		color: var(--color-text-muted);
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-radius: 0.375rem;
		padding: 0.3rem 0.75rem;
		cursor: pointer;
		transition: border-color 0.2s, color 0.2s;
	}

	.toggle-btn:hover {
		color: var(--color-accent);
		border-color: var(--color-accent);
	}

	.meta-checkbox {
		font-family: var(--font-mono);
		font-size: 0.6875rem;
		color: var(--color-text-muted);
		display: flex;
		align-items: center;
		gap: 0.375rem;
		cursor: pointer;
	}

	.meta-checkbox input {
		accent-color: var(--color-accent);
	}

	.copy-btn {
		background: none;
		border: 1px solid var(--color-border);
		border-radius: 0.375rem;
		padding: 0.25rem 0.375rem;
		cursor: pointer;
		color: var(--color-text-muted);
		display: flex;
		align-items: center;
		transition: border-color 0.2s, color 0.2s;
	}

	.copy-btn:hover {
		color: var(--color-accent);
		border-color: var(--color-accent);
	}

	.rgxf-code {
		font-family: var(--font-mono);
		font-size: 0.75rem;
		color: var(--color-text);
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-radius: 0.5rem;
		padding: 0.75rem 1rem;
		margin-top: 0.75rem;
		overflow-x: auto;
		white-space: pre;
		line-height: 1.5;
	}

	/* ── Viz section ─────────────────────────────────────────── */

	.viz-section {
		margin-top: 2rem;
		padding-top: 2rem;
		border-top: 1px solid var(--color-border);
	}

	h2 {
		font-family: var(--font-mono);
		font-size: 1.125rem;
		font-weight: 600;
		margin-bottom: 1rem;
	}

	.viz-row {
		display: flex;
		flex-wrap: wrap;
		gap: 1.5rem;
	}
</style>
