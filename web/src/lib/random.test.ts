import { afterEach, describe, expect, it, vi } from 'vitest';
import { randomId } from './random';
afterEach(() => vi.unstubAllGlobals());
describe('LAN-compatible identifiers', () => {
  it('uses native UUIDs when available', () => { vi.stubGlobal('crypto', { randomUUID: () => 'native-uuid' }); expect(randomId()).toBe('native-uuid'); });
  it('falls back to cryptographic random bytes with v4 and variant bits on HTTP', () => {
    const getRandomValues = vi.fn((bytes: Uint8Array) => { bytes.fill(255); return bytes; });
    vi.stubGlobal('crypto', { getRandomValues });
    expect(randomId()).toBe('ffffffff-ffff-4fff-bfff-ffffffffffff'); expect(getRandomValues).toHaveBeenCalledOnce();
  });
});
