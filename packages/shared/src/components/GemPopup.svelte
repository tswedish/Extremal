<script lang="ts">
	import GemView from './GemView.svelte';

	let {
		graph6,
		n,
		cid = '',
		goodmanGap = 0,
		autOrder = 1,
		scoreHex = '',
		histogram = [] as { k: number; red: number; blue: number }[],
		workerId = '',
		iteration = 0,
		onclose,
	}: {
		graph6: string;
		n: number;
		cid?: string;
		goodmanGap?: number;
		autOrder?: number;
		scoreHex?: string;
		histogram?: { k: number; red: number; blue: number }[];
		workerId?: string;
		iteration?: number;
		onclose: () => void;
	} = $props();

	let copied = $state(false);

	function copyGraph6() {
		navigator.clipboard.writeText(graph6);
		copied = true;
		setTimeout(() => { copied = false; }, 1500);
	}

	function handleBackdrop(e: MouseEvent) {
		if (e.target === e.currentTarget) onclose();
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Escape') onclose();
	}
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="backdrop" onclick={handleBackdrop}>
	<div class="popup card">
		<button class="close-btn" onclick={onclose}>x</button>

		<div class="gem-display">
			<GemView {graph6} {n} size={280} {cid} {goodmanGap} {autOrder} {histogram} />
		</div>

		<div class="details">
			{#if cid}
				<div class="detail-row">
					<span class="detail-label">CID</span>
					<span class="detail-value mono">{cid.slice(0, 32)}...</span>
				</div>
			{/if}
			<div class="detail-row">
				<span class="detail-label">n</span>
				<span class="detail-value mono">{n}</span>
			</div>
			{#if goodmanGap !== undefined}
				<div class="detail-row">
					<span class="detail-label">Goodman gap</span>
					<span class="detail-value mono">{goodmanGap.toFixed(4)}</span>
				</div>
			{/if}
			{#if autOrder !== undefined}
				<div class="detail-row">
					<span class="detail-label">|Aut(G)|</span>
					<span class="detail-value mono">{autOrder}</span>
				</div>
			{/if}
			{#if histogram.length > 0}
				<div class="detail-row">
					<span class="detail-label">Cliques</span>
					<div class="histogram">
						{#each histogram as tier}
							<span class="tier">k={tier.k}: <span class="red">{tier.red}</span>/<span class="blue">{tier.blue}</span></span>
						{/each}
					</div>
				</div>
			{/if}
			{#if workerId}
				<div class="detail-row">
					<span class="detail-label">Worker</span>
					<span class="detail-value mono">{workerId}</span>
				</div>
			{/if}
			{#if iteration > 0}
				<div class="detail-row">
					<span class="detail-label">Iteration</span>
					<span class="detail-value mono">{iteration.toLocaleString()}</span>
				</div>
			{/if}
		</div>

		<div class="graph6-section">
			<span class="detail-label">graph6</span>
			<div class="graph6-row">
				<code class="graph6-code">{graph6}</code>
				<button class="copy-btn" onclick={copyGraph6}>
					{copied ? 'Copied!' : 'Copy'}
				</button>
			</div>
		</div>
	</div>
</div>

<style>
	.backdrop {
		position: fixed;
		inset: 0;
		background: rgba(2, 2, 6, 0.85);
		z-index: 1000;
		display: flex;
		align-items: center;
		justify-content: center;
		backdrop-filter: blur(4px);
	}
	.popup {
		position: relative;
		max-width: 420px;
		width: 90%;
		max-height: 90vh;
		overflow-y: auto;
		display: flex;
		flex-direction: column;
		gap: 1rem;
	}
	.close-btn {
		position: absolute;
		top: 0.5rem;
		right: 0.75rem;
		background: none;
		border: none;
		color: var(--color-text-dim);
		font-family: var(--font-mono);
		font-size: 1rem;
		cursor: pointer;
	}
	.close-btn:hover { color: var(--color-text); }

	.gem-display {
		display: flex;
		justify-content: center;
	}

	.details {
		display: flex;
		flex-direction: column;
		gap: 0.3rem;
	}
	.detail-row {
		display: flex;
		justify-content: space-between;
		align-items: baseline;
		gap: 0.75rem;
		font-size: 0.8rem;
	}
	.detail-label {
		color: var(--color-text-muted);
		font-size: 0.7rem;
		flex-shrink: 0;
	}
	.detail-value {
		font-size: 0.75rem;
		text-align: right;
		word-break: break-all;
	}
	.score { font-size: 0.6rem; }

	.histogram {
		display: flex;
		gap: 0.5rem;
		font-family: var(--font-mono);
		font-size: 0.7rem;
	}
	.tier { color: var(--color-text-dim); }
	.red { color: var(--color-red); }
	.blue { color: #60a5fa; }

	.graph6-section {
		border-top: 1px solid var(--color-border);
		padding-top: 0.75rem;
	}
	.graph6-row {
		display: flex;
		gap: 0.5rem;
		align-items: center;
		margin-top: 0.3rem;
	}
	.graph6-code {
		flex: 1;
		font-size: 0.6rem;
		background: var(--color-bg);
		padding: 0.3rem 0.5rem;
		border-radius: 0.3rem;
		word-break: break-all;
		color: var(--color-text-muted);
	}
	.copy-btn {
		font-size: 0.65rem;
		padding: 0.2rem 0.5rem;
		border-radius: 0.25rem;
		background: var(--color-bg);
		border: 1px solid var(--color-border);
		color: var(--color-text-dim);
		cursor: pointer;
		font-family: var(--font-mono);
		flex-shrink: 0;
	}
	.copy-btn:hover { border-color: var(--color-accent); color: var(--color-accent); }
</style>
