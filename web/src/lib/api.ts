const BASE = '/api';

export interface HealthResponse {
	name: string;
	version: string;
	status: string;
}

export async function getHealth(): Promise<HealthResponse> {
	const res = await fetch(`${BASE}/health`);
	return res.json();
}

export async function getChallenges(): Promise<unknown[]> {
	const res = await fetch(`${BASE}/challenges`);
	const data = await res.json();
	return data.challenges;
}

export async function getRecords(): Promise<unknown[]> {
	const res = await fetch(`${BASE}/records`);
	const data = await res.json();
	return data.records;
}
