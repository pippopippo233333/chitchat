import { describe, expect, it } from 'vitest';
import { avatarUrl, memberPresence } from './profile';
describe('profile image origin and presence', () => {
  it('loads only server-owned avatar routes from the connected host', () => {
    const path = `/s/main/api/avatars/123/${'a'.repeat(64)}.webp`;
    expect(avatarUrl(path, 'https://chat.example')).toBe(`https://chat.example${path}`);
    for (const unsafe of [`https://other.example${path}`, 'javascript:alert(1)', '/api/host', `https://user:pass@chat.example${path}`, null]) {
      expect(avatarUrl(unsafe, 'https://chat.example')).toBe('');
    }
  });
  it('disconnected members are offline even with a saved away preference', () => {
    expect(memberPresence({ online: false, status: 'away' })).toBe('offline');
    expect(memberPresence({ online: true, status: 'away' })).toBe('away');
    expect(memberPresence({ online: true })).toBe('online');
  });
});
