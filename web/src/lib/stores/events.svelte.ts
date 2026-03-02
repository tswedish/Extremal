import { untrack } from 'svelte';
import { connectEvents, type EventMessage } from '$lib/api';

const MAX_EVENTS = 50;
const INITIAL_DELAY = 2000;
const MAX_DELAY = 30000;

type EventListener = (msg: EventMessage) => void;

let events = $state<EventMessage[]>([]);
let connected = $state(false);
let ws: WebSocket | null = null;
let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
let reconnectDelay = INITIAL_DELAY;
let intentionalClose = false;
let refCount = 0;
const listeners = new Set<EventListener>();

function handleMessage(ev: MessageEvent) {
	try {
		const msg: EventMessage = JSON.parse(ev.data);
		untrack(() => {
			if (events.some((e) => e.seq === msg.seq)) return;
			events = [msg, ...events].slice(0, MAX_EVENTS);
			for (const fn of listeners) {
				try { fn(msg); } catch { /* listener errors shouldn't break the store */ }
			}
		});
	} catch {
		// ignore malformed messages
	}
}

function handleOpen() {
	connected = true;
	reconnectDelay = INITIAL_DELAY;
}

function handleClose() {
	connected = false;
	ws = null;
	if (!intentionalClose) {
		scheduleReconnect();
	}
}

function scheduleReconnect() {
	if (reconnectTimer || intentionalClose) return;
	// Don't reconnect if the tab is hidden
	if (typeof document !== 'undefined' && document.hidden) return;
	reconnectTimer = setTimeout(() => {
		reconnectTimer = null;
		doConnect();
	}, reconnectDelay);
	reconnectDelay = Math.min(reconnectDelay * 2, MAX_DELAY);
}

function doConnect() {
	if (ws || intentionalClose) return;
	try {
		const lastSeq = events.length > 0 ? events[0].seq : 0;
		const socket = connectEvents();
		ws = socket;
		socket.onmessage = handleMessage;
		socket.onopen = () => {
			handleOpen();
			if (lastSeq > 0) socket.send(JSON.stringify({ after_seq: lastSeq }));
		};
		socket.onclose = handleClose;
		// Don't call socket.close() from onerror — the close event fires automatically
		socket.onerror = () => {};
	} catch {
		ws = null;
		scheduleReconnect();
	}
}

function handleVisibility() {
	if (document.hidden) return;
	// Tab became visible — reconnect if disconnected
	if (!ws && !intentionalClose && !reconnectTimer) {
		reconnectDelay = INITIAL_DELAY;
		doConnect();
	}
}

export function connect() {
	refCount++;
	if (refCount === 1) {
		intentionalClose = false;
		if (typeof document !== 'undefined') {
			document.addEventListener('visibilitychange', handleVisibility);
		}
		doConnect();
	}
}

export function disconnect() {
	refCount = Math.max(0, refCount - 1);
	if (refCount === 0) {
		intentionalClose = true;
		if (typeof document !== 'undefined') {
			document.removeEventListener('visibilitychange', handleVisibility);
		}
		if (reconnectTimer) {
			clearTimeout(reconnectTimer);
			reconnectTimer = null;
		}
		if (ws) {
			ws.onclose = null;
			ws.close();
			ws = null;
		}
		connected = false;
	}
}

/**
 * Subscribe to new events on the shared WebSocket.
 * Automatically calls connect/disconnect for lifecycle management.
 * Returns an unsubscribe function.
 */
export function subscribe(fn: EventListener): () => void {
	listeners.add(fn);
	connect();
	return () => {
		listeners.delete(fn);
		disconnect();
	};
}

export function getEvents(): EventMessage[] {
	return events;
}

export function isConnected(): boolean {
	return connected;
}
