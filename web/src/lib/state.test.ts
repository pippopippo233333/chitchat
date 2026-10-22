import { describe, expect, it } from 'vitest';
import type { Message } from './api';
import { mergeMessages, mergeEventMessage, mergeHistory, updateReaction } from './state';
const message = (id: string, content = id) => ({ id, content }) as Message;
describe('live message state', () => {
  it('deduplicates REST and WS echoes while preserving full snowflake precision', () => {
    const messages = mergeMessages([message('9007199254740993'), message('9007199254740992')], [message('9007199254740993', 'edited'), message('10000000000000000')]);
    expect(messages.map(item => item.id)).toEqual(['9007199254740992', '9007199254740993', '10000000000000000']);
    expect(messages[1].content).toBe('edited');
  });
  it('does not lose newer live edits when prepending history', () => {
    expect(mergeMessages([message('1'), message('2', 'old')], [message('2', 'new')])[1].content).toBe('new');
  });
  it('updates reaction counts without clearing another member’s reaction', () => {
    let reactions = updateReaction([], '👍', true, true);
    reactions = updateReaction(reactions, '👍', false, true);
    expect(reactions).toEqual([{ emoji: '👍', count: 2, me: true }]);
    reactions = updateReaction(reactions, '👍', true, false);
    expect(reactions).toEqual([{ emoji: '👍', count: 1, me: false }]);
    expect(updateReaction(reactions, '👍', false, false)).toEqual([]);
  });
  it('preserves REST reaction ownership through broadcast message edits', () => {
    const current = { ...message('1'), reactions: [{ emoji: '👍', count: 2, me: true }] };
    const event = { ...current, content: 'edited', reactions: [{ emoji: '👍', count: 3, me: false }] };
    expect(mergeEventMessage(current, event).reactions).toEqual([{ emoji: '👍', count: 3, me: true }]);
    expect(mergeEventMessage(current, event, new Map([['👍', false]])).reactions[0].me).toBe(false);
  });
  it('reconciles live updates during history fetch without losing ownership or resurrecting deletes', () => {
    const page = [{ ...message('1'), reactions: [{ emoji: '👍', count: 1, me: true }] }, { ...message('2'), reactions: [] }];
    const event = { ...page[0], content: 'live edit', reactions: [{ emoji: '👍', count: 1, me: false }] };
    const changed = new Map<string, Message | null>([['1', event], ['2', null]]);
    const result = mergeHistory(page, [], changed, new Map());
    expect(result).toHaveLength(1); expect(result[0].content).toBe('live edit'); expect(result[0].reactions[0].me).toBe(true);
    expect(mergeHistory(page, [], changed, new Map([['1', new Map([['👍', false]])]]))[0].reactions[0].me).toBe(false);
  });
});
