<script lang="ts">
	import '../app.css';
	let { children } = $props();
</script>

<div class="app">
	<header>
		<nav>
			<a href="/" class="logo">MineGraph</a>
			<div class="nav-links">
				<a href="/leaderboards">Leaderboards</a>
				<a href="/dashboard">Dashboard</a>
				<a href="/rain">Rain</a>
			</div>
		</nav>
	</header>
	<main>
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
	}
	nav {
		max-width: 1200px;
		margin: 0 auto;
		padding: 0.75rem 1.5rem;
		display: flex;
		align-items: center;
		justify-content: space-between;
	}
	.logo {
		font-family: var(--font-mono);
		font-size: 1.1rem;
		font-weight: 700;
		background: linear-gradient(135deg, #6366f1, #a855f7);
		-webkit-background-clip: text;
		-webkit-text-fill-color: transparent;
		background-clip: text;
	}
	.nav-links { display: flex; gap: 1.5rem; }
	.nav-links a {
		font-size: 0.85rem;
		color: var(--color-text-muted);
		transition: color 0.15s;
	}
	.nav-links a:hover { color: var(--color-text); }
	main {
		max-width: 1200px;
		margin: 0 auto;
		padding: 0.75rem 1.5rem;
		flex: 1;
		display: flex;
		flex-direction: column;
		min-height: 0;
		overflow: auto;
		width: 100%;
	}
</style>
