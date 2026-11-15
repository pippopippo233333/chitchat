const assets = import.meta.glob<string>('../../node_modules/flag-icons/flags/4x3/[a-z][a-z].svg', {
  eager: true, import: 'default', query: '?url&no-inline',
});

interface FlagPart { text: string; code?: string; url?: string }

export function flagParts(text: string): FlagPart[] {
  const parts: FlagPart[] = [];
  let cursor = 0;
  for (const match of text.matchAll(/[\u{1F1E6}-\u{1F1FF}]{2}/gu)) {
    const code = Array.from(match[0], letter => String.fromCharCode(letter.codePointAt(0)! - 0x1f1e6 + 97)).join('');
    const url = assets[`../../node_modules/flag-icons/flags/4x3/${code}.svg`];
    if (!url) continue;
    if (match.index > cursor) parts.push({ text: text.slice(cursor, match.index) });
    parts.push({ text: match[0], code: code.toUpperCase(), url });
    cursor = match.index + match[0].length;
  }
  if (cursor < text.length) parts.push({ text: text.slice(cursor) });
  return parts;
}
