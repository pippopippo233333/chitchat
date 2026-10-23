import type { Channel, DirectMessage, Snapshot } from './api';

// Private calls (docs/api.md 4p): voice in a direct message. A ring is someone calling us or inviting us into a call.
export interface Ring { channel_id: string; from_user_id: string; from_name: string; invited: boolean; at: number }
export const RING_SECONDS = 45;

export function addRing(rings: Ring[], data: Omit<Ring, 'at'>, now = Date.now()): Ring[] {
  return [...rings.filter(ring => ring.channel_id !== data.channel_id), { ...data, at: now }];
}
export function liveRings(rings: Ring[], now = Date.now()) { return rings.filter(ring => now - ring.at < RING_SECONDS * 1000); }

// The room the voice panel shows for a call: the direct message with that person, or (when we were invited into
// someone else's call) a call named after who invited us. Calls are capped by the host, not a channel limit.
export function callRoom(id: string, snapshot: Snapshot, dms: DirectMessage[], names: Record<string, string>): Channel | undefined {
  const dm = dms.find(item => item.id === id), name = dm?.user.display_name ?? names[id];
  if (!name) return undefined;
  return { id, type: 'dm', name: `Call with ${name}`, topic: '', position: 0, category_id: null, media_allowed: false, my_permissions: snapshot.permissions, user_limit: 25 };
}

// Loops the incoming-call sound until stopped. A browser that blocks autoplay just stays quiet (the card still shows).
export function startRingtone(volume: number): () => void {
  if (volume <= 0 || typeof Audio === 'undefined') return () => {};
  const audio = new Audio('/sounds/incoming-call.mp3');
  audio.loop = true; audio.volume = Math.min(1, volume / 100);
  void audio.play().catch(() => {});
  return () => { audio.pause(); audio.src = ''; };
}
