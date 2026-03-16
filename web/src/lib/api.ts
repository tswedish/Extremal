const BASE = '/api';

// ── Types ────────────────────────────────────────────────────────────

export interface HealthResponse {
	name: string;
	version: string;
	status: string;
}

export interface RgxfJson {
	n: number;
	encoding: string;
	bits_b64: string;
}

export interface VerifyRequest {
	oras_version: string;
	k: number;
	ell: number;
	graph: RgxfJson;
	want_cid: boolean;
}

export interface VerifyResponse {
	status: 'accepted' | 'rejected';
	graph_cid?: string;
	reason?: string;
	witness?: number[];
}

export interface LeaderboardSummary {
	k: number;
	ell: number;
	n: number;
	entry_count: number;
	top_cid: string | null;
	last_updated: string | null;
}

export interface LeaderboardEntry {
	k: number;
	ell: number;
	n: number;
	graph_cid: string;
	rank: number;
	tier1_max: number;
	tier1_min: number;
	goodman_gap: number;
	tier2_aut: number;
	score_json: string;
	key_id: string | null;
	commit_hash: string | null;
	admitted_at: string;
}

export interface LeaderboardDetail {
	k: number;
	ell: number;
	n: number;
	total: number;
	offset: number;
	limit: number;
	entries: LeaderboardEntry[];
	top_graph: RgxfJson | null;
}

export interface ThresholdInfo {
	entry_count: number;
	capacity: number;
	worst_tier1_max: number | null;
	worst_tier1_min: number | null;
	worst_goodman_gap: number | null;
	worst_tier2_aut: number | null;
	worst_tier3_cid: string | null;
}

export interface SubmitRequest {
	k: number;
	ell: number;
	n: number;
	graph: RgxfJson;
}

export interface SubmitResponse {
	graph_cid: string;
	verdict: 'accepted' | 'rejected';
	reason?: string;
	witness?: number[];
	admitted: boolean;
	rank: number | null;
	score: Record<string, unknown> | null;
}

export interface SubmissionDetail {
	graph_cid: string;
	k: number;
	ell: number;
	n: number;
	rgxf: RgxfJson | null;
	submitted_at: string;
	verdict: string | null;
	reason: string | null;
	witness: number[] | null;
	verified_at: string | null;
	leaderboard_rank: number | null;
	score: Record<string, unknown> | null;
	key_id: string | null;
	sig_status: string;
	commit_hash: string | null;
}

export interface KeyLeaderboardEntry {
	k: number;
	ell: number;
	n: number;
	graph_cid: string;
	rank: number;
	tier1_max: number;
	tier1_min: number;
	goodman_gap: number;
	tier2_aut: number;
	admitted_at: string;
}

export interface KeyInfo {
	key_id: string;
	public_key: string;
	display_name: string | null;
	github_repo: string | null;
	created_at: string;
	leaderboard_entries: KeyLeaderboardEntry[];
}

// ── API Functions ────────────────────────────────────────────────────

export async function getHealth(): Promise<HealthResponse> {
	const res = await fetch(`${BASE}/health`);
	return res.json();
}

export async function getLeaderboards(): Promise<LeaderboardSummary[]> {
	const res = await fetch(`${BASE}/leaderboards`);
	const data = await res.json();
	return data.leaderboards;
}

export async function getNValuesForPair(
	k: number,
	ell: number
): Promise<{ k: number; ell: number; n_values: number[] }> {
	const res = await fetch(`${BASE}/leaderboards/${k}/${ell}`);
	if (!res.ok) {
		const err = await res.json();
		throw new Error(err.error || `HTTP ${res.status}`);
	}
	return res.json();
}

export async function getLeaderboard(
	k: number,
	ell: number,
	n: number,
	offset: number = 0,
	limit: number = 50
): Promise<LeaderboardDetail> {
	const res = await fetch(
		`${BASE}/leaderboards/${k}/${ell}/${n}?offset=${offset}&limit=${limit}`
	);
	if (!res.ok) {
		const err = await res.json();
		throw new Error(err.error || `HTTP ${res.status}`);
	}
	return res.json();
}

export async function getLeaderboardGraphs(
	k: number,
	ell: number,
	n: number,
	limit: number = 50,
	offset: number = 0
): Promise<RgxfJson[]> {
	const res = await fetch(
		`${BASE}/leaderboards/${k}/${ell}/${n}/graphs?limit=${limit}&offset=${offset}`
	);
	if (!res.ok) {
		return []; // graceful fallback — thumbnails just won't render
	}
	const data = await res.json();
	return data.graphs;
}

export async function getThreshold(
	k: number,
	ell: number,
	n: number
): Promise<ThresholdInfo> {
	const res = await fetch(`${BASE}/leaderboards/${k}/${ell}/${n}/threshold`);
	if (!res.ok) {
		const err = await res.json();
		throw new Error(err.error || `HTTP ${res.status}`);
	}
	return res.json();
}

export async function submitVerify(req: VerifyRequest): Promise<VerifyResponse> {
	const res = await fetch(`${BASE}/verify`, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify(req)
	});
	if (!res.ok) {
		const err = await res.json();
		throw new Error(err.error || `HTTP ${res.status}`);
	}
	return res.json();
}

export async function submitGraph(req: SubmitRequest): Promise<SubmitResponse> {
	const res = await fetch(`${BASE}/submit`, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify(req)
	});
	if (!res.ok) {
		const err = await res.json();
		throw new Error(err.error || `HTTP ${res.status}`);
	}
	return res.json();
}

export async function getSubmission(cid: string): Promise<SubmissionDetail> {
	const res = await fetch(`${BASE}/submissions/${encodeURIComponent(cid)}`);
	if (!res.ok) {
		const err = await res.json();
		throw new Error(err.error || `HTTP ${res.status}`);
	}
	return res.json();
}

export async function getKeyInfo(keyId: string): Promise<KeyInfo> {
	const res = await fetch(`${BASE}/keys/${encodeURIComponent(keyId)}`);
	if (!res.ok) {
		const err = await res.json();
		throw new Error(err.error || `HTTP ${res.status}`);
	}
	return res.json();
}
