export interface TextPart { text: string; href?: string; bold?: boolean }

function safeHref(candidate: string): string | undefined {
  try {
    const url = new URL(/^www\./i.test(candidate) ? `https://${candidate}` : candidate);
    if (['http:', 'https:'].includes(url.protocol) && url.hostname && !url.username && !url.password) return url.href;
  } catch { /* Leave unparseable destinations as text. */ }
}

export function messageLinks(content: string): TextPart[] {
  const parts: TextPart[] = [];
  const markup = /\[([^\[\]\r\n]+)\]\((<[^<>\s]+>|[^()\s]*(?:\([^()\s]*\)[^()\s]*)*)\)|\*\*([^*\r\n]+)\*\*/gu;
  let cursor = 0;
  for (const match of content.matchAll(markup)) {
    parts.push(...plainLinks(content.slice(cursor, match.index)));
    const [, label, destination, bold] = match;
    if (label !== undefined) {
      const href = safeHref(destination.replace(/^<|>$/gu, ''));
      parts.push(href ? { text: label, href } : { text: match[0] });
    } else {
      parts.push({ text: bold, bold: true });
    }
    cursor = match.index + match[0].length;
  }
  parts.push(...plainLinks(content.slice(cursor)));
  return parts;
}

function plainLinks(content: string): TextPart[] {
  const parts: TextPart[] = [];
  const expression = /\b(?:https?:\/\/|www\.)[^\s<>"\u0000-\u001f]+/giu;
  let cursor = 0;
  for (const match of content.matchAll(expression)) {
    let candidate = match[0].replace(/[.,!?;:]+$/u, '');
    // Preserve balanced parentheses in URLs, but leave sentence punctuation as plain text.
    for (const [open, close] of [['(', ')'], ['[', ']'], ['{', '}']]) {
      while (candidate.endsWith(close) && candidate.split(close).length > candidate.split(open).length) candidate = candidate.slice(0, -1);
    }
    const href = safeHref(candidate);
    if (!href) continue;
    if (match.index > cursor) parts.push({ text: content.slice(cursor, match.index) });
    parts.push({ text: candidate, href }); cursor = match.index + candidate.length;
  }
  if (cursor < content.length) parts.push({ text: content.slice(cursor) });
  return parts;
}
