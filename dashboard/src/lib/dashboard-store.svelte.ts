// Dashboard state management — connects to relay server via WebSocket

import type { RainGemData } from '@minegraph/shared';

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
	gemScale = $state(60);
	fadeDuration = $state(120);
	maxGemsPerColumn = $state(MAX_GEMS_PER_WORKER);
	showInfo = $state(true);
	columnOrder = $state<string[]>([]);

	private ws: WebSocket | null = null;
	private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
	private backoffMs = 1000;

	connect(url?: string) {
		if (url) this.serverUrl = url;
		this.doConnect();
	}

	disconnect() {
		if (this.reconnectTimer) {
			clearTimeout(this.reconnectTimer);
			this.reconnectTimer = null;
		}
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
				this.handleEvent(event);
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

	private handleEvent(event: UiEvent) {
		switch (event.type) {
			case 'WorkerConnected': {
				const state: WorkerState = {
					workerId: event.worker_id,
					keyId: event.key_id ?? '',
					n: event.n ?? 0,
					strategy: event.strategy ?? '',
					metadata: event.metadata ?? null,
					connected: true,
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
				};
				const newMap = new Map(this.workers);
				newMap.set(event.worker_id, state);
				this.workers = newMap;
				if (!this.columnOrder.includes(event.worker_id)) {
					this.columnOrder = [...this.columnOrder, event.worker_id];
				}
				break;
			}
			case 'WorkerDisconnected': {
				const w = this.workers.get(event.worker_id);
				if (w) {
					const newMap = new Map(this.workers);
					newMap.set(event.worker_id, { ...w, connected: false });
					this.workers = newMap;
				}
				break;
			}
			case 'WorkerEvent': {
				if (event.event) {
					this.handleWorkerMessage(event.worker_id, event.event);
				}
				break;
			}
		}
	}

	private handleWorkerMessage(workerId: string, msg: WorkerMessage) {
		const w = this.workers.get(workerId);
		if (!w) return;

		const newMap = new Map(this.workers);

		switch (msg.type) {
			case 'Progress': {
				newMap.set(workerId, {
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
				const gem: RainGemData = {
					graph6: msg.graph6 ?? '',
					cid: msg.cid ?? '',
					n: w.n,
					goodmanGap: msg.goodman_gap ?? 0,
					autOrder: msg.aut_order ?? 1,
					scoreHex: msg.score_hex ?? '',
					histogram: (msg.histogram ?? []).map(([k, red, blue]) => ({ k, red, blue })),
					workerId,
					iteration: msg.iteration ?? 0,
					lastUpdated: Date.now(),
				};

				// Insert into sorted bestGems (lower score_hex = better)
				const gems = [...w.bestGems];
				const insertIdx = gems.findIndex(g => gem.scoreHex < g.scoreHex);
				if (insertIdx === -1) {
					gems.push(gem);
				} else {
					gems.splice(insertIdx, 0, gem);
				}
				// Cap at max
				const trimmed = gems.slice(0, this.maxGemsPerColumn);

				newMap.set(workerId, { ...w, bestGems: trimmed });
				break;
			}
			case 'RoundComplete': {
				newMap.set(workerId, {
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

		this.workers = newMap;
	}

	// Persistence
	saveSettings() {
		if (typeof localStorage === 'undefined') return;
		localStorage.setItem('mg-dash-url', this.serverUrl);
		localStorage.setItem('mg-dash-scale', String(this.gemScale));
		localStorage.setItem('mg-dash-fade', String(this.fadeDuration));
		localStorage.setItem('mg-dash-mode', this.mode);
		localStorage.setItem('mg-dash-info', String(this.showInfo));
	}

	loadSettings() {
		if (typeof localStorage === 'undefined') return;
		this.serverUrl = localStorage.getItem('mg-dash-url') ?? this.serverUrl;
		this.gemScale = Number(localStorage.getItem('mg-dash-scale')) || this.gemScale;
		this.fadeDuration = Number(localStorage.getItem('mg-dash-fade')) || this.fadeDuration;
		this.mode = (localStorage.getItem('mg-dash-mode') as 'monitor' | 'rain') || this.mode;
		this.showInfo = localStorage.getItem('mg-dash-info') !== 'false';
	}
}

export const store = new DashboardStore();
