import { describe, expect, it } from 'vitest';
import { composerParts } from './composer';

const member = (id: string, display_name: string) => ({ user: { id, username: display_name.toLowerCase(), display_name }, role_ids: [] }) as never;
const context = {
  members: [member('200000000000000001', 'Ann')], roles: [{ id: '5', name: 'Mods' }] as never, channels: [{ id: '7', name: 'general' }] as never,
  emojis: [{ id: '200000000000000002', name: 'idk', url: '/s/main/api/emojis/200000000000000002/' + 'a'.repeat(64) + '.webp' }] as never,
  origin: 'https://chat.example', selfId: '200000000000000001',
};

describe('composerParts', () => {
  it('shows mentions as names and server emoji as images, keeping the tokens', () => {
    expect(composerParts('hi <:idk:200000000000000002> <@200000000000000001> <@&5> <#7>!', context)).toEqual([
      { text: 'hi ' },
      { token: '<:idk:200000000000000002>', url: 'https://chat.example/s/main/api/emojis/200000000000000002/' + 'a'.repeat(64) + '.webp', name: 'idk' },
      { text: ' ' }, { token: '<@200000000000000001>', label: '@Ann', self: true }, { text: ' ' }, { token: '<@&5>', label: '@Mods' },
      { text: ' ' }, { token: '<#7>', label: '#general' }, { text: '!' },
    ]);
  });
  it('leaves unknown or deleted tokens as the raw text that will be sent', () => {
    expect(composerParts('<@1> <:gone:2> <#3>', context)).toEqual([{ text: '<@1> <:gone:2> <#3>' }]);
  });
  it('round-trips to the same text', () => {
    const value = 'a\n<@200000000000000001>\n\n<:idk:200000000000000002>b';
    expect(composerParts(value, context).map(part => 'token' in part ? part.token : part.text).join('')).toBe(value);
  });
});
