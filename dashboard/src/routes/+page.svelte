<script lang="ts">
	import { store } from '$lib/dashboard-store.svelte';
	import GemViewSquare from '@minegraph/shared/components/GemViewSquare.svelte';
	import GemPopup from '@minegraph/shared/components/GemPopup.svelte';
	import type { RainGemData } from '@minegraph/shared';

	// ── Connection panel state ──────────────────────────
	let urlInput = $state(store.serverUrl);
	let showConnPanel = $state(false);

	function reconnect() {
		store.connect(urlInput);
		store.saveSettings();
	}

	// ── Popup state ─────────────────────────────────────
	let popupGem = $state<RainGemData | null>(null);

	// ── Derived ─────────────────────────────────────────
	const workerList = $derived(
		store.columnOrder
			.map(id => store.workers.get(id))
			.filter((w): w is NonNullable<typeof w> => !!w)
	);
	const connectedCount = $derived(workerList.filter(w => w.connected).length);
	const totalDiscoveries = $derived(workerList.reduce((s, w) => s + w.discoveriesSoFar, 0));
	const totalAdmitted = $derived(workerList.reduce((s, w) => s + w.totalAdmitted, 0));

	// ── Rain: opacity decay ─────────────────────────────
	let now = $state(Date.now());

	$effect(() => {
		if (store.mode !== 'rain') return;
		let running = true;
		function tick() {
			if (!running) return;
			now = Date.now();
			requestAnimationFrame(tick);
		}
		requestAnimationFrame(tick);
		return () => { running = false; };
	});

	function gemOpacity(gem: RainGemData): number {
		const age = (now - gem.lastUpdated) / 1000;
		return Math.max(0.15, 1.0 - age / store.fadeDuration);
	}
</script>

{#if store.mode === 'monitor'}
	<!-- ═══ MONITOR MODE ═══ -->
	<div class="monitor">
		<!-- Connection panel toggle -->
		<div class="conn-bar">
			<button class="conn-toggle" onclick={() => showConnPanel = !showConnPanel}>
				{showConnPanel ? 'Hide' : 'Connection'}
			</button>
			{#if showConnPanel}
				<div class="conn-panel">
					<input type="text" bind:value={urlInput} placeholder="ws://localhost:4000/ws/ui" class="url-input" />
					<button class="btn" onclick={reconnect}>Connect</button>
				</div>
			{/if}
		</div>

		<!-- Fleet summary -->
		<div class="summary-grid">
			<div class="stat-card">
				<div class="stat-value">{connectedCount}</div>
				<div class="stat-label">Workers</div>
			</div>
			<div class="stat-card">
				<div class="stat-value">{totalDiscoveries.toLocaleString()}</div>
				<div class="stat-label">Discoveries</div>
			</div>
			<div class="stat-card">
				<div class="stat-value">{totalAdmitted.toLocaleString()}</div>
				<div class="stat-label">Admitted</div>
			</div>
		</div>

		<!-- Worker cards -->
		{#if workerList.length === 0}
			<div class="card empty-state">
				<p>No workers connected.</p>
				<p class="dim">Start a dashboard relay server and connect workers.</p>
				<code>./run dashboard</code>
			</div>
		{:else}
			<div class="worker-grid">
				{#each workerList as w}
					<div class="card worker-card" class:disconnected={!w.connected}>
						<div class="worker-header">
							<span class="worker-id mono">{w.workerId}</span>
							{#if w.connected}
								<span class="badge badge-green">live</span>
							{:else}
								<span class="badge badge-red">offline</span>
							{/if}
						</div>
						<div class="worker-meta">
							<span>n={w.n}</span>
							<span class="dim">{w.strategy}</span>
						</div>

						<!-- Progress bar -->
						{#if w.maxIters > 0}
							<div class="progress-bar">
								<div class="progress-fill" style="width: {(w.iteration / w.maxIters * 100).toFixed(1)}%"></div>
							</div>
							<div class="progress-text">{w.iteration.toLocaleString()} / {w.maxIters.toLocaleString()}</div>
						{/if}

						<div class="worker-stats">
							<div class="ws-row"><span class="ws-label">Round</span><span class="ws-value mono">{w.round}</span></div>
							<div class="ws-row"><span class="ws-label">Discoveries</span><span class="ws-value mono">{w.discoveriesSoFar.toLocaleString()}</span></div>
							<div class="ws-row"><span class="ws-label">Submitted</span><span class="ws-value mono">{w.totalSubmitted}</span></div>
							<div class="ws-row"><span class="ws-label">Admitted</span><span class="ws-value mono accent">{w.totalAdmitted}</span></div>
							<div class="ws-row"><span class="ws-label">Buffered</span><span class="ws-value mono">{w.buffered}</span></div>
							<div class="ws-row"><span class="ws-label">Last round</span><span class="ws-value mono">{w.lastRoundMs}ms</span></div>
						</div>

						<!-- Current search gem -->
						{#if w.currentGraph6}
							<div class="current-gem">
								<GemViewSquare graph6={w.currentGraph6} n={w.n} size={80} invalid={w.violationScore > 0} />
							</div>
						{/if}
					</div>
				{/each}
			</div>
		{/if}
	</div>

{:else}
	<!-- ═══ RAIN MODE ═══ -->
	<div class="rain">
		<!-- Controls overlay -->
		{#if store.showInfo}
			<div class="rain-controls">
				<label class="control">
					<span>Size</span>
					<input type="range" min="20" max="200" bind:value={store.gemScale} onchange={() => store.saveSettings()} />
					<span class="mono">{store.gemScale}px</span>
				</label>
				<label class="control">
					<span>Fade</span>
					<input type="range" min="10" max="600" bind:value={store.fadeDuration} onchange={() => store.saveSettings()} />
					<span class="mono">{store.fadeDuration}s</span>
				</label>
				<button class="ctrl-btn" onclick={() => { store.showInfo = false; store.saveSettings(); }}>Fullscreen</button>
			</div>
		{:else}
			<button class="show-controls-btn" onclick={() => { store.showInfo = true; store.saveSettings(); }}>Controls</button>
		{/if}

		<!-- Worker columns -->
		<div class="rain-columns">
			{#each workerList as w}
				<div class="rain-column" style="width: {store.gemScale + 8}px">
					<!-- Column header -->
					<div class="col-header" class:disconnected={!w.connected}>
						<span class="col-id">{w.workerId}</span>
						<span class="col-dot" class:live={w.connected}></span>
						<span class="col-n">n={w.n}</span>
					</div>

					<!-- Best gems stack -->
					<div class="gem-stack">
						{#each w.bestGems as gem, i (gem.cid || i)}
							<div
								class="gem-slot"
								style="transition: transform 0.3s ease"
							>
								<GemViewSquare
									graph6={gem.graph6}
									n={gem.n}
									size={store.gemScale}
									cid={gem.cid}
									goodmanGap={gem.goodmanGap}
									autOrder={gem.autOrder}
									opacity={gemOpacity(gem)}
									glowing={now - gem.lastUpdated < 2000}
									onclick={() => { popupGem = gem; }}
								/>
							</div>
						{/each}
					</div>

					<!-- Current search representative -->
					{#if w.currentGraph6}
						<div class="current-rep">
							<GemViewSquare
								graph6={w.currentGraph6}
								n={w.n}
								size={store.gemScale}
								opacity={0.4}
								invalid={w.violationScore > 0}
							/>
						</div>
					{/if}
				</div>
			{/each}

			{#if workerList.length === 0}
				<div class="rain-empty">
					<span>Waiting for workers...</span>
				</div>
			{/if}
		</div>

		<!-- Fades -->
		<div class="fade-top"></div>
		<div class="fade-bottom"></div>
	</div>
{/if}

<!-- Popup -->
{#if popupGem}
	<GemPopup
		graph6={popupGem.graph6}
		n={popupGem.n}
		cid={popupGem.cid}
		goodmanGap={popupGem.goodmanGap}
		autOrder={popupGem.autOrder}
		scoreHex={popupGem.scoreHex}
		histogram={popupGem.histogram}
		workerId={popupGem.workerId}
		iteration={popupGem.iteration}
		onclose={() => { popupGem = null; }}
	/>
{/if}

<style>
	/* ── Monitor ────────────────────────────────────── */
	.monitor { padding-top: 0.5rem; }

	.conn-bar { margin-bottom: 1rem; }
	.conn-toggle {
		font-family: var(--font-mono); font-size: 0.7rem;
		padding: 0.2rem 0.5rem; border-radius: 0.25rem;
		background: var(--color-bg); border: 1px solid var(--color-border);
		color: var(--color-text-dim); cursor: pointer;
	}
	.conn-panel {
		display: flex; gap: 0.5rem; margin-top: 0.5rem; align-items: center;
	}
	.url-input {
		flex: 1; font-family: var(--font-mono); font-size: 0.75rem;
		padding: 0.3rem 0.5rem; border-radius: 0.3rem;
		background: var(--color-bg); border: 1px solid var(--color-border);
		color: var(--color-text);
	}
	.btn {
		font-family: var(--font-mono); font-size: 0.7rem;
		padding: 0.3rem 0.75rem; border-radius: 0.3rem;
		background: var(--color-accent); border: none;
		color: white; cursor: pointer;
	}

	.summary-grid {
		display: grid; grid-template-columns: repeat(auto-fit, minmax(120px, 1fr));
		gap: 0.75rem; margin-bottom: 1.5rem;
	}
	.stat-card {
		background: var(--color-surface); border: 1px solid var(--color-border);
		border-radius: 0.75rem; padding: 0.75rem; text-align: center;
	}
	.stat-value { font-family: var(--font-mono); font-size: 1.5rem; font-weight: 700; color: var(--color-accent); }
	.stat-label { font-size: 0.7rem; color: var(--color-text-muted); }

	.empty-state { text-align: center; padding: 3rem; }
	.empty-state p { margin-bottom: 0.5rem; }
	.empty-state code {
		display: inline-block; background: var(--color-surface-2);
		padding: 0.4rem 0.75rem; border-radius: 0.3rem;
		font-family: var(--font-mono); font-size: 0.8rem; color: var(--color-accent);
	}
	.dim { color: var(--color-text-dim); font-size: 0.8rem; }
	.accent { color: var(--color-accent); }

	.worker-grid {
		display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
		gap: 0.75rem;
	}
	.worker-card { transition: border-color 0.15s; }
	.worker-card.disconnected { opacity: 0.4; }
	.worker-header {
		display: flex; justify-content: space-between; align-items: center;
		margin-bottom: 0.3rem;
	}
	.worker-id { font-weight: 700; font-size: 0.85rem; }
	.worker-meta {
		display: flex; gap: 0.75rem; font-size: 0.75rem; color: var(--color-text-muted);
		margin-bottom: 0.5rem; padding-bottom: 0.4rem;
		border-bottom: 1px solid var(--color-border);
	}

	.progress-bar {
		height: 3px; background: var(--color-bg); border-radius: 2px;
		margin-bottom: 0.15rem; overflow: hidden;
	}
	.progress-fill { height: 100%; background: var(--color-accent); transition: width 0.3s; border-radius: 2px; }
	.progress-text { font-family: var(--font-mono); font-size: 0.6rem; color: var(--color-text-dim); margin-bottom: 0.4rem; }

	.worker-stats { display: flex; flex-direction: column; gap: 0.15rem; }
	.ws-row { display: flex; justify-content: space-between; font-size: 0.75rem; }
	.ws-label { color: var(--color-text-muted); }
	.ws-value { font-size: 0.75rem; }

	.current-gem { margin-top: 0.5rem; display: flex; justify-content: center; }

	/* ── Rain ───────────────────────────────────────── */
	.rain {
		position: relative;
		height: 100%;
		background: #020206;
		overflow: hidden;
	}

	.rain-controls {
		position: absolute; top: 0.5rem; left: 0.5rem; z-index: 30;
		display: flex; gap: 0.75rem; align-items: center;
		background: rgba(8, 8, 14, 0.85);
		padding: 0.4rem 0.75rem; border-radius: 0.4rem;
		border: 1px solid var(--color-border);
	}
	.control {
		display: flex; gap: 0.3rem; align-items: center;
		font-size: 0.6rem; color: var(--color-text-dim);
	}
	.control input[type=range] { width: 60px; height: 3px; }
	.ctrl-btn {
		font-family: var(--font-mono); font-size: 0.6rem;
		padding: 0.15rem 0.4rem; border-radius: 0.2rem;
		background: none; border: 1px solid var(--color-border);
		color: var(--color-text-dim); cursor: pointer;
	}
	.show-controls-btn {
		position: absolute; top: 0.5rem; left: 0.5rem; z-index: 30;
		font-family: var(--font-mono); font-size: 0.55rem;
		padding: 0.15rem 0.4rem; border-radius: 0.2rem;
		background: rgba(8, 8, 14, 0.7); border: 1px solid rgba(42, 42, 58, 0.3);
		color: rgba(85, 85, 104, 0.4); cursor: pointer;
	}
	.show-controls-btn:hover { color: var(--color-text-dim); }

	.rain-columns {
		display: flex;
		gap: 4px;
		height: 100%;
		padding: 2.5rem 0.5rem 1rem;
		overflow-x: auto;
		align-items: flex-start;
	}

	.rain-column {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 2px;
		flex-shrink: 0;
	}

	.col-header {
		display: flex; align-items: center; gap: 0.2rem;
		margin-bottom: 0.25rem;
	}
	.col-header.disconnected { opacity: 0.3; }
	.col-id { font-family: var(--font-mono); font-size: 0.45rem; color: rgba(99, 102, 241, 0.4); }
	.col-dot {
		width: 4px; height: 4px; border-radius: 50%;
		background: var(--color-red);
	}
	.col-dot.live { background: var(--color-green); }
	.col-n { font-family: var(--font-mono); font-size: 0.4rem; color: rgba(136, 136, 160, 0.3); }

	.gem-stack {
		display: flex;
		flex-direction: column;
		gap: 2px;
	}

	.gem-slot {
		position: relative;
	}

	.current-rep {
		margin-top: auto;
		padding-top: 4px;
		border-top: 1px solid rgba(42, 42, 58, 0.2);
	}

	.rain-empty {
		position: absolute;
		top: 50%;
		left: 50%;
		transform: translate(-50%, -50%);
	}
	.rain-empty span {
		font-family: var(--font-mono); font-size: 0.75rem;
		color: rgba(136, 136, 160, 0.3);
		animation: idle-pulse 3s ease-in-out infinite;
	}
	@keyframes idle-pulse {
		0%, 100% { opacity: 0.2; }
		50% { opacity: 0.5; }
	}

	.fade-top {
		position: absolute; top: 0; left: 0; right: 0; height: 60px;
		background: linear-gradient(180deg, #020206 0%, transparent 100%);
		z-index: 20; pointer-events: none;
	}
	.fade-bottom {
		position: absolute; bottom: 0; left: 0; right: 0; height: 60px;
		background: linear-gradient(0deg, #020206 0%, transparent 100%);
		z-index: 20; pointer-events: none;
	}
</style>
