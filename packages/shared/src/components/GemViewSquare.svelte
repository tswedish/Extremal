<script lang="ts">
	import { decodeGraph6 } from '../graph6';

	let {
		graph6,
		n,
		size = 60,
		cid = '',
		goodmanGap = 0,
		autOrder = 1,
		opacity = 1,
		glowing = false,
		invalid = false,
		onclick = undefined as (() => void) | undefined,
	}: {
		graph6: string;
		n: number;
		size?: number;
		cid?: string;
		goodmanGap?: number;
		autOrder?: number;
		opacity?: number;
		glowing?: boolean;
		invalid?: boolean;
		onclick?: (() => void) | undefined;
	} = $props();

	let canvas = $state<HTMLCanvasElement | null>(null);
	const matrix = $derived(decodeGraph6(graph6));

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

	$effect(() => {
		if (!canvas || !matrix || matrix.length === 0) return;
		const nn = matrix.length;
		const dpr = typeof window !== 'undefined' ? (window.devicePixelRatio || 1) : 1;
		const W = Math.round(size * dpr);
		canvas.width = W;
		canvas.height = W;
		const ctx = canvas.getContext('2d')!;

		const baseHue = cid ? cidToHue(cid) : g6ToHue(graph6);
		const waveAngle = cid ? cidToAngle(cid) * Math.PI : 0.7;

		let colorCount: number;
		if      (autOrder <= 1)  colorCount = 1;
		else if (autOrder <= 2)  colorCount = 2;
		else if (autOrder <= 5)  colorCount = 3;
		else if (autOrder <= 15) colorCount = 4;
		else if (autOrder <= 50) colorCount = 5;
		else                     colorCount = 6;
		const hues = buildPalette(baseHue, colorCount);

		const waveFreq = goodmanGap * 1.5;
		const waveAmp = goodmanGap === 0 ? 0 : Math.min(0.40, 0.15 + goodmanGap * 0.06);
		const waveDx = Math.cos(waveAngle);
		const waveDy = Math.sin(waveAngle);

		function cellHash(i: number, j: number): number {
			const v = ((i * 2654435761) ^ (j * 2246822519)) >>> 0;
			return (v / 4294967296) - 0.5;
		}

		// Square grid: cells fill the entire canvas
		const cell = W / nn;

		const nonEdgeRgb = hslToRgb(baseHue + 0.5, 0.10, 0.08);
		const bgStr = '#08080e';
		const spineStr = rgb(hslToRgb(baseHue, 0.25, 0.18));

		ctx.fillStyle = bgStr;
		ctx.fillRect(0, 0, W, W);

		for (let i = 0; i < nn; i++) {
			for (let j = 0; j < nn; j++) {
				const px = j * cell;
				const py = i * cell;

				if (i === j) {
					ctx.fillStyle = spineStr;
				} else if (!matrix[i][j]) {
					ctx.fillStyle = rgb(nonEdgeRgb);
				} else {
					const diag = (i + j) / (2 * (nn - 1));
					const hueIdx = Math.floor(diag * hues.length * 0.999);
					const h = hues[Math.min(hueIdx, hues.length - 1)];
					const drift = cellHash(i, j) * 0.1;
					const finalHue = h + drift;

					const normX = j / (nn - 1);
					const normY = i / (nn - 1);
					const proj = normX * waveDx + normY * waveDy;
					const wave = Math.sin(proj * waveFreq * Math.PI * 2);
					const lightness = 0.55 + wave * waveAmp;

					ctx.fillStyle = rgb(hslToRgb(finalHue, 0.80, lightness));
				}

				ctx.fillRect(px, py, cell + 0.5, cell + 0.5);
			}
		}
	});
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
	class="gem-square"
	class:glowing
	class:invalid
	class:clickable={!!onclick}
	style="width: {size}px; height: {size}px; opacity: {opacity}"
	onclick={onclick}
>
	<canvas bind:this={canvas} style="width: {size}px; height: {size}px"></canvas>
</div>

<style>
	.gem-square {
		display: inline-block;
		position: relative;
		transition: opacity 0.3s ease;
	}
	.gem-square canvas {
		display: block;
		border-radius: 2px;
	}
	.gem-square.clickable {
		cursor: pointer;
	}
	.gem-square.clickable:hover {
		filter: brightness(1.15);
	}
	.gem-square.glowing {
		animation: gem-glow 1s ease-out;
	}
	.gem-square.invalid {
		border: 1px solid rgba(239, 68, 68, 0.4);
		border-radius: 2px;
	}
	@keyframes gem-glow {
		0% { box-shadow: 0 0 12px rgba(99, 102, 241, 0.8), 0 0 24px rgba(168, 85, 247, 0.4); }
		100% { box-shadow: none; }
	}
</style>
