import type { Channel, DirectMessage, Snapshot, VoiceState } from './api';

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

// Calls already going on in our direct messages that should ring us: on opening the app from a call notification
// the ring itself was sent while we were away. `seen` holds calls we were in, declined or were already rung for;
// a call drops out of it when it ends, so the next call rings again.
export function ongoingCalls(states: VoiceState[], dms: DirectMessage[], me: string, joined: string | null, seen: Set<string>): Omit<Ring, 'at'>[] {
  const live = new Set(states.filter(state => dms.some(dm => dm.id === state.channel_id)).map(state => state.channel_id));
  for (const state of states) if (live.has(state.channel_id) && state.user_id === me) seen.add(state.channel_id);
  if (joined && live.has(joined)) seen.add(joined);
  for (const id of [...seen]) if (!live.has(id)) seen.delete(id);
  const rings: Omit<Ring, 'at'>[] = [];
  for (const id of live) {
    if (seen.has(id)) continue;
    seen.add(id);
    const caller = states.find(state => state.channel_id === id && state.user_id !== me), dm = dms.find(item => item.id === id);
    rings.push({ channel_id: id, from_user_id: caller?.user_id ?? dm?.user.id ?? '', from_name: dm?.user.display_name ?? 'Someone', invited: false });
  }
  return rings;
}
