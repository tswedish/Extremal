<script lang="ts">
	let status = $state<string>('connecting...');

	async function checkHealth() {
		try {
			const res = await fetch('/api/health');
			const data = await res.json();
			status = `${data.name} v${data.version} — ${data.status}`;
		} catch {
			status = 'server offline';
		}
	}

	$effect(() => {
		checkHealth();
	});
</script>

<svelte:head>
	<title>RamseyNet</title>
</svelte:head>

<div class="hero">
	<h1>RamseyNet</h1>
	<p class="subtitle">
		A permissionless protocol for distributed Ramsey graph search
		and deterministic generative graph art
	</p>
	<div class="status-badge" class:online={status.includes('ok')}>
		{status}
	</div>
</div>

<div class="grid">
	<div class="card">
		<h2>Challenges</h2>
		<p>Browse active Ramsey challenges and their best-known bounds.</p>
		<a href="/challenges">View challenges</a>
	</div>
	<div class="card">
		<h2>Submit</h2>
		<p>Submit a candidate graph for verification against a challenge.</p>
		<a href="/submit">Submit graph</a>
	</div>
	<div class="card">
		<h2>Records</h2>
		<p>Track the evolving frontier of best-known Ramsey numbers.</p>
		<a href="/challenges">View records</a>
	</div>
</div>

<style>
	.hero {
		text-align: center;
		padding: 3rem 0 2rem;
	}

	h1 {
		font-family: var(--font-mono);
		font-size: 3rem;
		font-weight: 800;
		letter-spacing: -0.03em;
		background: linear-gradient(135deg, var(--color-accent), #a78bfa);
		-webkit-background-clip: text;
		-webkit-text-fill-color: transparent;
		background-clip: text;
	}

	.subtitle {
		color: var(--color-text-muted);
		max-width: 560px;
		margin: 0.75rem auto 1.5rem;
	}

	.status-badge {
		display: inline-block;
		padding: 0.25rem 0.75rem;
		border-radius: 9999px;
		font-family: var(--font-mono);
		font-size: 0.75rem;
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		color: var(--color-text-muted);
	}

	.status-badge.online {
		border-color: var(--color-accepted);
		color: var(--color-accepted);
	}

	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
		gap: 1.25rem;
		margin-top: 2rem;
	}

	.card {
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-radius: 0.75rem;
		padding: 1.5rem;
	}

	.card h2 {
		font-size: 1.125rem;
		margin-bottom: 0.5rem;
	}

	.card p {
		color: var(--color-text-muted);
		font-size: 0.875rem;
		margin-bottom: 1rem;
	}

	.card a {
		font-size: 0.875rem;
		font-weight: 500;
	}
</style>
