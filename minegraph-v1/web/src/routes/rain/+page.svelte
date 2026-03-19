<script lang="ts">
	import GemView from '$lib/components/GemView.svelte';
	import { getLeaderboard, getWorkers, subscribeEvents, type ServerEvent, type WorkerInfo } from '$lib/api';

	// ── Types ────────────────────────────────────────────────

	interface RainGem {
		id: string;
		graph6: string;
		n: number;
		cid: string;
		goodmanGap: number;
		autOrder: number;
		histogram: { k: number; red: number; blue: number }[];
		// Display state
		x: number;          // 0-1 normalized
		y: number;          // 0-1 normalized
		targetY: number;    // where it's drifting toward
		size: number;
		opacity: number;
		glow: number;       // 0-1, 1 = just admitted flash
		source: 'leaderboard' | 'worker' | 'admission';
		label: string;
		rank?: number;
		workerId?: string;
		born: number;
		dying: boolean;     // fading out (replaced)
	}

	// ── State ────────────────────────────────────────────────

	const N = 25;
	let gems = $state<RainGem[]>([]);
	let workers = $state<WorkerInfo[]>([]);
	let containerEl = $state<HTMLDivElement | null>(null);
	let now = $state(Date.now());
	let leaderboardGems = $state<RainGem[]>([]); // persistent leaderboard display
	let idle = $state(true); // no workers active
	let lastAdmission = $state(0);
	let gemIdCounter = 0;

	const MAX_GEMS = 60;
	const WORKER_COLUMN_WIDTH = 0.12; // fraction of screen per worker

	function nextId(): string { return `g${gemIdCounter++}`; }

	// ── Load initial leaderboard for ambient display ─────────

	async function loadLeaderboard() {
		try {
			const d = await getLeaderboard(N, 20);
			const entries = d.entries || [];
			leaderboardGems = entries
				.filter((e: any) => e.graph6)
				.map((e: any, i: number) => ({
					id: nextId(),
					graph6: e.graph6,
					n: N,
					cid: e.cid,
					goodmanGap: e.goodman_gap ?? 0,
					autOrder: e.aut_order ?? 1,
					histogram: e.histogram?.tiers ?? [],
					x: 0.1 + (i % 5) * 0.18,
					y: 0.15 + Math.floor(i / 5) * 0.22,
					targetY: 0.15 + Math.floor(i / 5) * 0.22,
					size: i < 3 ? 80 : 56,
					opacity: i < 3 ? 0.9 : 0.4 + Math.random() * 0.3,
					glow: 0,
					source: 'leaderboard' as const,
					label: `#${e.rank}`,
					rank: e.rank,
					born: Date.now(),
					dying: false,
				}));
		} catch { /* ignore */ }
	}

	async function loadWorkers() {
		try {
			const d = await getWorkers();
			workers = (d.workers || []).filter((w: WorkerInfo) => !w.stale);
			idle = workers.length === 0;
		} catch { workers = []; idle = true; }
	}

	$effect(() => {
		loadLeaderboard();
		loadWorkers();
		const interval = setInterval(loadWorkers, 5000);
		return () => clearInterval(interval);
	});

	// ── SSE: react to real events ────────────────────────────

	let processing = false; // drop events if overwhelmed

	$effect(() => {
		const unsub = subscribeEvents((event: ServerEvent) => {
			if (processing) return; // drop if busy
			processing = true;

			try {
				if (event.type === 'admission' && event.n === N) {
					handleAdmission(event);
				} else if (event.type === 'worker_heartbeat') {
					handleWorkerHeartbeat(event);
				}
			} finally {
				// Release after a short debounce
				setTimeout(() => { processing = false; }, 50);
			}
		});
		return unsub;
	});

	function handleAdmission(event: ServerEvent) {
		lastAdmission = Date.now();

		// Flash all leaderboard gems
		leaderboardGems = leaderboardGems.map(g => ({ ...g, glow: 0.8 }));

		// Spawn an admission gem that drifts down from top
		const admitGem: RainGem = {
			id: nextId(),
			graph6: '', // will be filled by refresh
			n: N,
			cid: event.cid ?? '',
			goodmanGap: 0,
			autOrder: 1,
			histogram: [],
			x: 0.3 + Math.random() * 0.4,
			y: -0.05,
			targetY: 0.5 + Math.random() * 0.3,
			size: 72,
			opacity: 1,
			glow: 1,
			source: 'admission',
			label: `#${event.rank ?? '?'}`,
			rank: event.rank,
			born: Date.now(),
			dying: false,
		};

		gems = [...gems.slice(-MAX_GEMS + 1), admitGem];

		// Refresh leaderboard after a moment
		setTimeout(loadLeaderboard, 500);
	}

	function handleWorkerHeartbeat(event: ServerEvent) {
		if (!event.stats?.current_graph6) return;
		idle = false;

		const workerIdx = workers.findIndex(w => w.worker_id === event.worker_id);
		const col = workerIdx >= 0 ? workerIdx : workers.length;

		// Worker gems appear on the right side, drifting slowly
		const x = 0.65 + col * WORKER_COLUMN_WIDTH;
		if (x > 0.95) return; // off screen

		const workerGem: RainGem = {
			id: nextId(),
			graph6: event.stats.current_graph6!,
			n: N,
			cid: '',
			goodmanGap: event.stats.goodman_gap ?? 0,
			autOrder: event.stats.aut_order ?? 1,
			histogram: [],
			x,
			y: -0.05,
			targetY: 0.3 + Math.random() * 0.5,
			size: 44,
			opacity: 0.6,
			glow: (event.stats.total_admitted ?? 0) > 0 ? 0.3 : 0,
			source: 'worker',
			label: event.worker_id ?? '',
			workerId: event.worker_id,
			born: Date.now(),
			dying: false,
		};

		gems = [...gems.slice(-MAX_GEMS + 1), workerGem];
	}

	// ── Animation loop ───────────────────────────────────────

	$effect(() => {
		let running = true;
		let lastTime = performance.now();

		function tick(time: number) {
			if (!running) return;
			const dt = Math.min(time - lastTime, 50); // cap delta
			lastTime = time;
			now = Date.now();

			// Animate gems: drift toward targetY, fade glow, age out
			gems = gems
				.map(g => {
					const age = (now - g.born) / 1000;
					const newY = g.y + (g.targetY - g.y) * 0.005 * dt; // slow drift
					const newGlow = Math.max(0, g.glow - 0.001 * dt);
					const newOpacity = g.dying
						? Math.max(0, g.opacity - 0.002 * dt)
						: g.opacity;

					// Start dying after 30s for worker gems, 60s for admissions
					const maxAge = g.source === 'worker' ? 30 : g.source === 'admission' ? 60 : 999;
					const shouldDie = age > maxAge && !g.dying;

					return {
						...g,
						y: newY,
						glow: newGlow,
						opacity: newOpacity,
						dying: g.dying || shouldDie,
					};
				})
				.filter(g => g.opacity > 0.01);

			// Fade leaderboard gem glow
			leaderboardGems = leaderboardGems.map(g => ({
				...g,
				glow: Math.max(0, g.glow - 0.0005 * dt),
			}));

			requestAnimationFrame(tick);
		}
		requestAnimationFrame(tick);
		return () => { running = false; };
	});

	// ── Derived state ────────────────────────────────────────

	const totalAdmitted = $derived(workers.reduce((s, w) => s + (w.stats?.total_admitted ?? 0), 0));
	const totalDiscoveries = $derived(workers.reduce((s, w) => s + (w.stats?.total_discoveries ?? 0), 0));
	const timeSinceAdmission = $derived(
		lastAdmission > 0 ? Math.floor((now - lastAdmission) / 1000) : -1
	);
</script>

<div class="rain" bind:this={containerEl}>
	<!-- Leaderboard gems (persistent, left side) -->
	{#each leaderboardGems as gem (gem.id)}
		<div class="rain-gem"
			style="left:{gem.x * 100}%; top:{gem.y * 100}%; opacity:{gem.opacity}"
			class:glow={gem.glow > 0.1}>
			{#if gem.glow > 0.1}
				<div class="glow-ring" style="opacity:{gem.glow}"></div>
			{/if}
			<GemView graph6={gem.graph6} n={gem.n} size={gem.size} cid={gem.cid}
				goodmanGap={gem.goodmanGap} autOrder={gem.autOrder}
				histogram={gem.histogram} label={gem.label} />
		</div>
	{/each}

	<!-- Dynamic gems (workers + admissions) -->
	{#each gems as gem (gem.id)}
		{#if gem.graph6}
			<div class="rain-gem"
				class:admission={gem.source === 'admission'}
				class:worker-gem={gem.source === 'worker'}
				style="left:{gem.x * 100}%; top:{gem.y * 100}%; opacity:{gem.opacity}">
				{#if gem.glow > 0.2}
					<div class="glow-ring" style="opacity:{gem.glow}"></div>
				{/if}
				<GemView graph6={gem.graph6} n={gem.n} size={gem.size} cid={gem.cid}
					goodmanGap={gem.goodmanGap} autOrder={gem.autOrder}
					histogram={gem.histogram} label={gem.label} />
			</div>
		{/if}
	{/each}

	<!-- Top overlay -->
	<div class="overlay top-overlay">
		<span class="rain-title">MineGraph</span>
		<div class="rain-stats">
			{#if !idle}
				<span class="stat">{workers.length} worker{workers.length !== 1 ? 's' : ''}</span>
				<span class="stat-dim">{totalDiscoveries.toLocaleString()} found</span>
				<span class="stat-dim">{totalAdmitted} admitted</span>
			{:else}
				<span class="stat-dim">idle</span>
			{/if}
		</div>
	</div>

	<!-- Bottom overlay -->
	<div class="overlay bottom-overlay">
		{#if timeSinceAdmission >= 0 && timeSinceAdmission < 10}
			<span class="admission-flash">Admission</span>
		{:else if idle}
			<span class="idle-text">Waiting for workers...</span>
		{/if}
	</div>

	<!-- Subtle column markers for workers -->
	{#each workers as worker, i}
		<div class="worker-col-label"
			style="left: {(0.65 + i * WORKER_COLUMN_WIDTH) * 100}%">
			<span class="wcl-text">{worker.worker_id}</span>
		</div>
	{/each}

	<!-- Edge fades -->
	<div class="fade-top"></div>
	<div class="fade-bottom"></div>
</div>

<style>
	.rain {
		position: fixed;
		inset: 0;
		background: #020206;
		overflow: hidden;
		z-index: 100;
	}

	.rain-gem {
		position: absolute;
		transform: translateX(-50%) translateY(-50%);
		transition: top 2s ease-out, opacity 1s ease;
		pointer-events: none;
	}

	.rain-gem.admission {
		z-index: 10;
		filter: drop-shadow(0 0 16px rgba(99, 102, 241, 0.5))
		        drop-shadow(0 0 32px rgba(168, 85, 247, 0.25));
		animation: arrive 1.5s ease-out;
	}

	.rain-gem.worker-gem {
		z-index: 5;
		animation: drift-in 2s ease-out;
	}

	@keyframes arrive {
		0% { transform: translateX(-50%) translateY(-50%) scale(1.6); filter: drop-shadow(0 0 30px rgba(99, 102, 241, 0.9)) drop-shadow(0 0 60px rgba(168, 85, 247, 0.5)); }
		100% { transform: translateX(-50%) translateY(-50%) scale(1); }
	}

	@keyframes drift-in {
		0% { opacity: 0; transform: translateX(-50%) translateY(-80%); }
		100% { opacity: 0.6; transform: translateX(-50%) translateY(-50%); }
	}

	.glow-ring {
		position: absolute;
		inset: -12px;
		border-radius: 50%;
		border: 1px solid rgba(99, 102, 241, 0.4);
		background: radial-gradient(circle, rgba(99, 102, 241, 0.08) 0%, transparent 70%);
		pointer-events: none;
		animation: pulse-ring 2s ease-out;
	}

	@keyframes pulse-ring {
		0% { transform: scale(0.5); opacity: 1; }
		100% { transform: scale(2); opacity: 0; }
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
	.bottom-overlay {
		bottom: 0; padding: 2rem 1.5rem;
		background: linear-gradient(0deg, rgba(2,2,6,0.9) 0%, transparent 100%);
		height: 100px;
	}

	.rain-title {
		font-family: var(--font-mono); font-size: 0.9rem; font-weight: 700;
		background: linear-gradient(135deg, #6366f1, #a855f7);
		-webkit-background-clip: text; -webkit-text-fill-color: transparent; background-clip: text;
	}
	.rain-stats { display: flex; gap: 1rem; }
	.stat { font-family: var(--font-mono); font-size: 0.7rem; color: rgba(99, 102, 241, 0.7); }
	.stat-dim { font-family: var(--font-mono); font-size: 0.65rem; color: rgba(136, 136, 160, 0.4); }

	.admission-flash {
		font-family: var(--font-mono); font-size: 1rem; font-weight: 700;
		color: rgba(99, 102, 241, 0.9);
		text-shadow: 0 0 20px rgba(99, 102, 241, 0.5);
		animation: flash-text 1.5s ease-out;
	}
	@keyframes flash-text {
		0% { opacity: 1; transform: scale(1.2); }
		100% { opacity: 0; transform: scale(1); }
	}

	.idle-text {
		font-family: var(--font-mono); font-size: 0.75rem;
		color: rgba(136, 136, 160, 0.3);
		animation: idle-pulse 3s ease-in-out infinite;
	}
	@keyframes idle-pulse {
		0%, 100% { opacity: 0.2; }
		50% { opacity: 0.5; }
	}

	.worker-col-label {
		position: absolute; bottom: 8px;
		transform: translateX(-50%);
		z-index: 25; pointer-events: none;
	}
	.wcl-text {
		font-family: var(--font-mono); font-size: 0.5rem;
		color: rgba(99, 102, 241, 0.2);
	}

	.fade-top {
		position: absolute; top: 0; left: 0; right: 0; height: 80px;
		background: linear-gradient(180deg, #020206 0%, transparent 100%);
		z-index: 20; pointer-events: none;
	}
	.fade-bottom {
		position: absolute; bottom: 0; left: 0; right: 0; height: 80px;
		background: linear-gradient(0deg, #020206 0%, transparent 100%);
		z-index: 20; pointer-events: none;
	}
</style>
