import { describe, expect, it } from 'vitest';
import { messageLinks } from './links';

describe('message links', () => {
  it('makes HTTP(S) and www links available without changing surrounding text', () => {
    const text = 'See https://example.com/a?x=1&y=2 and www.example.org.';
    const parts = messageLinks(text);
    expect(parts.map(part => part.text).join('')).toBe(text);
    expect(parts.filter(part => part.href).map(part => part.href)).toEqual(['https://example.com/a?x=1&y=2', 'https://www.example.org/']);
  });
  it('keeps punctuation outside links while retaining balanced URL parentheses', () => {
    const parts = messageLinks('(https://example.com/wiki/Test_(thing)).');
    expect(parts.map(part => part.text).join('')).toBe('(https://example.com/wiki/Test_(thing)).');
    expect(parts.find(part => part.href)?.href).toBe('https://example.com/wiki/Test_(thing)');
  });
  it('does not activate script, data, credential-bearing, or malformed links', () => {
    for (const text of ['javascript:alert(1)', 'data:text/html,<script>', 'https://user:password@example.com', 'https://']) expect(messageLinks(text).some(part => part.href)).toBe(false);
  });
  it('renders labeled HTTPS links and bold text in scout messages', () => {
    expect(messageLinks('**Active fights** · [Join E9EEB7](https://example.com/?server=id)')).toEqual([
      { text: 'Active fights', bold: true },
      { text: ' · ' },
      { text: 'Join E9EEB7', href: 'https://example.com/?server=id' },
    ]);
  });
  it('preserves unsafe labeled links as plain text', () => {
    for (const text of ['[Example](javascript:alert(1))', '[Example](https://user:password@example.com)', '[Example]()']) {
      expect(messageLinks(text)).toEqual([{ text }]);
    }
  });
  it('renders markup as text and exposes canonical hostnames for confirmation', () => {
    const text = '<img src=x onerror=alert(1)> https://Ã©xample.com';
    expect(messageLinks(text).map(part => part.text).join('')).toBe(text);
    expect(new URL(messageLinks(text).find(part => part.href)!.href!).hostname).toBe('xn--xample-9ua.com');
  });
});
