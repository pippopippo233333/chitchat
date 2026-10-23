import type { Channel, CustomEmoji, Member, Role } from './api';
import { emojiImageUrl } from './custom-emoji';

// What the draft composer shows for each piece of the canonical message text. Mentions and server emoji become
// atomic chips; anything unknown (deleted member, role, channel or emoji) stays as the raw text that will be sent.
export type ComposerPart = { text: string } | { token: string; label: string; self?: boolean } | { token: string; url: string; name: string };
export interface ComposerContext { members: Member[]; roles: Role[]; channels: Channel[]; emojis: CustomEmoji[]; origin: string; selfId?: string }

export function composerParts(value: string, context: ComposerContext): ComposerPart[] {
  const parts: ComposerPart[] = [], pattern = /<@(&?)([0-9]+)>|<#([0-9]+)>|<:([a-zA-Z0-9_]{2,32}):([0-9]+)>/g;
  let last = 0;
  const text = (end: number) => { if (end > last) parts.push({ text: value.slice(last, end) }); };
  for (const match of value.matchAll(pattern)) {
    const token = match[0];
    let part: ComposerPart | null = null;
    if (match[5]) {
      const emoji = context.emojis.find(item => item.id === match[5]), url = emoji ? emojiImageUrl(emoji.url, context.origin) : '';
      if (url) part = { token, url, name: emoji!.name };
    } else if (match[3]) {
      const channel = context.channels.find(item => item.id === match[3]);
      if (channel) part = { token, label: '#' + channel.name };
    } else if (match[1]) {
      const role = context.roles.find(item => item.id === match[2]);
      if (role) part = { token, label: '@' + role.name };
    } else {
      const member = context.members.find(item => item.user.id === match[2]);
      if (member) part = { token, label: '@' + member.user.display_name, self: member.user.id === context.selfId };
    }
    if (!part) continue;
    text(match.index); parts.push(part); last = match.index + token.length;
  }
  text(value.length);
  return parts;
}
