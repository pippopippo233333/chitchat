// App colour theme for this device. The build turns every colour in the CSS into
// `var(--c-<hex>, #<hex>)` (vite.config.ts); a theme sets those variables to the same colour
// moved to the chosen hue, keeping its lightness so contrast stays as designed.

export const themeKey = 'chitchat.theme';
export const presets = [
  { name: 'Mint', color: null },
  { name: 'Blue', color: '#8ab4ff' },
  { name: 'Purple', color: '#c3a6ff' },
  { name: 'Pink', color: '#ff9fd0' },
  { name: 'Red', color: '#ff8f8f' },
  { name: 'Orange', color: '#ffb27a' },
  { name: 'Yellow', color: '#f0d77a' },
  { name: 'Gray', color: '#c4c4c4' },
] as const;

type Lch = [number, number, number];
/** Chroma of the default accent (#a4e4c2): the theme colour's chroma is relative to it. */
const accentChroma = 0.0803;

export function parseHex(hex: string) {
  const h = hex.replace('#', '').toLowerCase();
  if (!/^([0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})$/.test(h)) return null;
  const full = h.length <= 4 ? [...h].map(c => c + c).join('') : h;
  const n = (i: number) => parseInt(full.slice(i, i + 2), 16) / 255;
  return { rgb: [n(0), n(2), n(4)] as [number, number, number], alpha: full.length === 8 ? n(6) : 1 };
}

const lin = (c: number) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const gam = (c: number) => (c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055);

export function toOklch(hex: string): { lch: Lch; alpha: number } {
  const p = parseHex(hex) ?? { rgb: [0, 0, 0] as [number, number, number], alpha: 1 };
  const [r, g, b] = p.rgb.map(lin);
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
  const A = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
  const B = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
  return { lch: [L, Math.hypot(A, B), ((Math.atan2(B, A) * 180) / Math.PI + 360) % 360], alpha: p.alpha };
}

function rgbOf([L, C, H]: Lch) {
  const a = C * Math.cos((H * Math.PI) / 180), b = C * Math.sin((H * Math.PI) / 180);
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
  return [4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s, -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s, -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s];
}

/** The nearest displayable colour: lowers chroma until it fits in sRGB. */
export function toHex(lch: Lch) {
  const [L, H] = [lch[0], lch[2]];
  let C = lch[1], rgb = rgbOf([L, C, H]);
  while (C > 0 && rgb.some(v => v < -0.0005 || v > 1.0005)) { C = Math.max(0, C - 0.004); rgb = rgbOf([L, C, H]); }
  return '#' + rgb.map(v => Math.round(gam(Math.min(1, Math.max(0, v))) * 255).toString(16).padStart(2, '0')).join('');
}

/** Reds, oranges and yellows that mean "error" or "warning" keep their colour in every theme. */
function semantic([, C, H]: Lch) { return C > 0.05 && (H < 110 || H > 345); }

/** A colour of the default design, moved to the theme colour. */
export function themed(hex: string, theme: string) {
  const { lch, alpha } = toOklch(hex), t = toOklch(theme).lch;
  if (semantic(lch)) return null;
  const k = Math.min(2.5, t[1] / accentChroma);
  const alphaHex = alpha < 1 ? Math.round(alpha * 255).toString(16).padStart(2, '0') : '';
  return toHex([lch[0], Math.min(0.37, lch[1] * k), t[2]]) + alphaHex;
}

function cssColours(): string[] {
  const found = new Set<string>();
  const walk = (rules: CSSRuleList) => { for (const rule of Array.from(rules)) { for (const m of rule.cssText.matchAll(/--c-([0-9a-f]{3,8})\b/g)) found.add(m[1]); if ('cssRules' in rule) walk((rule as CSSGroupingRule).cssRules); } };
  for (const sheet of Array.from(document.styleSheets)) { try { walk(sheet.cssRules); } catch { /* A cross-origin sheet has no colours of ours. */ } }
  return [...found];
}

export function loadTheme(): string | null {
  try { const v = localStorage.getItem(themeKey); return v && parseHex(v) ? v : null; } catch { return null; }
}

export function storeTheme(color: string | null) {
  try { if (color) localStorage.setItem(themeKey, color); else localStorage.removeItem(themeKey); } catch { /* Applies for this visit only. */ }
}

/** Recolours the page (`null` = the default design). */
export function applyTheme(color: string | null) {
  const root = document.documentElement.style;
  for (const name of Array.from(root)) if (name.startsWith('--c-')) root.removeProperty(name);
  const bar = document.querySelector('meta[name="theme-color"]');
  if (!color) { bar?.setAttribute('content', '#111b20'); return; }
  for (const hex of cssColours()) { const value = themed('#' + hex, color); if (value) root.setProperty('--c-' + hex, value); }
  bar?.setAttribute('content', themed('#111b20', color) ?? '#111b20');
}
