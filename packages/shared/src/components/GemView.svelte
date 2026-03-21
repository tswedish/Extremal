<script lang="ts">
	import { decodeGraph6 } from '../graph6';

	let {
		graph6,
		n,
		size = 360,
		label = '',
		cid = '',
		goodmanGap = 0,
		autOrder = 1,
		histogram = [] as { k: number; red: number; blue: number }[],
	}: {
		graph6: string;
		n: number;
		size?: number;
		label?: string;
		cid?: string;
		goodmanGap?: number;
		autOrder?: number;
		histogram?: { k: number; red: number; blue: number }[];
	} = $props();

	let canvas = $state<HTMLCanvasElement | null>(null);
	const matrix = $derived(decodeGraph6(graph6));

	// ── Hash derivation ───────────────────────────────────
	function cidToHue(cid: string): number {
		if (cid.length < 6) return 0.55;
		return (parseInt(cid.slice(0, 6), 16) % 100000) / 100000;
	}

	function cidToAngle(cid: string): number {
		if (cid.length < 12) return 0.3;
		return (parseInt(cid.slice(6, 12), 16) % 100000) / 100000;
	}

	function g6ToHue(s: string): number {
		let h = 5381;
		for (let i = 0; i < s.length; i++) h = ((h << 5) + h + s.charCodeAt(i)) >>> 0;
		return (h % 100000) / 100000;
	}

	// ── Color helpers ─────────────────────────────────────
	function hslToRgb(h: number, s: number, l: number): [number, number, number] {
		h = ((h % 1) + 1) % 1;
		const c = (1 - Math.abs(2 * l - 1)) * s;
		const x = c * (1 - Math.abs(((h * 6) % 2) - 1));
		const m = l - c / 2;
		let r = 0, g = 0, b = 0;
		if      (h < 1/6) { r = c; g = x; }
		else if (h < 2/6) { r = x; g = c; }
		else if (h < 3/6) { g = c; b = x; }
		else if (h < 4/6) { g = x; b = c; }
		else if (h < 5/6) { r = x; b = c; }
		else              { r = c; b = x; }
		return [Math.round((r+m)*255), Math.round((g+m)*255), Math.round((b+m)*255)];
	}

	function rgb(c: [number, number, number]): string {
		return `rgb(${c[0]},${c[1]},${c[2]})`;
	}

	function buildPalette(baseHue: number, count: number): number[] {
		if (count <= 1) return [baseHue];
		const hues: number[] = [];
		for (let i = 0; i < count; i++) hues.push((baseHue + i / count) % 1);
		return hues;
	}

	// ── Render ────────────────────────────────────────────
	$effect(() => {
		if (!canvas || !matrix || matrix.length === 0) return;
		const nn = matrix.length;
		const dpr = typeof window !== 'undefined' ? (window.devicePixelRatio || 1) : 1;
		const W = Math.round(size * dpr);
		canvas.width = W;
		canvas.height = W;
		const ctx = canvas.getContext('2d')!;

		// Identity
		const baseHue = cid ? cidToHue(cid) : g6ToHue(graph6);
		const waveAngle = cid ? cidToAngle(cid) * Math.PI : 0.7;

		// Aut-driven palette richness
		let colorCount: number;
		if      (autOrder <= 1)  colorCount = 1;
		else if (autOrder <= 2)  colorCount = 2;
		else if (autOrder <= 5)  colorCount = 3;
		else if (autOrder <= 15) colorCount = 4;
		else if (autOrder <= 50) colorCount = 5;
		else                     colorCount = 6;
		const hues = buildPalette(baseHue, colorCount);

		// Goodman gap -> lightness wave
		const waveFreq = goodmanGap * 1.5;
		const waveAmp = goodmanGap === 0 ? 0 : Math.min(0.40, 0.15 + goodmanGap * 0.06);
		const waveDx = Math.cos(waveAngle);
		const waveDy = Math.sin(waveAngle);

		// Histogram asymmetry -> hue noise
		// Use the highest-k tier's |red - blue| as the "drama" value
		let maxDiff = 0;
		for (const tier of histogram) {
			maxDiff = Math.max(maxDiff, Math.abs(tier.red - tier.blue));
		}
		const hueNoise = Math.min(0.5, maxDiff * 0.10);

		function cellHash(i: number, j: number): number {
			const v = ((i * 2654435761) ^ (j * 2246822519)) >>> 0;
			return (v / 4294967296) - 0.5;
		}

		// Grid
		const gridW = 2 * nn - 1;
		const margin = W * 0.03;
		const cell = (W - 2 * margin) / gridW;

		const nonEdgeRgb = hslToRgb(baseHue + 0.5, 0.10, 0.08);
		const bgStr = '#08080e';
		const spineStr = rgb(hslToRgb(baseHue, 0.25, 0.18));
		const outlineStr = rgb(hslToRgb(baseHue, 0.40, 0.25));

		ctx.fillStyle = bgStr;
		ctx.fillRect(0, 0, W, W);

		for (let i = 0; i < nn; i++) {
			for (let j = 0; j < nn; j++) {
				const gx = j - i + (nn - 1);
				const gy = i + j;
				const px = margin + gx * cell;
				const py = margin + gy * cell;

				if (i === j) {
					ctx.fillStyle = spineStr;
				} else if (!matrix[i][j]) {
					ctx.fillStyle = rgb(nonEdgeRgb);
				} else {
					const diag = (i + j) / (2 * (nn - 1));
					const hueIdx = Math.floor(diag * hues.length * 0.999);
					const h = hues[Math.min(hueIdx, hues.length - 1)];
					const drift = cellHash(i, j) * hueNoise;
					const finalHue = h + drift;

					const normX = gx / (gridW - 1);
					const normY = gy / (gridW - 1);
					const proj = normX * waveDx + normY * waveDy;
					const wave = Math.sin(proj * waveFreq * Math.PI * 2);
					const lightness = 0.55 + wave * waveAmp;

					ctx.fillStyle = rgb(hslToRgb(finalHue, 0.80, lightness));
				}

				ctx.fillRect(px, py, cell + 0.5, cell + 0.5);
			}
		}

		// Diamond outline
		const topX = margin + (nn - 1) * cell + cell / 2, topY = margin;
		const rightX = margin + gridW * cell, rightY = margin + (nn - 1) * cell + cell / 2;
		const bottomX = topX, bottomY = margin + gridW * cell;
		const leftX = margin, leftY = rightY;

		ctx.strokeStyle = outlineStr;
		ctx.lineWidth = Math.max(1, W / 200);
		ctx.beginPath();
		ctx.moveTo(topX, topY);
		ctx.lineTo(rightX, rightY);
		ctx.lineTo(bottomX, bottomY);
		ctx.lineTo(leftX, leftY);
		ctx.closePath();
		ctx.stroke();

		// Spine
		ctx.lineWidth = Math.max(0.5, W / 400);
		ctx.beginPath();
		ctx.moveTo(topX, topY);
		ctx.lineTo(bottomX, bottomY);
		ctx.stroke();
	});
</script>

<div class="gem-container">
	<canvas bind:this={canvas} style="width: {size}px; height: {size}px" class="gem-canvas"></canvas>
	{#if label}
		<div class="gem-label">{label}</div>
	{/if}
</div>

<style>
	.gem-container {
		display: inline-flex;
		flex-direction: column;
		align-items: center;
		gap: 0.5rem;
	}
	.gem-canvas {
		border-radius: 0.75rem;
		border: 1px solid rgba(99, 102, 241, 0.15);
		display: block;
	}
	.gem-label {
		font-size: 0.7rem;
		color: #8888a0;
		font-family: 'JetBrains Mono', monospace;
		text-align: center;
		max-width: 100%;
		overflow: hidden;
		text-overflow: ellipsis;
	}
</style>
