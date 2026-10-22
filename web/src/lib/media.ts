export function mediaUrl(path: string | null, origin: string): string {
  if (!path) return '';
  try { const url = new URL(path, origin); return ['http:', 'https:'].includes(url.protocol) && url.origin === new URL(origin).origin && /\/api\/files\//.test(url.pathname) ? url.href : ''; } catch { return ''; }
}
export function bytes(value: number) { return value >= 1048576 ? `${(value / 1048576).toFixed(1)} MB` : `${Math.ceil(value / 1024)} KB`; }
export function fileInput(node: HTMLElement, options: { enabled: boolean; add: (files: File[]) => void }) {
  const paste = (event: ClipboardEvent) => { const files = [...(event.clipboardData?.files ?? [])]; if (options.enabled && files.length) { event.preventDefault(); options.add(files); } };
  const drag = (event: DragEvent) => { if (options.enabled && event.dataTransfer?.types.includes('Files')) event.preventDefault(); };
  const drop = (event: DragEvent) => { if (event.dataTransfer?.types.includes('Files')) { event.preventDefault(); if (options.enabled) options.add([...event.dataTransfer.files]); } };
  node.addEventListener('paste', paste); node.addEventListener('dragover', drag); node.addEventListener('drop', drop);
  return { update(next: typeof options) { options = next; }, destroy() { node.removeEventListener('paste', paste); node.removeEventListener('dragover', drag); node.removeEventListener('drop', drop); } };
}

export const uploadExtensions = ['png', 'jpg', 'jpeg', 'gif', 'webp', 'mp4', 'webm', 'mp3', 'ogg', 'wav', 'flac', 'm4a', 'txt', 'md', 'csv', 'log', 'json', 'pdf'];
export const uploadAccept = uploadExtensions.map(extension => '.' + extension).join(',');
export function allowedUpload(filename: string) { const suffix = filename.split('.').at(-1)?.toLowerCase(); return filename.includes('.') && !!suffix && uploadExtensions.includes(suffix); }
