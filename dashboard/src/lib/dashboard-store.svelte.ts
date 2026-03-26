// Dashboard state management — connects to relay server via WebSocket

import type { RainGemData } from '@extremal/shared';

// ── Protocol types (mirror Rust protocol.rs) ──────────────

export interface WorkerMessage {
	type: 'Register' | 'Progress' | 'Discovery' | 'RoundComplete';
	// Register
	key_id?: string;
	worker_id?: string;
	n?: number;
	strategy?: string;
	metadata?: Record<string, any>;
	// Progress
	iteration?: number;
	max_iters?: number;
	violation_score?: number;
	current_graph6?: string;
	discoveries_so_far?: number;
	// Discovery
	graph6?: string;
	cid?: string;
	goodman_gap?: number;
	aut_order?: number;
	score_hex?: string;
	histogram?: [number, number, number][];
	// RoundComplete
	round?: number;
	duration_ms?: number;
	discoveries?: number;
	submitted?: number;
	admitted?: number;
	buffered?: number;
}

export interface UiEvent {
	type: 'WorkerConnected' | 'WorkerDisconnected' | 'WorkerEvent';
	worker_id: string;
	// WorkerConnected
	key_id?: string;
	n?: number;
	strategy?: string;
	metadata?: Record<string, any>;
	verified?: boolean;
	api_addr?: string;
	// WorkerEvent
	event?: WorkerMessage;
}

// ── Worker state ──────────────────────────────────────────

export interface WorkerState {
	workerId: string;
	keyId: string;
	n: number;
	strategy: string;
	metadata: Record<string, any> | null;
	connected: boolean;
	verified: boolean;
	// Live progress
	iteration: number;
	maxIters: number;
	violationScore: number;
	currentGraph6: string;
	discoveriesSoFar: number;
	// Round history
	round: number;
	lastRoundMs: number;
	totalSubmitted: number;
	totalAdmitted: number;
	buffered: number;
	// Best discoveries (sorted by score)
	bestGems: RainGemData[];
}

// ── Dashboard store ───────────────────────────────────────

const MAX_GEMS_PER_WORKER = 20;

class DashboardStore {
	serverUrl = $state('ws://localhost:4000/ws/ui');
	connected = $state(false);
	workers = $state<Map<string, WorkerState>>(new Map());
	mode = $state<'monitor' | 'rain'>('monitor');
	gemScale = $state(100);
	fadeDuration = $state(3600); // 1 hour default, in seconds
	maxGemsPerColumn = $state(MAX_GEMS_PER_WORKER);
	showInfo = $state(true);
	columnOrder = $state<string[]>([]);

	private ws: WebSocket | null = null;
	private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
	private backoffMs = 1000;
	private pendingEvents: UiEvent[] = [];
	private flushTimer: ReturnType<typeof setTimeout> | null = null;

	connect(url?: string) {
		if (url) this.serverUrl = url;
		this.doConnect();
	}

	disconnect() {
		if (this.reconnectTimer) {
			clearTimeout(this.reconnectTimer);
			this.reconnectTimer = null;
		}
		if (this.flushTimer) {
			clearTimeout(this.flushTimer);
			this.flushTimer = null;
		}
		this.pendingEvents = [];
		if (this.ws) {
			this.ws.close();
			this.ws = null;
		}
		this.connected = false;
	}

	private doConnect() {
		// Cancel any pending reconnect
		if (this.reconnectTimer) {
			clearTimeout(this.reconnectTimer);
			this.reconnectTimer = null;
		}

		// Disarm and close old socket (prevent its onclose from triggering reconnect)
		if (this.ws) {
			this.ws.onclose = null;
			this.ws.onerror = null;
			this.ws.onmessage = null;
			this.ws.close();
			this.ws = null;
		}

		try {
			this.ws = new WebSocket(this.serverUrl);
		} catch {
			this.scheduleReconnect();
			return;
		}

		this.ws.onopen = () => {
			this.connected = true;
			this.backoffMs = 1000;
		};

		this.ws.onclose = () => {
			this.connected = false;
			this.scheduleReconnect();
		};

		this.ws.onerror = () => {
			this.connected = false;
		};

		this.ws.onmessage = (e) => {
			try {
				const event: UiEvent = JSON.parse(e.data);
				// Drop if too many pending — prevents browser freeze
				if (this.pendingEvents.length < 100) {
					this.pendingEvents.push(event);
					this.scheduleFlush();
				}
			} catch { /* ignore parse errors */ }
		};
	}

	private scheduleReconnect() {
		if (this.reconnectTimer) return;
		this.reconnectTimer = setTimeout(() => {
			this.reconnectTimer = null;
			this.backoffMs = Math.min(this.backoffMs * 2, 30000);
			this.doConnect();
		}, this.backoffMs);
	}

	private scheduleFlush() {
		if (this.flushTimer) return;
		// Flush at 2 Hz max — worker stats don't need 60fps updates
		this.flushTimer = setTimeout(() => {
			this.flushTimer = null;
			this.flushEvents();
		}, 500);
	}

	private flushEvents() {
		const events = this.pendingEvents;
		this.pendingEvents = [];
		if (events.length === 0) return;

		// Deduplicate: for WorkerEvent/Progress, only keep the latest per worker
		const latestProgress = new Map<string, WorkerMessage>();
		const filtered: UiEvent[] = [];
		for (const event of events) {
			if (event.type === 'WorkerEvent' && event.event?.type === 'Progress') {
				latestProgress.set(event.worker_id, event.event);
			} else {
				filtered.push(event);
			}
		}
		// Re-add only the latest progress per worker
		for (const [workerId, msg] of latestProgress) {
			filtered.push({ type: 'WorkerEvent', worker_id: workerId, event: msg });
		}

		// Mutate a single map copy, assign once at the end
		const map = new Map(this.workers);
		let columnOrderChanged = false;

		for (const event of filtered) {
			switch (event.type) {
				case 'WorkerConnected': {
					map.set(event.worker_id, {
						workerId: event.worker_id,
						keyId: event.key_id ?? '',
						n: event.n ?? 0,
						strategy: event.strategy ?? '',
						metadata: event.metadata ?? null,
						connected: true,
						verified: event.verified ?? false,
						iteration: 0,
						maxIters: 0,
						violationScore: 0,
						currentGraph6: '',
						discoveriesSoFar: 0,
						round: 0,
						lastRoundMs: 0,
						totalSubmitted: 0,
						totalAdmitted: 0,
						buffered: 0,
						bestGems: [],
					});
					if (!this.columnOrder.includes(event.worker_id)) {
						this.columnOrder = [...this.columnOrder, event.worker_id];
						columnOrderChanged = true;
					}
					break;
				}
				case 'WorkerDisconnected': {
					const w = map.get(event.worker_id);
					if (w) map.set(event.worker_id, { ...w, connected: false });
					break;
				}
				case 'WorkerEvent': {
					if (event.event) {
						this.applyWorkerMessage(map, event.worker_id, event.event);
					}
					break;
				}
			}
		}

		// Single reactive assignment
		this.workers = map;
	}

	private applyWorkerMessage(map: Map<string, WorkerState>, workerId: string, msg: WorkerMessage) {
		const w = map.get(workerId);
		if (!w) return;

		switch (msg.type) {
			case 'Progress': {
				map.set(workerId, {
					...w,
					iteration: msg.iteration ?? w.iteration,
					maxIters: msg.max_iters ?? w.maxIters,
					violationScore: msg.violation_score ?? w.violationScore,
					currentGraph6: msg.current_graph6 ?? w.currentGraph6,
					discoveriesSoFar: msg.discoveries_so_far ?? w.discoveriesSoFar,
				});
				break;
			}
			case 'Discovery': {
				const cid = msg.cid ?? '';
				const gems = [...w.bestGems];

				// Check if this CID already exists in the pool
				const existingIdx = gems.findIndex(g => g.cid === cid);
				if (existingIdx !== -1) {
					// Already have this graph — just refresh its timestamp
					gems[existingIdx] = { ...gems[existingIdx], lastUpdated: Date.now() };
					map.set(workerId, { ...w, bestGems: gems });
					break;
				}

				// New discovery — insert sorted by score
				const gem: RainGemData = {
					graph6: msg.graph6 ?? '',
					cid,
					n: w.n,
					goodmanGap: msg.goodman_gap ?? 0,
					autOrder: msg.aut_order ?? 1,
					scoreHex: msg.score_hex ?? '',
					histogram: (msg.histogram ?? []).map(([k, red, blue]) => ({ k, red, blue })),
					workerId,
					iteration: msg.iteration ?? 0,
					lastUpdated: Date.now(),
				};

				const insertIdx = gems.findIndex(g => gem.scoreHex < g.scoreHex);
				if (insertIdx === -1) {
					gems.push(gem);
				} else {
					gems.splice(insertIdx, 0, gem);
				}
				map.set(workerId, { ...w, bestGems: gems.slice(0, this.maxGemsPerColumn) });
				break;
			}
			case 'RoundComplete': {
				map.set(workerId, {
					...w,
					round: msg.round ?? w.round,
					lastRoundMs: msg.duration_ms ?? w.lastRoundMs,
					totalSubmitted: w.totalSubmitted + (msg.submitted ?? 0),
					totalAdmitted: w.totalAdmitted + (msg.admitted ?? 0),
					buffered: msg.buffered ?? w.buffered,
				});
				break;
			}
		}
	}

	// Persistence
	saveSettings() {
		if (typeof localStorage === 'undefined') return;
		localStorage.setItem('mg-dash-url', this.serverUrl);
		localStorage.setItem('mg-dash-scale', String(this.gemScale));
		localStorage.setItem('mg-dash-fade', String(this.fadeDuration));
		localStorage.setItem('mg-dash-gems', String(this.maxGemsPerColumn));
		localStorage.setItem('mg-dash-mode', this.mode);
		localStorage.setItem('mg-dash-info', String(this.showInfo));
	}

	loadSettings() {
		if (typeof localStorage === 'undefined') return;
		this.serverUrl = localStorage.getItem('mg-dash-url') ?? this.serverUrl;
		this.gemScale = Number(localStorage.getItem('mg-dash-scale')) || this.gemScale;
		this.fadeDuration = Number(localStorage.getItem('mg-dash-fade')) || this.fadeDuration;
		this.maxGemsPerColumn = Number(localStorage.getItem('mg-dash-gems')) || this.maxGemsPerColumn;
		this.mode = (localStorage.getItem('mg-dash-mode') as 'monitor' | 'rain') || this.mode;
		this.showInfo = localStorage.getItem('mg-dash-info') !== 'false';
	}
}

export const store = new DashboardStore();
