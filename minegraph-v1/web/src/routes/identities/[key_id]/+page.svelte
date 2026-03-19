<script lang="ts">
	import { page } from '$app/stores';
	import { getIdentity, getIdentitySubmissions, type IdentityDetail } from '$lib/api';

	const keyId = $derived($page.params.key_id);
	let identity = $state<IdentityDetail | null>(null);
	let submissions = $state<any[]>([]);
	let loading = $state(true);
	let error = $state('');

	$effect(() => {
		loading = true; error = '';
		Promise.all([
			getIdentity(keyId),
			getIdentitySubmissions(keyId, 50),
		]).then(([id, subs]) => {
			identity = id;
			submissions = subs.submissions || [];
			loading = false;
		}).catch(e => { error = e.message; loading = false; });
	});

	function copyToClipboard(text: string) {
		navigator.clipboard.writeText(text);
	}
</script>

<h1>Identity</h1>

{#if loading}
	<div class="shimmer" style="height: 150px; border-radius: 0.5rem;"></div>
{:else if error}
	<p class="error">{error}</p>
{:else if identity}
	<div class="card id-card">
		<dl>
			<dt>Key ID</dt>
			<dd class="mono">{identity.key_id}
				<button class="copy-btn" onclick={() => copyToClipboard(identity!.key_id)}>Copy</button>
			</dd>
			<dt>Public Key</dt>
			<dd class="mono pk">{identity.public_key}
				<button class="copy-btn" onclick={() => copyToClipboard(identity!.public_key)}>Copy</button>
			</dd>
			{#if identity.display_name}
				<dt>Name</dt><dd>{identity.display_name}</dd>
			{/if}
			{#if identity.github_repo}
				<dt>Repo</dt><dd><a href={identity.github_repo}>{identity.github_repo}</a></dd>
			{/if}
			<dt>Registered</dt><dd class="dm">{new Date(identity.created_at).toLocaleString()}</dd>
		</dl>
	</div>

	{#if submissions.length > 0}
		<section class="subs">
			<h2>Recent Submissions ({submissions.length})</h2>
			<table>
				<thead><tr><th>CID</th><th>When</th></tr></thead>
				<tbody>
					{#each submissions as sub}
						<tr>
							<td><a href="/submissions/{sub.cid}" class="mono">{sub.cid.slice(0, 20)}...</a></td>
							<td class="dm">{new Date(sub.created_at).toLocaleString()}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</section>
	{/if}
{/if}

<style>
	h1 { font-family: var(--font-mono); font-size: 1.3rem; margin-bottom: 1rem; }
	h2 { font-family: var(--font-mono); font-size: 0.9rem; color: var(--color-text-muted); margin-bottom: 0.5rem; }
	.error { color: var(--color-red); }
	.id-card { margin-bottom: 1.5rem; }
	dl { display: grid; grid-template-columns: auto 1fr; gap: 0.3rem 1rem; }
	dt { font-size: 0.75rem; color: var(--color-text-muted); }
	dd { font-size: 0.8rem; display: flex; align-items: center; gap: 0.5rem; }
	.pk { font-size: 0.6rem; word-break: break-all; }
	.dm { color: var(--color-text-muted); font-size: 0.8rem; }
	.copy-btn {
		font-size: 0.6rem; padding: 0.1rem 0.3rem; border-radius: 0.2rem;
		background: var(--color-bg); border: 1px solid var(--color-border);
		color: var(--color-text-dim); cursor: pointer; font-family: var(--font-mono);
	}
	.copy-btn:hover { border-color: var(--color-accent); color: var(--color-accent); }
	table { width: 100%; border-collapse: collapse; font-size: 0.8rem; }
	th { text-align: left; padding: 0.3rem 0.5rem; border-bottom: 1px solid var(--color-border); color: var(--color-text-muted); font-size: 0.7rem; text-transform: uppercase; }
	td { padding: 0.3rem 0.5rem; border-bottom: 1px solid rgba(42,42,58,0.2); }
	.subs { margin-top: 1rem; }
</style>
