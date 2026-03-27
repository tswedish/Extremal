<script lang="ts">
	import '../app.css';
	let { children } = $props();
</script>

<div class="app">
	<header>
		<nav>
			<a href="https://extremalgames.ai" class="logo" target="_blank" rel="noopener noreferrer">Extremal Games</a>
			<div class="nav-links">
				<a href="/leaderboards">Leaderboards</a>
				<a href="/submit">Submit</a>
				<a href="/dashboard">Dashboard</a>
				<a href="/rain">Rain</a>
				<a href="https://github.com/tswedish/Extremal" target="_blank" rel="noopener noreferrer" class="github-link" title="GitHub">
					<svg width="18" height="18" viewBox="0 0 16 16" fill="currentColor"><path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8z"/></svg>
				</a>
				<span class="build-tag">v{__APP_VERSION__} ({__BUILD_COMMIT__})</span>
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
	.github-link {
		display: flex;
		align-items: center;
		opacity: 0.5;
		transition: opacity 0.15s;
	}
	.github-link:hover { opacity: 1; }
	.build-tag {
		font-family: var(--font-mono);
		font-size: 0.6rem;
		color: var(--color-text-dim, #555);
		opacity: 0.5;
		margin-left: 0.5rem;
		user-select: all;
	}
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
