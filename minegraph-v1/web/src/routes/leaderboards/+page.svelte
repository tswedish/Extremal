<script lang="ts">
	import { getLeaderboards, type LeaderboardSummary } from '$lib/api';

	let boards = $state<LeaderboardSummary[]>([]);
	let loading = $state(true);

	$effect(() => {
		getLeaderboards().then(d => { boards = d.leaderboards; loading = false; }).catch(() => { loading = false; });
	});
</script>

<h1>Leaderboards</h1>
<p class="subtitle">Select a vertex count to view ranked graphs.</p>

{#if loading}
	<div class="grid">
		{#each [1,2,3] as _}
			<div class="card shimmer" style="height: 80px;"></div>
		{/each}
	</div>
{:else if boards.length === 0}
	<p class="empty">No leaderboards yet. Start a worker to populate.</p>
{:else}
	<div class="grid">
		{#each boards as board}
			<a href="/leaderboards/{board.n}" class="card board-card">
				<div class="board-n">n = {board.n}</div>
				<div class="board-count">{board.entry_count} graph{board.entry_count !== 1 ? 's' : ''}</div>
			</a>
		{/each}
	</div>
{/if}

<style>
	h1 { font-family: var(--font-mono); font-size: 1.5rem; margin-bottom: 0.3rem; }
	.subtitle { color: var(--color-text-muted); margin-bottom: 1.5rem; }
	.empty { color: var(--color-text-muted); font-style: italic; }
	.grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(180px, 1fr)); gap: 1rem; }
	.board-card { transition: border-color 0.15s; cursor: pointer; }
	.board-card:hover { border-color: var(--color-accent); }
	.board-n { font-family: var(--font-mono); font-size: 1.3rem; font-weight: 700; color: var(--color-accent); }
	.board-count { font-size: 0.8rem; color: var(--color-text-muted); margin-top: 0.25rem; }
</style>
