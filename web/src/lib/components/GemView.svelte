<script lang="ts">
	import type { RgxfJson } from '$lib/api';
	import { decodeRgxf } from '$lib/rgxf';

	let {
		rgxf,
		size = 360,
		label = '',
		graphCid = '',
		goodmanGap = 0,
		cMax = 0,
		cMin = 0,
		autOrder = 1,
	}: {
		rgxf: RgxfJson;
		size?: number;
		label?: string;
		graphCid?: string;
		/** Goodman gap — 0 = flat lightness, higher = more lightness bands */
		goodmanGap?: number;
		cMax?: number;
		cMin?: number;
		autOrder?: number;
	} = $props();

	let canvas = $state<HTMLCanvasElement | null>(null);
	const matrix = $derived(decodeRgxf(rgxf));

	function cidToHue(cid: string): number {
		const hex = cid.replace(/^sha256-/, '');
		if (hex.length < 6) return 0.55;
		return (parseInt(hex.slice(0, 6), 16) % 100000) / 100000;
	}

	// Second hash value from CID for wave direction
	function cidToAngle(cid: string): number {
		const hex = cid.replace(/^sha256-/, '');
		if (hex.length < 12) return 0.3;
		return (parseInt(hex.slice(6, 12), 16) % 100000) / 100000;
	}

	function bitsToHue(data: RgxfJson): number {
		const raw = atob(data.bits_b64);
		let h = 5381;
		for (let i = 0; i < raw.length; i++) h = ((h << 5) + h + raw.charCodeAt(i)) >>> 0;
		return (h % 100000) / 100000;
	}

	function hslToRgb(h: number, s: number, l: number): [number, number, number] {
		h = ((h % 1) + 1) % 1;
		const c = (1 - Math.abs(2 * l - 1)) * s;
		const x = c * (1 - Math.abs(((h * 6) % 2) - 1));
		const m = l - c / 2;
		let r = 0, g = 0, b = 0;
		if (h < 1/6)      { r = c; g = x; }
		else if (h < 2/6) { r = x; g = c; }
		else if (h < 3/6) { g = c; b = x; }
		else if (h < 4/6) { g = x; b = c; }
		else if (h < 5/6) { r = x; b = c; }
		else              { r = c; b = x; }
		return [
			Math.round((r + m) * 255),
			Math.round((g + m) * 255),
			Math.round((b + m) * 255)
		];
	}

	function rgb(c: [number, number, number]): string {
		return `rgb(${c[0]},${c[1]},${c[2]})`;
	}

	// Build evenly-spaced hue palette around base hue
	function buildPalette(baseHue: number, count: number): number[] {
		if (count <= 1) return [baseHue];
		const hues: number[] = [];
		for (let i = 0; i < count; i++) {
			hues.push((baseHue + i / count) % 1);
		}
		return hues;
	}

	$effect(() => {
		if (!canvas) return;
		const n = rgxf.n;
		const adj = matrix;
		const dpr = window.devicePixelRatio || 1;
		const W = Math.round(size * dpr);
		canvas.width = W;
		canvas.height = W;
		const ctx = canvas.getContext('2d')!;

		if (n === 0) { ctx.fillStyle = '#0a0a12'; ctx.fillRect(0, 0, W, W); return; }

		const baseHue = graphCid ? cidToHue(graphCid) : bitsToHue(rgxf);
		const waveAngle = graphCid ? cidToAngle(graphCid) * Math.PI : 0.7;

		// Palette: autOrder drives color count
		// aut=1 → 1 (monochrome), 2 → 2 (complementary), 3-5 → 3 (triadic), etc.
		let colorCount: number;
		if (autOrder <= 1) colorCount = 1;
		else if (autOrder <= 2) colorCount = 2;
		else if (autOrder <= 5) colorCount = 3;
		else if (autOrder <= 15) colorCount = 4;
		else if (autOrder <= 50) colorCount = 5;
		else colorCount = 6;

		const hues = buildPalette(baseHue, colorCount);

		// Wave: goodman gap controls spatial sine frequency for lightness
		// gap=0 → flat (no bands), gap=N → N visible bands
		const waveFreq = goodmanGap * 1.5;
		const waveAmp = goodmanGap === 0 ? 0 : Math.min(0.40, 0.15 + goodmanGap * 0.06);
		const waveDx = Math.cos(waveAngle);
		const waveDy = Math.sin(waveAngle);

		// Hue noise from |cMax - cMin|: how much each cell's hue drifts from palette
		// diff=0 → 0 drift (monochrome for aut=1, clean palette otherwise)
		// higher diff → noisier, more scattered hue
		const diff = Math.abs(cMax - cMin);
		const hueNoise = Math.min(0.5, diff * 0.10);

		// Deterministic per-cell hash for hue noise (returns -0.5..+0.5)
		function cellHash(i: number, j: number): number {
			const v = ((i * 2654435761) ^ (j * 2246822519)) >>> 0;
			return (v / 4294967296) - 0.5;
		}

		const gridW = 2 * n - 1;
		const margin = W * 0.03;
		const cell = (W - 2 * margin) / gridW;

		const nonEdgeRgb = hslToRgb(baseHue + 0.5, 0.10, 0.08);
		const bgStr = '#0a0a10';
		const spineStr = rgb(hslToRgb(baseHue, 0.25, 0.18));
		const outlineStr = rgb(hslToRgb(baseHue, 0.40, 0.25));

		ctx.fillStyle = bgStr;
		ctx.fillRect(0, 0, W, W);

		for (let i = 0; i < n; i++) {
			for (let j = 0; j < n; j++) {
				const gx = j - i + (n - 1);
				const gy = i + j;
				const px = margin + gx * cell;
				const py = margin + gy * cell;

				if (i === j) {
					ctx.fillStyle = spineStr;
				} else if (!adj[i][j]) {
					ctx.fillStyle = rgb(nonEdgeRgb);
				} else {
					// Pick base hue from palette by diagonal position
					const diag = (i + j) / (2 * (n - 1));
					const hueIdx = Math.floor(diag * hues.length * 0.999);
					const h = hues[Math.min(hueIdx, hues.length - 1)];

				// Add per-cell hue noise scaled by |cMax - cMin|
				const drift = cellHash(i, j) * hueNoise;
				const finalHue = h + drift;

				// Lightness wave from goodman gap
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
		const topX = margin + (n - 1) * cell + cell / 2, topY = margin;
		const rightX = margin + gridW * cell, rightY = margin + (n - 1) * cell + cell / 2;
		const bottomX = topX, bottomY = margin + gridW * cell;
		const leftX = margin, leftY = rightY;

		ctx.strokeStyle = outlineStr;
		ctx.lineWidth = Math.max(1, W / 200);
		ctx.beginPath();
		ctx.moveTo(topX, topY); ctx.lineTo(rightX, rightY);
		ctx.lineTo(bottomX, bottomY); ctx.lineTo(leftX, leftY);
		ctx.closePath(); ctx.stroke();

		// Spine
		ctx.lineWidth = Math.max(0.5, W / 400);
		ctx.beginPath(); ctx.moveTo(topX, topY); ctx.lineTo(bottomX, bottomY); ctx.stroke();
	});
</script>

<div class="gem-container">
	<canvas
		bind:this={canvas}
		style="width: {size}px; height: {size}px"
		class="gem-canvas"
	></canvas>
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
		border-radius: 0.5rem;
		border: 1px solid rgba(99, 102, 241, 0.2);
		display: block;
	}
	.gem-label {
		font-size: 0.75rem;
		color: #8888a0;
		font-family: 'JetBrains Mono', monospace;
		text-align: center;
	}
</style>
