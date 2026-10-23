import type { CustomEmoji } from './api';
import type { EmojiGroup } from './emoji';

export function emojiImageUrl(path: string, origin: string): string {
  try {
    const url = new URL(path, origin);
    return url.origin === new URL(origin).origin && !url.username && !url.password
      && /^\/s\/[^/]+\/api\/emojis\/\d+\/[a-f0-9]{64}\.webp$/.test(url.pathname) ? url.href : '';
  } catch { return ''; }
}
export function customReaction(emoji: CustomEmoji) { return `custom:${emoji.id}`; }
export function customToken(emoji: CustomEmoji) { return `<:${emoji.name}:${emoji.id}>`; }
export function reactionName(value: string, emojis: CustomEmoji[]) {
  return value.startsWith('custom:') ? emojis.find(emoji => customReaction(emoji) === value)?.name ?? 'deleted emoji' : value;
}
export function emojiGroups(emojis: CustomEmoji[], origin: string): EmojiGroup[] {
  const items = emojis.map(emoji => ({ emoji: customReaction(emoji), words: emoji.name, url: emojiImageUrl(emoji.url, origin) })).filter(item => item.url);
  return items.length ? [{name:'Server emoji',items}] : [];
}
export function customParts(text: string, emojis: CustomEmoji[], origin: string): {text:string;url?:string;name?:string}[] {
  const parts: {text:string;url?:string;name?:string}[] = [];
  const pattern = /<:([a-zA-Z0-9_]{2,32}):([0-9]+)>/g;
  let cursor = 0;
  for (const match of text.matchAll(pattern)) {
    if (match.index > cursor) parts.push({text:text.slice(cursor, match.index)});
    const emoji = emojis.find(item => item.id === match[2]);
    const url = emoji ? emojiImageUrl(emoji.url, origin) : '';
    parts.push(url ? {text:match[0],url,name:emoji!.name} : {text:`:${match[1]}:`});
    cursor = match.index + match[0].length;
  }
  if (cursor < text.length) parts.push({text:text.slice(cursor)});
  return parts;
}
