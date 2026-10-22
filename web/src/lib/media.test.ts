import { describe, expect, it, vi } from 'vitest';
import { mediaUrl, fileInput, allowedUpload } from './media';
import { contentLimits } from './limits';
describe('attachment boundaries', () => {
  it('allows named download formats while rejecting active or executable extensions', () => {
    for (const name of ['notes.txt','README.MD','data.csv','host.log','settings.json','document.PDF','photo.jpg','music.MP3','voice.ogg','recording.webm','music.m4a','sound.wav','sound.flac']) expect(allowedUpload(name)).toBe(true);
    for (const name of ['script.exe','page.html','drawing.svg','run.js','notes.txt.exe','unnamed','trailing.']) expect(allowedUpload(name)).toBe(false);
  });
  it('resolves signed host-relative media URLs while preserving their signature', () => {
    expect(mediaUrl('/s/dev/api/files/1/file/a.webp?exp=1&sig=x', 'https://chat.test')).toBe('https://chat.test/s/dev/api/files/1/file/a.webp?exp=1&sig=x');
  });
  it('rejects active schemes, foreign origins, and nonmedia paths', () => {
    for (const path of ['javascript:alert(1)', '//other.test/s/dev/api/files/1', '/auth/logout', 'data:image/svg+xml,<svg/>']) expect(mediaUrl(path, 'https://chat.test')).toBe('');
  });
  it('allows attachment-only messages while retaining text validation', () => {
    expect(contentLimits('', 1).error).toBe('');
    expect(contentLimits('').error).not.toBe('');
    expect(contentLimits('x'.repeat(4001), 1).error).not.toBe('');
  });
  it('handles dropped/pasted files only when enabled and removes listeners on teardown', () => {
    const target = new EventTarget(); const add = vi.fn(); const file = new File(['test'], 'test.png', { type: 'image/png' });
    const action = fileInput(target as HTMLElement, { enabled: true, add });
    const drop = new Event('drop', { cancelable: true }); Object.defineProperty(drop, 'dataTransfer', { value: { types: ['Files'], files: [file] } });
    target.dispatchEvent(drop); expect(add).toHaveBeenCalledWith([file]); expect(drop.defaultPrevented).toBe(true);
    const paste = new Event('paste', { cancelable: true }); Object.defineProperty(paste, 'clipboardData', { value: { files: [file] } });
    action.update({ enabled: false, add }); target.dispatchEvent(paste); expect(add).toHaveBeenCalledTimes(1); expect(paste.defaultPrevented).toBe(false);
    action.update({ enabled: true, add }); target.dispatchEvent(paste); expect(add).toHaveBeenCalledTimes(2);
    action.destroy(); target.dispatchEvent(paste); expect(add).toHaveBeenCalledTimes(2);
  });
});
