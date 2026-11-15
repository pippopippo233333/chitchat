import { describe, expect, it } from 'vitest';
import { flagParts } from './flags';

describe('message flags', () => {
  it('uses an image for a country flag and preserves the surrounding text', () => {
    const parts = flagParts('Location: 🇩🇪 Frankfurt');
    expect(parts.map(part => part.text).join('')).toBe('Location: 🇩🇪 Frankfurt');
    expect(parts[1]).toMatchObject({ code: 'DE', url: expect.any(String) });
  });

  it('keeps unsupported regional indicator pairs as text', () => {
    expect(flagParts('🇿🇿')).toEqual([{ text: '🇿🇿' }]);
  });
});
