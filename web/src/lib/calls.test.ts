import { describe, expect, it } from 'vitest';
import { addRing, callRoom, liveRings, ongoingCalls } from './calls';

const snapshot = { permissions: '123' } as never;
const dms = [{ id: '9', user: { id: '2', display_name: 'Ben' } }] as never;

describe('calls', () => {
  it('keeps one ring per call and drops old ones', () => {
    const rings = addRing(addRing([], { channel_id: '9', from_user_id: '2', from_name: 'Ben', invited: false }, 0), { channel_id: '9', from_user_id: '3', from_name: 'Cat', invited: true }, 1000);
    expect(rings).toEqual([{ channel_id: '9', from_user_id: '3', from_name: 'Cat', invited: true, at: 1000 }]);
    expect(liveRings(rings, 30_000)).toHaveLength(1);
    expect(liveRings(rings, 47_000)).toHaveLength(0);
  });
  it('names a call after the direct message or whoever invited us', () => {
    expect(callRoom('9', snapshot, dms, {})?.name).toBe('Call with Ben');
    expect(callRoom('8', snapshot, dms, { 8: 'Cat' })).toMatchObject({ id: '8', type: 'dm', name: 'Call with Cat', my_permissions: '123' });
    expect(callRoom('7', snapshot, dms, {})).toBeUndefined();
  });
});

describe('ongoingCalls', () => {
  const state = (user_id: string, channel_id: string) => ({ user_id, channel_id }) as never;
  it('rings once for a call going on in our direct message, not for calls we were in', () => {
    const seen = new Set<string>();
    expect(ongoingCalls([state('2', '9')], dms, '1', null, seen)).toEqual([{ channel_id: '9', from_user_id: '2', from_name: 'Ben', invited: false }]);
    expect(ongoingCalls([state('2', '9')], dms, '1', null, seen)).toEqual([]);
    expect(ongoingCalls([], dms, '1', null, seen)).toEqual([]);
    expect(seen.size).toBe(0);
    const after = new Set<string>();
    expect(ongoingCalls([state('2', '9'), state('1', '9')], dms, '1', '9', after)).toEqual([]);
    expect(ongoingCalls([state('2', '9')], dms, '1', null, after)).toEqual([]);
    expect(ongoingCalls([state('2', '5')], dms, '1', null, new Set())).toEqual([]);
  });
});
