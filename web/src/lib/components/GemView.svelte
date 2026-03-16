<script lang="ts">
	import type { RgxfJson } from '$lib/api';
	import { decodeRgxf } from '$lib/rgxf';

	let {
		rgxf,
		size = 360,
		label = ''
	}: {
		rgxf: RgxfJson;
		size?: number;
		label?: string;
	} = $props();

	let canvas = $state<HTMLCanvasElement | null>(null);

	const matrix = $derived(decodeRgxf(rgxf));

	// ── Deterministic hash-derived palette ──────────────────────
	function hashBytes(data: Uint8Array): Uint8Array {
		// Simple deterministic hash (djb2 variant → spread into 32 bytes)
		let h = 5381;
		for (let i = 0; i < data.length; i++) {
			h = ((h << 5) + h + data[i]) >>> 0;
		}
		const out = new Uint8Array(32);
		for (let i = 0; i < 32; i++) {
			h = ((h << 5) + h + i) >>> 0;
			out[i] = h & 0xff;
		}
		return out;
	}

	function hashFloats(seed: Uint8Array, count: number): number[] {
		const out: number[] = [];
		let cur = seed;
		while (out.length < count) {
			cur = hashBytes(cur);
			for (let i = 0; i < cur.length - 3 && out.length < count; i += 4) {
				const v = ((cur[i] << 24) | (cur[i+1] << 16) | (cur[i+2] << 8) | cur[i+3]) >>> 0;
				out.push(v / 0x100000000);
			}
		}
		return out;
	}

	function hsl(h: number, s: number, l: number): [number, number, number] {
		h = ((h % 1) + 1) % 1;
		s = Math.max(0, Math.min(1, s));
		l = Math.max(0, Math.min(1, l));
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

	function makePalette(rgxfData: RgxfJson) {
		// Hash the packed bits to derive a deterministic palette
		const raw = atob(rgxfData.bits_b64);
		const bytes = new Uint8Array(raw.length);
		for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
		const seed = hashBytes(bytes);
		const v = hashFloats(seed, 16);

		const baseH = v[0];
		const compH = (baseH + 0.45 + 0.10 * v[1]) % 1.0;

		return {
			edgeDark:   hsl(baseH, 0.50 + 0.15 * v[6], 0.30 + 0.10 * v[7]),
			edgeMain:   hsl(baseH, 0.70 + 0.20 * v[2], 0.55 + 0.15 * v[3]),
			edgeBright: hsl(baseH, 0.60 + 0.20 * v[4], 0.75 + 0.15 * v[5]),
			nonEdge:    hsl(compH, 0.15 + 0.10 * v[8], 0.06 + 0.03 * v[9]),
			bg:         hsl(compH, 0.12, 0.03),
			spine:      hsl(baseH, 0.25, 0.12 + 0.04 * v[10]),
			glow:       hsl(baseH, 0.50 + 0.20 * v[11], 0.40 + 0.10 * v[12]),
			grid:       hsl(baseH, 0.15, 0.10 + 0.04 * v[13]),
			outline:    hsl(baseH, 0.35, 0.18 + 0.06 * v[14]),
		};
	}

	function rgb(c: [number, number, number]): string {
		return `rgb(${c[0]},${c[1]},${c[2]})`;
	}

	// ── Render ──────────────────────────────────────────────────
	$effect(() => {
		if (!canvas) return;
		const n = rgxf.n;
		const adj = matrix;
		const pal = makePalette(rgxf);
		const dpr = window.devicePixelRatio || 1;

		canvas.width = size * dpr;
		canvas.height = size * dpr;
		const ctx = canvas.getContext('2d')!;
		ctx.scale(dpr, dpr);

		ctx.fillStyle = rgb(pal.bg);
		ctx.fillRect(0, 0, size, size);

		if (n === 0) return;

		const gridW = 2 * n - 1;
		const margin = size * 0.06;
		const cellSize = (size - 2 * margin) / gridW;

		// ── Render sharp diamond to offscreen canvas ──
		const offscreen = document.createElement('canvas');
		offscreen.width = canvas.width;
		offscreen.height = canvas.height;
		const off = offscreen.getContext('2d')!;
		off.scale(dpr, dpr);
		off.fillStyle = rgb(pal.bg);
		off.fillRect(0, 0, size, size);

		for (let i = 0; i < n; i++) {
			for (let j = 0; j < n; j++) {
				const rx = j - i + (n - 1);
				const ry = i + j;
				const px = margin + rx * cellSize;
				const py = margin + ry * cellSize;
				const cs = cellSize + 0.5;

				if (i === j) {
					off.fillStyle = rgb(pal.spine);
				} else if (adj[i][j]) {
					// Edge: gradient from dark to bright across the diamond
					const t = (i + j) / (2 * (n - 1));
					if (t < 0.33) off.fillStyle = rgb(pal.edgeDark);
					else if (t < 0.66) off.fillStyle = rgb(pal.edgeMain);
					else off.fillStyle = rgb(pal.edgeBright);
				} else {
					off.fillStyle = rgb(pal.nonEdge);
				}
				off.fillRect(px, py, cs, cs);
			}
		}

		// ── Grid lines ──
		off.strokeStyle = rgb(pal.grid);
		off.lineWidth = 0.5;
		for (let k = 0; k <= gridW; k++) {
			const pos = margin + k * cellSize;
			off.beginPath();
			off.moveTo(margin, pos);
			off.lineTo(margin + gridW * cellSize, pos);
			off.stroke();
			off.beginPath();
			off.moveTo(pos, margin);
			off.lineTo(pos, margin + gridW * cellSize);
			off.stroke();
		}

		// ── Diamond outline ──
		const topX = margin + (n - 1) * cellSize + cellSize / 2;
		const topY = margin;
		const rightX = margin + gridW * cellSize;
		const rightY = margin + (n - 1) * cellSize + cellSize / 2;
		const bottomX = topX;
		const bottomY = margin + gridW * cellSize;
		const leftX = margin;
		const leftY = rightY;

		off.strokeStyle = rgb(pal.outline);
		off.lineWidth = 1.5;
		off.beginPath();
		off.moveTo(topX, topY);
		off.lineTo(rightX, rightY);
		off.lineTo(bottomX, bottomY);
		off.lineTo(leftX, leftY);
		off.closePath();
		off.stroke();

		// ── Spine line ──
		off.strokeStyle = rgb(pal.outline);
		off.lineWidth = 0.8;
		off.beginPath();
		off.moveTo(topX, topY);
		off.lineTo(bottomX, bottomY);
		off.stroke();

		// ── Composite: glow + sharp ──
		const blurPx = Math.max(1.5, size / 40);
		ctx.filter = `blur(${blurPx}px)`;
		ctx.drawImage(offscreen, 0, 0, size, size);
		ctx.filter = 'none';

		ctx.globalAlpha = 0.65;
		ctx.drawImage(offscreen, 0, 0, size, size);
		ctx.globalAlpha = 1.0;
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
