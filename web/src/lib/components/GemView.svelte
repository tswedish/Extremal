<script lang="ts">
	// Re-export from shared package — all existing imports continue to work
	import SharedGemView from '@extremal/shared/components/GemView.svelte';

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
</script>

<SharedGemView {graph6} {n} {size} {label} {cid} {goodmanGap} {autOrder} {histogram} />
