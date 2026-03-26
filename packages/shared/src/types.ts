// Shared TypeScript types for Extremal web apps

export interface HistogramTier {
	k: number;
	red: number;
	blue: number;
}

/** A scored discovery from a worker, used in rain columns. */
export interface RainGemData {
	graph6: string;
	cid: string;
	n: number;
	goodmanGap: number;
	autOrder: number;
	scoreHex: string;
	histogram: HistogramTier[];
	workerId: string;
	iteration: number;
	/** Timestamp when this gem was last updated/inserted */
	lastUpdated: number;
}
