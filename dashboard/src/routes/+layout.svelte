<script lang="ts">
	import '../app.css';
	import { store } from '$lib/dashboard-store.svelte';

	let { children } = $props();

	$effect(() => {
		store.loadSettings();
		store.connect();
		return () => store.disconnect();
	});
</script>

<div class="app">
	<header>
		<nav>
			<a href="/" class="logo">MineGraph Dashboard</a>
			<div class="nav-right">
				<div class="mode-toggle">
					<button class:active={store.mode === 'monitor'} onclick={() => { store.mode = 'monitor'; store.saveSettings(); }}>Monitor</button>
					<button class:active={store.mode === 'rain'} onclick={() => { store.mode = 'rain'; store.saveSettings(); }}>Rain</button>
				</div>
				<span class="conn-status" class:connected={store.connected}>
					{store.connected ? 'connected' : 'disconnected'}
				</span>
			</div>
		</nav>
	</header>
	<main class:rain-mode={store.mode === 'rain'}>
		{@render children()}
	</main>
</div>

<style>
	.app {
		height: 100vh;
		display: flex;
		flex-direction: column;
	}
	header {
		border-bottom: 1px solid var(--color-border);
		background: var(--color-surface);
		z-index: 50;
	}
	nav {
		max-width: 1400px;
		margin: 0 auto;
		padding: 0.5rem 1.5rem;
		display: flex;
		align-items: center;
		justify-content: space-between;
	}
	.logo {
		font-family: var(--font-mono);
		font-size: 0.95rem;
		font-weight: 700;
		background: linear-gradient(135deg, #6366f1, #a855f7);
		-webkit-background-clip: text;
		-webkit-text-fill-color: transparent;
		background-clip: text;
	}
	.nav-right { display: flex; align-items: center; gap: 1rem; }
	.mode-toggle { display: flex; gap: 0; }
	.mode-toggle button {
		font-family: var(--font-mono);
		font-size: 0.7rem;
		padding: 0.25rem 0.75rem;
		background: var(--color-bg);
		border: 1px solid var(--color-border);
		color: var(--color-text-dim);
		cursor: pointer;
	}
	.mode-toggle button:first-child { border-radius: 0.3rem 0 0 0.3rem; }
	.mode-toggle button:last-child { border-radius: 0 0.3rem 0.3rem 0; border-left: none; }
	.mode-toggle button.active {
		background: var(--color-accent);
		color: white;
		border-color: var(--color-accent);
	}
	.conn-status {
		font-family: var(--font-mono);
		font-size: 0.65rem;
		color: var(--color-red);
	}
	.conn-status.connected { color: var(--color-green); }

	main {
		flex: 1;
		overflow: auto;
		min-height: 0;
	}
	main:not(.rain-mode) {
		max-width: 1400px;
		margin: 0 auto;
		padding: 0.75rem 1.5rem;
		width: 100%;
	}
	main.rain-mode {
		padding: 0;
		overflow: hidden;
	}
</style>
