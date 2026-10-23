import { describe, expect, it } from 'vitest';
import { EMOJI_GROUPS, searchEmoji } from './emoji';

describe('emoji list', () => {
  it('has no duplicates and every entry has search words', () => {
    const all = EMOJI_GROUPS.flatMap(group => group.items);
    expect(new Set(all.map(item => item.emoji)).size).toBe(all.length);
    expect(all.every(item => item.emoji && item.words)).toBe(true);
  });
  it('searches by word prefix', () => {
    expect(searchEmoji('thumb').map(item => item.emoji)).toEqual(['👍', '👎']);
    expect(searchEmoji('  ')).toEqual([]);
  });
});
