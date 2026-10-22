import type { Message, Reaction } from './api';

// IDs exceed Number.MAX_SAFE_INTEGER. Compare as integers without precision loss.
export function compareIds(a: string, b: string): number { return a.length - b.length || a.localeCompare(b); }
export function mergeMessages(current: Message[], incoming: Message[]): Message[] {
  const byId = new Map(current.map(message => [message.id, message]));
  for (const message of incoming) byId.set(message.id, message);
  return [...byId.values()].sort((a, b) => compareIds(a.id, b.id));
}
export function updateReaction(reactions: Reaction[], emoji: string, self: boolean, add: boolean): Reaction[] {
  const existing = reactions.find(reaction => reaction.emoji === emoji);
  if (!existing) return add ? [...reactions, { emoji, count: 1, me: self }] : reactions;
  const count = Math.max(0, existing.count + (add ? 1 : -1));
  return reactions.map(reaction => reaction.emoji !== emoji ? reaction : { ...reaction, count, me: self ? add : reaction.me }).filter(reaction => reaction.count > 0);
}
export function upsert<T extends { id: string }>(items: T[], item: T): T[] { return [...items.filter(old => old.id !== item.id), item]; }

// Broadcast messages contain me=false for every recipient. Only REST or self reaction events know ownership.
export function mergeEventMessage(current: Message | undefined, event: Message, ownEvents?: Map<string, boolean>): Message {
  return { ...event, reactions: event.reactions.map(reaction => ({ ...reaction, me: ownEvents?.get(reaction.emoji) ?? current?.reactions.find(old => old.emoji === reaction.emoji)?.me ?? false })) };
}

export function mergeHistory(page: Message[], current: Message[], changed: Map<string, Message | null>, ownEvents: Map<string, Map<string, boolean>>): Message[] {
  let merged = mergeMessages(page, current);
  for (const [id, event] of changed) {
    if (event === null) merged = merged.filter(message => message.id !== id);
    else merged = mergeMessages(merged, [mergeEventMessage(page.find(message => message.id === id) ?? merged.find(message => message.id === id), event, ownEvents.get(id))]);
  }
  return merged;
}
