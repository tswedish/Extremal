<script lang="ts">
	import GemViewSquare from '@extremal/shared/components/GemViewSquare.svelte';
	import GemPopup from '@extremal/shared/components/GemPopup.svelte';
	import { getLeaderboard, subscribeEvents, type ServerEvent, type LeaderboardEntry } from '$lib/api';

	// ── Types ────────────────────────────────────────────────

	interface ColumnGem {
		graph6: string;
		n: number;
		cid: string;
		goodmanGap: number;
		autOrder: number;
		rank?: number;
		keyId: string;
		workerId: string;
		lastUpdated: number;
	}

	interface WorkerColumn {
		keyId: string;
		workerId: string;
		gems: ColumnGem[];
		lastSeen: number;
	}

	// ── Settings ─────────────────────────────────────────────

	let gemScale = $state(90);
	let maxGems = $state(30);
	let showControls = $state(true);

	// ── State ────────────────────────────────────────────────

	let leaderboardGems = $state<ColumnGem[]>([]);
	let workerColumns = $state<Map<string, WorkerColumn>>(new Map());
	let columnOrder = $state<string[]>([]);
	let admissionsCount = $state(0);
	let now = $state(Date.now());
	let popupGem = $state<ColumnGem | null>(null);
	let activeN = $state(25);

	// ── Leaderboard loading ─────────────────────────────────

	async function loadLeaderboard() {
		try {
			const d = await getLeaderboard(activeN, maxGems);
			const entries = d.entries || [];
			leaderboardGems = entries
				.filter((e: LeaderboardEntry) => e.graph6)
				.map((e: LeaderboardEntry) => ({
					graph6: e.graph6!,
					n: activeN,
					cid: e.cid,
					goodmanGap: e.goodman_gap ?? 0,
					autOrder: e.aut_order ?? 1,
					rank: e.rank,
					keyId: e.key_id,
					workerId: '',
					lastUpdated: Date.now(),
				}));
		} catch { /* server offline */ }
	}

	$effect(() => {
		loadLeaderboard();
	});

	// ── SSE events ──────────────────────────────────────────

	$effect(() => {
		const unsub = subscribeEvents((event: ServerEvent) => {
			if (event.type === 'admission') {
				handleAdmission(event);
			} else if (event.type === 'submission') {
				handleSubmission(event);
			}
		});
		return unsub;
	});

	function getColumnKey(event: ServerEvent): string {
		const wid = event.metadata?.worker_id ?? '';
		return `${event.key_id ?? 'unknown'}:${wid}`;
	}

	function handleAdmission(event: ServerEvent) {
		admissionsCount++;
		const colKey = getColumnKey(event);
		const n = event.n ?? activeN;

		// Only track admissions for the active n
		if (n !== activeN) return;

		const gem: ColumnGem = {
			graph6: event.graph6 ?? '',
			n,
			cid: event.cid ?? '',
			goodmanGap: event.goodman_gap ?? 0,
			autOrder: event.aut_order ?? 1,
			rank: event.rank,
			keyId: event.key_id ?? '',
			workerId: event.metadata?.worker_id ?? '',
			lastUpdated: Date.now(),
		};

		// Update worker column
		if (gem.graph6) {
			const cols = new Map(workerColumns);
			const existing = cols.get(colKey);
			if (existing) {
				// Insert sorted by rank (lower = better)
				const gems = [...existing.gems];
				// Dedup by CID
				const dupIdx = gems.findIndex(g => g.cid === gem.cid);
				if (dupIdx !== -1) {
					gems[dupIdx] = { ...gems[dupIdx], lastUpdated: Date.now() };
				} else {
					const insertIdx = gems.findIndex(g => (g.rank ?? 9999) > (gem.rank ?? 9999));
					if (insertIdx === -1) gems.push(gem);
					else gems.splice(insertIdx, 0, gem);
				}
				cols.set(colKey, { ...existing, gems: gems.slice(0, maxGems), lastSeen: Date.now() });
			} else {
				cols.set(colKey, {
					keyId: event.key_id ?? '',
					workerId: event.metadata?.worker_id ?? '',
					gems: [gem],
					lastSeen: Date.now(),
				});
				if (!columnOrder.includes(colKey)) {
					columnOrder = [...columnOrder, colKey];
				}
			}
			workerColumns = cols;
		}

		// Refresh leaderboard
		setTimeout(loadLeaderboard, 500);
	}

	function handleSubmission(event: ServerEvent) {
		const colKey = getColumnKey(event);
		// Just update lastSeen so column stays alive
		const cols = new Map(workerColumns);
		const existing = cols.get(colKey);
		if (existing) {
			cols.set(colKey, { ...existing, lastSeen: Date.now() });
			workerColumns = cols;
		} else {
			// New column from a non-admitted submission
			cols.set(colKey, {
				keyId: event.key_id ?? '',
				workerId: event.metadata?.worker_id ?? '',
				gems: [],
				lastSeen: Date.now(),
			});
			if (!columnOrder.includes(colKey)) {
				columnOrder = [...columnOrder, colKey];
			}
			workerColumns = cols;
		}
	}

	// ── Stale column cleanup (every 5s) ─────────────────────

	$effect(() => {
		const interval = setInterval(() => {
			now = Date.now();
			const staleThreshold = 120_000; // 2 minutes
			const cols = new Map(workerColumns);
			let changed = false;
			for (const [key, col] of cols) {
				if (now - col.lastSeen > staleThreshold && col.gems.length === 0) {
					cols.delete(key);
					columnOrder = columnOrder.filter(k => k !== key);
					changed = true;
				}
			}
			if (changed) workerColumns = cols;
		}, 5000);
		return () => clearInterval(interval);
	});

	// ── Derived ─────────────────────────────────────────────

	const activeColumns = $derived(
		columnOrder
			.map(key => ({ key, col: workerColumns.get(key)! }))
			.filter(c => c.col)
	);
</script>

<div class="rain">
	<!-- Controls -->
	{#if showControls}
		<div class="rain-controls">
			<label class="control">
				<span>Size</span>
				<input type="range" min="40" max="200" bind:value={gemScale} />
				<span class="mono">{gemScale}px</span>
			</label>
			<label class="control">
				<span>Depth</span>
				<input type="range" min="5" max="100" step="5" bind:value={maxGems} />
				<span class="mono">{maxGems}</span>
			</label>
			<label class="control">
				<span>n</span>
				<input type="number" min="3" max="100" bind:value={activeN} onchange={() => { loadLeaderboard(); workerColumns = new Map(); columnOrder = []; }} class="n-input" />
			</label>
			<button class="ctrl-btn" onclick={() => showControls = false}>Fullscreen</button>
		</div>
	{:else}
		<button class="show-controls-btn" onclick={() => showControls = true}>Controls</button>
	{/if}

	<!-- Columns layout -->
	<div class="rain-columns">
		<!-- Worker columns (left side) -->
		{#each activeColumns as { key, col }}
			<div class="rain-column" style="width: {gemScale + 8}px">
				<div class="col-header">
					<span class="col-id">{col.workerId || col.keyId.slice(0, 8)}</span>
					<span class="col-dot" class:active={Date.now() - col.lastSeen < 30000}></span>
				</div>
				<div class="gem-stack">
					{#each col.gems as gem, i (gem.cid || i)}
						<div class="gem-slot">
							<GemViewSquare
								graph6={gem.graph6}
								n={gem.n}
								size={gemScale}
								cid={gem.cid}
								goodmanGap={gem.goodmanGap}
								autOrder={gem.autOrder}
								glowing={Date.now() - gem.lastUpdated < 3000}
								onclick={() => { popupGem = gem; }}
							/>
						</div>
					{/each}
				</div>
			</div>
		{/each}

		<!-- Leaderboard column (center, wider) -->
		<div class="rain-column leaderboard-column" style="width: {gemScale + 24}px">
			<div class="col-header lb-header">
				<span class="col-id lb-title">Leaderboard</span>
				<span class="col-n">n={activeN}</span>
			</div>
			<div class="gem-stack">
				{#each leaderboardGems as gem, i (gem.cid || i)}
					<div class="gem-slot">
						<div class="rank-badge">#{gem.rank}</div>
						<GemViewSquare
							graph6={gem.graph6}
							n={gem.n}
							size={gemScale}
							cid={gem.cid}
							goodmanGap={gem.goodmanGap}
							autOrder={gem.autOrder}
							onclick={() => { popupGem = gem; }}
						/>
					</div>
				{/each}
			</div>
		</div>

		<!-- More worker columns (right side, if any overflow) -->

		{#if activeColumns.length === 0 && leaderboardGems.length === 0}
			<div class="rain-empty">
				<span>Waiting for submissions...</span>
			</div>
		{/if}
	</div>

	<!-- Overlays -->
	<div class="overlay top-overlay">
		<span class="rain-title">Extremal</span>
		<div class="rain-stats">
			{#if admissionsCount > 0}
				<span class="stat">{admissionsCount} admitted</span>
				<span class="stat-dim">{activeColumns.length} sources</span>
			{:else}
				<span class="stat-dim">waiting for submissions...</span>
			{/if}
		</div>
	</div>

	<div class="fade-top"></div>
	<div class="fade-bottom"></div>
</div>

<!-- Popup -->
{#if popupGem}
	<GemPopup
		graph6={popupGem.graph6}
		n={popupGem.n}
		cid={popupGem.cid}
		goodmanGap={popupGem.goodmanGap}
		autOrder={popupGem.autOrder}
		workerId={popupGem.workerId || popupGem.keyId}
		onclose={() => { popupGem = null; }}
	/>
{/if}

<style>
	.rain {
		position: fixed;
		inset: 0;
		background: #020206;
		overflow: hidden;
		z-index: 100;
	}

	/* Controls */
	.rain-controls {
		position: absolute; top: 0.5rem; left: 0.5rem; z-index: 40;
		display: flex; gap: 0.75rem; align-items: center;
		background: rgba(8, 8, 14, 0.9);
		padding: 0.4rem 0.75rem; border-radius: 0.4rem;
		border: 1px solid var(--color-border);
	}
	.control {
		display: flex; gap: 0.3rem; align-items: center;
		font-size: 0.6rem; color: var(--color-text-dim);
		font-family: var(--font-mono);
	}
	.control input[type=range] { width: 60px; height: 3px; }
	.n-input {
		width: 40px; font-family: var(--font-mono); font-size: 0.6rem;
		background: var(--color-bg); border: 1px solid var(--color-border);
		color: var(--color-text); border-radius: 0.2rem; padding: 0.1rem 0.3rem;
		text-align: center;
	}
	.ctrl-btn {
		font-family: var(--font-mono); font-size: 0.6rem;
		padding: 0.15rem 0.4rem; border-radius: 0.2rem;
		background: none; border: 1px solid var(--color-border);
		color: var(--color-text-dim); cursor: pointer;
	}
	.show-controls-btn {
		position: absolute; top: 0.5rem; left: 0.5rem; z-index: 40;
		font-family: var(--font-mono); font-size: 0.55rem;
		padding: 0.15rem 0.4rem; border-radius: 0.2rem;
		background: rgba(8, 8, 14, 0.7); border: 1px solid rgba(42, 42, 58, 0.3);
		color: rgba(85, 85, 104, 0.4); cursor: pointer;
	}
	.show-controls-btn:hover { color: var(--color-text-dim); }

	/* Columns */
	.rain-columns {
		display: flex;
		gap: 6px;
		height: 100%;
		padding: 2.5rem 1rem 1rem;
		overflow-x: auto;
		align-items: flex-start;
		justify-content: center;
	}

	.rain-column {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 2px;
		flex-shrink: 0;
	}

	.leaderboard-column {
		order: 0;
		margin: 0 1rem;
		border-left: 1px solid rgba(99, 102, 241, 0.1);
		border-right: 1px solid rgba(99, 102, 241, 0.1);
		padding: 0 0.5rem;
	}

	.col-header {
		display: flex; align-items: center; gap: 0.2rem;
		margin-bottom: 0.25rem;
	}
	.col-id {
		font-family: var(--font-mono); font-size: 0.45rem;
		color: rgba(99, 102, 241, 0.4);
		max-width: 80px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
	}
	.col-dot {
		width: 4px; height: 4px; border-radius: 50%;
		background: var(--color-text-dim);
	}
	.col-dot.active { background: var(--color-green); }
	.col-n {
		font-family: var(--font-mono); font-size: 0.4rem;
		color: rgba(136, 136, 160, 0.3);
	}

	.lb-header { justify-content: center; gap: 0.5rem; }
	.lb-title {
		font-size: 0.55rem !important;
		color: rgba(99, 102, 241, 0.6) !important;
		font-weight: 700;
	}

	.gem-stack {
		display: flex;
		flex-direction: column;
		gap: 2px;
	}

	.gem-slot { position: relative; }

	.rank-badge {
		position: absolute;
		top: 2px; left: 2px; z-index: 2;
		font-family: var(--font-mono);
		font-size: 0.45rem;
		font-weight: 700;
		color: rgba(99, 102, 241, 0.7);
		background: rgba(8, 8, 14, 0.7);
		padding: 0 0.2rem;
		border-radius: 0.15rem;
	}

	.rain-empty {
		position: absolute;
		top: 50%; left: 50%;
		transform: translate(-50%, -50%);
	}
	.rain-empty span {
		font-family: var(--font-mono); font-size: 0.75rem;
		color: rgba(136, 136, 160, 0.3);
		animation: idle-pulse 3s ease-in-out infinite;
	}

	/* Overlays */
	.overlay {
		position: absolute; left: 0; right: 0; z-index: 30;
		display: flex; align-items: center; justify-content: center; gap: 1.5rem;
		pointer-events: none;
	}
	.top-overlay {
		top: 0; padding: 1rem 1.5rem;
		background: linear-gradient(180deg, rgba(2,2,6,0.85) 0%, transparent 100%);
		justify-content: space-between;
	}
	.rain-title {
		font-family: var(--font-mono); font-size: 0.9rem; font-weight: 700;
		background: linear-gradient(135deg, #6366f1, #a855f7);
		-webkit-background-clip: text; -webkit-text-fill-color: transparent; background-clip: text;
	}
	.rain-stats { display: flex; gap: 1rem; }
	.stat { font-family: var(--font-mono); font-size: 0.7rem; color: rgba(99, 102, 241, 0.7); }
	.stat-dim { font-family: var(--font-mono); font-size: 0.65rem; color: rgba(136, 136, 160, 0.4); }

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

	@keyframes idle-pulse {
		0%, 100% { opacity: 0.2; }
		50% { opacity: 0.5; }
	}
</style>
