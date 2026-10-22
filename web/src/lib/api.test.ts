import { afterEach, describe, expect, it, vi } from 'vitest';
import { Api, ApiError, can, normalizeHost, sessionKey } from './api';

afterEach(() => vi.unstubAllGlobals());
describe('API contract', () => {
  it('scopes bearer tokens to a server and omits credentials on host discovery', async () => {
    const fetch = vi.fn().mockImplementation(() => Promise.resolve(new Response('{}', { status: 200 })));
    vi.stubGlobal('fetch', fetch);
    const api = new Api('https://chat.example.com/', 'gaming', 'secret');
    await api.host(); await api.request('/me');
    expect(fetch.mock.calls[0][0]).toBe('https://chat.example.com/api/host');
    expect(fetch.mock.calls[0][1].headers.Authorization).toBeUndefined();
    expect(fetch.mock.calls[1][0]).toBe('https://chat.example.com/s/gaming/api/me');
    expect(fetch.mock.calls[1][1].headers.Authorization).toBe('Bearer secret');
    expect(fetch.mock.calls[1][1].credentials).toBe('omit');
    expect(api.socketUrl()).toBe('wss://chat.example.com/s/gaming/api/ws');
    expect(api.socketUrl()).not.toContain('secret');
  });
  it('passes server errors and retry hints to the caller', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ error: { code: 'rate_limited', message: 'Slow down.', retry_after_ms: 1200 } }), { status: 429 })));
    await expect(new Api('http://localhost').request('/me')).rejects.toMatchObject({ status: 429, code: 'rate_limited', message: 'Slow down.', retryAfter: 1200 });
  });
  it('handles empty 204 responses and URL-encodes reaction emoji', async () => {
    const fetch = vi.fn().mockResolvedValue(new Response(null, { status: 204 })); vi.stubGlobal('fetch', fetch);
    await expect(new Api('http://localhost', 'dev').react('42', '99', '👍', true)).resolves.toBeUndefined();
    expect(fetch.mock.calls[0][0]).toBe('http://localhost/s/dev/api/channels/42/messages/99/reactions/%F0%9F%91%8D');
    expect(fetch.mock.calls[0][1].method).toBe('DELETE');
  });
  it('preserves AbortError so obsolete history loads do not become user errors', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new DOMException('Aborted', 'AbortError')));
    await expect(new Api('http://localhost').messages('42')).rejects.toMatchObject({ name: 'AbortError' });
  });
  it('rejects non-host URLs and separates sessions by origin and server', () => {
    for (const value of ['javascript:alert(1)', 'https://user:pass@host.test', 'https://host.test/path', 'https://host.test?token=x']) expect(() => normalizeHost(value)).toThrow();
    expect(sessionKey('https://a.test', 'dev')).not.toBe(sessionKey('https://b.test', 'dev'));
    expect(sessionKey('https://a.test', 'dev')).not.toBe(sessionKey('https://a.test', 'work'));
  });
  it('honors owner and administrator permissions without number truncation', () => {
    expect(can('0', 3, true)).toBe(true); expect(can('512', 3)).toBe(true);
    expect(can('259', 8)).toBe(true); expect(can('259', 3)).toBe(false); expect(can('771', 3)).toBe(true);
    expect(can('9223372036854775810', 1)).toBe(true); expect(can('invalid', 1)).toBe(false);
    expect(new ApiError(401, 'unauthorized', 'Expired')).toBeInstanceOf(Error);
  });
});
