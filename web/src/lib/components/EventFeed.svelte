<script lang="ts">
	import { connect, disconnect, getEvents, isConnected } from '$lib/stores/events.svelte';

	let { maxEvents = 20 }: { maxEvents?: number } = $props();

	// Use $effect with cleanup instead of onMount/onDestroy — required for
	// reliable lifecycle in Svelte 5 runes-mode components with SvelteKit navigation.
	$effect(() => {
		connect();
		return () => disconnect();
	});

	const events = $derived(getEvents().slice(0, maxEvents));
	const connected = $derived(isConnected());

	function typeColor(eventType: string): string {
		if (eventType.includes('created')) return 'var(--color-accent)';
		if (eventType.includes('verified') || eventType.includes('accepted'))
			return 'var(--color-accepted)';
		if (eventType.includes('rejected')) return 'var(--color-rejected)';
		if (eventType.includes('record')) return '#f59e0b';
		return 'var(--color-text-muted)';
	}

	function summarizePayload(payload: string): string {
		try {
			const p = typeof payload === 'string' ? JSON.parse(payload) : payload;
			if (p.challenge_id) return p.challenge_id;
			if (p.graph_cid) return p.graph_cid.slice(0, 12) + '...';
			return JSON.stringify(p).slice(0, 40);
		} catch {
			return String(payload).slice(0, 40);
		}
	}
</script>

<div class="event-feed">
	<div class="feed-header">
		<span class="feed-title">Live Events</span>
		<span class="connection-dot" class:online={connected}></span>
	</div>
	{#if events.length === 0}
		<div class="empty">No events yet</div>
	{:else}
		<div class="events-list">
			{#each events as event (event.seq)}
				<div class="event-row">
					<span class="seq">#{event.seq}</span>
					<span class="event-type" style="color: {typeColor(event.event_type)}">
						{event.event_type}
					</span>
					<span class="payload">{summarizePayload(event.payload)}</span>
				</div>
			{/each}
		</div>
	{/if}
</div>

<style>
	.event-feed {
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-radius: 0.75rem;
		overflow: hidden;
	}

	.feed-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 0.75rem 1rem;
		border-bottom: 1px solid var(--color-border);
	}

	.feed-title {
		font-family: var(--font-mono);
		font-size: 0.8125rem;
		font-weight: 600;
	}

	.connection-dot {
		width: 8px;
		height: 8px;
		border-radius: 50%;
		background: var(--color-rejected);
	}

	.connection-dot.online {
		background: var(--color-accepted);
	}

	.empty {
		padding: 1.5rem 1rem;
		text-align: center;
		color: var(--color-text-muted);
		font-size: 0.8125rem;
	}

	.events-list {
		max-height: 320px;
		overflow-y: auto;
	}

	.event-row {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		padding: 0.375rem 1rem;
		font-family: var(--font-mono);
		font-size: 0.75rem;
		border-bottom: 1px solid var(--color-border);
	}

	.event-row:last-child {
		border-bottom: none;
	}

	.seq {
		color: var(--color-text-muted);
		min-width: 2.5rem;
	}

	.event-type {
		font-weight: 600;
		min-width: 8rem;
	}

	.payload {
		color: var(--color-text-muted);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
</style>
