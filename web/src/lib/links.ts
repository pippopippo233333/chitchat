export interface TextPart { text: string; href?: string }

export function messageLinks(content: string): TextPart[] {
  const parts: TextPart[] = [];
  const expression = /\b(?:https?:\/\/|www\.)[^\s<>"\u0000-\u001f]+/giu;
  let cursor = 0;
  for (const match of content.matchAll(expression)) {
    let candidate = match[0].replace(/[.,!?;:]+$/u, '');
    // Preserve balanced parentheses in URLs, but leave sentence punctuation as plain text.
    for (const [open, close] of [['(', ')'], ['[', ']'], ['{', '}']]) {
      while (candidate.endsWith(close) && candidate.split(close).length > candidate.split(open).length) candidate = candidate.slice(0, -1);
    }
    let url: URL;
    try { url = new URL(/^www\./i.test(candidate) ? `https://${candidate}` : candidate); } catch { continue; }
    if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password || !url.hostname) continue;
    if (match.index > cursor) parts.push({ text: content.slice(cursor, match.index) });
    parts.push({ text: candidate, href: url.href }); cursor = match.index + candidate.length;
  }
  if (cursor < content.length) parts.push({ text: content.slice(cursor) });
  return parts;
}
