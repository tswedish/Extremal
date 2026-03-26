// Extremal v1 API client

const BASE = '/api';

async function get<T>(path: string): Promise<T> {
	const res = await fetch(`${BASE}${path}`);
	if (!res.ok) {
		const body = await res.json().catch(() => ({ error: res.statusText }));
		throw new Error(body.error || res.statusText);
	}
	return res.json();
}

// ── Types ───────────────────────────────────────────────────

export interface HealthResponse {
	name: string;
	version: string;
	status: string;
	db: string;
	server_key_id: string;
}

export interface LeaderboardSummary {
	n: number;
	entry_count: number;
}

export interface LeaderboardEntry {
	rank: number;
	cid: string;
	key_id: string;
	graph6?: string;
	goodman_gap?: number;
	aut_order?: number;
	histogram?: { tiers: { k: number; red: number; blue: number }[] };
	admitted_at: string;
}

export interface LeaderboardDetail {
	n: number;
	total: number;
	entries: LeaderboardEntry[];
	top_graph?: { cid: string; graph6: string; rank: number };
}

export interface LeaderboardGraph {
	rank: number;
	cid: string;
	graph6: string;
}

export interface ThresholdInfo {
	n: number;
	count: number;
	capacity: number;
	threshold_score_bytes: string | null;
}

export interface SubmissionDetail {
	submission: { cid: string; key_id: string; metadata: any; created_at: string };
	graph: { n: number; graph6: string } | null;
	score: { histogram: any; goodman_gap: number; aut_order: number } | null;
	receipt: { server_key_id: string; verdict: string; signature: string; score: any } | null;
}

export interface ServerEvent {
	type: 'admission' | 'submission';
	n?: number;
	cid?: string;
	key_id?: string;
	rank?: number;
	// Enriched fields for admissions (for rain visualization)
	graph6?: string;
	goodman_gap?: number;
	aut_order?: number;
	metadata?: Record<string, any>;
}

// ── API functions ───────────────────────────────────────────

export interface HistorySnapshot {
	t: string;
	count: number;
	total_score: number;
	best_gap: number | null;
	worst_gap: number | null;
	median_gap: number | null;
	avg_gap: number | null;
	best_aut: number | null;
	avg_aut: number | null;
}

export interface IdentityDetail {
	key_id: string;
	public_key: string;
	display_name: string | null;
	github_repo: string | null;
	created_at: string;
}

export async function getHealth(): Promise<HealthResponse> {
	return get('/health');
}

export async function getHistory(n: number, since?: string): Promise<{ snapshots: HistorySnapshot[] }> {
	const params = since ? `?since=${encodeURIComponent(since)}` : '';
	return get(`/leaderboards/${n}/history${params}`);
}

export async function getIdentity(keyId: string): Promise<IdentityDetail> {
	return get(`/keys/${keyId}`);
}

export async function getIdentitySubmissions(keyId: string, limit = 50): Promise<{ submissions: any[] }> {
	return get(`/keys/${keyId}/submissions?limit=${limit}`);
}

export async function getLeaderboards(): Promise<{ leaderboards: LeaderboardSummary[] }> {
	return get('/leaderboards');
}

export async function getLeaderboard(n: number, limit = 50, offset = 0): Promise<LeaderboardDetail> {
	return get(`/leaderboards/${n}?limit=${limit}&offset=${offset}`);
}

export async function getLeaderboardGraphs(n: number, limit = 50, offset = 0): Promise<{ graphs: LeaderboardGraph[] }> {
	return get(`/leaderboards/${n}/graphs?limit=${limit}&offset=${offset}`);
}

export async function getThreshold(n: number): Promise<ThresholdInfo> {
	return get(`/leaderboards/${n}/threshold`);
}

export async function getSubmission(cid: string): Promise<SubmissionDetail> {
	return get(`/submissions/${cid}`);
}

// ── SSE ─────────────────────────────────────────────────────

export function subscribeEvents(onEvent: (event: ServerEvent) => void): () => void {
	const source = new EventSource(`${BASE}/events`);
	source.onmessage = (e) => {
		try {
			const event: ServerEvent = JSON.parse(e.data);
			onEvent(event);
		} catch { /* ignore parse errors */ }
	};
	return () => source.close();
}
