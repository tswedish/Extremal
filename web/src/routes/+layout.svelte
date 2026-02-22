<script lang="ts">
	import '../app.css';

	let { children } = $props();
</script>

<div class="app">
	<header>
		<nav>
			<a href="/" class="logo">RamseyNet</a>
			<div class="nav-links">
				<a href="/challenges">Challenges</a>
				<a href="/records">Records</a>
				<a href="/submit">Submit</a>
			</div>
		</nav>
	</header>

	<main>
		{@render children()}
	</main>
</div>

<style>
	.app {
		min-height: 100vh;
		display: flex;
		flex-direction: column;
	}

	header {
		border-bottom: 1px solid var(--color-border);
		padding: 0.75rem 1.5rem;
	}

	nav {
		display: flex;
		align-items: center;
		justify-content: space-between;
		max-width: 1200px;
		margin: 0 auto;
	}

	.logo {
		font-family: var(--font-mono);
		font-size: 1.25rem;
		font-weight: 700;
		color: var(--color-text);
		letter-spacing: -0.02em;
	}

	.nav-links {
		display: flex;
		gap: 1.5rem;
	}

	.nav-links a {
		color: var(--color-text-muted);
		font-size: 0.875rem;
	}

	.nav-links a:hover {
		color: var(--color-text);
	}

	main {
		flex: 1;
		max-width: 1200px;
		margin: 0 auto;
		padding: 2rem 1.5rem;
		width: 100%;
	}
</style>
