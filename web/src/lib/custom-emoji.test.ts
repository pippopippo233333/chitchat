import { describe, expect, it } from 'vitest';
import { customParts, customToken, emojiGroups, emojiImageUrl, reactionName } from './custom-emoji';
const emoji = {id:'12345',name:'party',url:`/s/main/api/emojis/12345/${'a'.repeat(64)}.webp`};
describe('custom emoji resolve only through the current server list', () => {
  it('turns valid known tokens into server-owned image references', () => {
    const parts = customParts(`hello ${customToken(emoji)} there`,[emoji],'https://chat.example');
    expect(parts).toHaveLength(3);
    expect(parts[1]).toMatchObject({name:'party',url:`https://chat.example${emoji.url}`});
    expect(reactionName('custom:12345',[emoji])).toBe('party');
    expect(emojiGroups([emoji],'https://chat.example')[0].items[0].emoji).toBe('custom:12345');
  });
  it('keeps unknown or deleted IDs as text and rejects cross-host images', () => {
    expect(customParts('<:gone:23456>',[emoji],'https://chat.example')).toEqual([{text:':gone:'}]);
    expect(emojiImageUrl(`https://other.example${emoji.url}`,'https://chat.example')).toBe('');
    expect(emojiImageUrl('/not-an-emoji','https://chat.example')).toBe('');
    expect(emojiGroups([{...emoji,url:'https://tracker.example/photo'}],'https://chat.example')).toEqual([]);
    expect(reactionName('custom:999',[])).toBe('deleted emoji');
  });
});
