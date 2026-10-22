import { describe, expect, it } from 'vitest';
import { ApiError } from './api';
import { contentLimits, sendRestriction } from './limits';

describe('message limits and sending restrictions', () => {
  it('matches server character and line limits, including emoji', () => {
    expect(contentLimits('😀'.repeat(4000)).error).toBe('');
    expect(contentLimits('x'.repeat(4001)).error).toContain('4,000');
    expect(contentLimits(Array(60).fill('a').join('\n')).error).toBe('');
    expect(contentLimits(Array(61).fill('a').join('\n')).error).toContain('60 lines');
    expect(contentLimits(' \n ').error).toContain('Write');
  });
  it('uses the server retry hint without changing the draft', () => {
    expect(sendRestriction(new ApiError(429, 'rate_limited', 'Slow down', 2500), 1000)).toEqual({ cooldown: 3500 });
    expect(sendRestriction(new ApiError(429, 'rate_limited', 'Slow down', -1), 1000)).toEqual({ cooldown: 2000 });
    expect(sendRestriction(new Error('offline'), 1000)).toEqual({});
  });
  it('uses a server mute deadline and handles malformed timestamps', () => {
    const until = '2026-10-21T20:00:00Z';
    expect(sendRestriction(new ApiError(403, 'timed_out', 'Muted', undefined, until), 0)).toEqual({ mutedUntil: Date.parse(until) });
    expect(sendRestriction(new ApiError(403, 'timed_out', 'Muted', undefined, 'invalid'), 1000)).toEqual({ mutedUntil: 11000 });
  });
});
