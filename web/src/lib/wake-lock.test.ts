import { describe, expect, it, vi } from 'vitest';
import { VoiceWakeLock } from './wake-lock';
function fixture() {
  const page = { visibilityState: 'visible', addEventListener: vi.fn(), removeEventListener: vi.fn() };
  const locks: { released: boolean; release: ReturnType<typeof vi.fn>; addEventListener: ReturnType<typeof vi.fn> }[] = [];
  const provider = { request: vi.fn(async () => { const lock = { released: false, release: vi.fn(async () => { lock.released = true; }), addEventListener: vi.fn() }; locks.push(lock); return lock; }) };
  const client = new VoiceWakeLock(provider,page);
  const visible = async (state: string) => { page.visibilityState = state; page.addEventListener.mock.calls[0][1](); await Promise.resolve(); };
  return {page,provider,client,locks,visible};
}
describe('visible chat screen wake lock', () => {
  it('holds while chat is active, releases when hidden, reacquires on return and releases when disabled', async () => {
    const {client,provider,locks,visible,page} = fixture(); expect(provider.request).not.toHaveBeenCalled();
    client.setActive(true); await Promise.resolve(); expect(provider.request).toHaveBeenCalledWith('screen');
    client.setActive(true); expect(provider.request).toHaveBeenCalledTimes(1);
    await visible('hidden'); expect(locks[0].release).toHaveBeenCalledOnce();
    await visible('visible'); expect(provider.request).toHaveBeenCalledTimes(2);
    client.setActive(false); expect(locks[1].release).toHaveBeenCalledOnce();
    client.dispose(); expect(page.removeEventListener).toHaveBeenCalledOnce();
  });
  it('releases a lock granted after leaving instead of retaining it', async () => {
    let finish!: (lock: any) => void; const release = vi.fn(async () => {});
    const client = new VoiceWakeLock({request: () => new Promise(resolve => finish = resolve)}, {visibilityState:'visible',addEventListener(){},removeEventListener(){}});
    client.setActive(true); client.setActive(false); finish({released:false,release,addEventListener(){}}); await Promise.resolve(); expect(release).toHaveBeenCalledOnce(); client.dispose();
  });
  it('tolerates denial and unsupported browsers without interrupting or retrying voice', async () => {
    const {page} = fixture(); const request = vi.fn(async () => {throw Error('Denied');});
    const client = new VoiceWakeLock({request},page); client.setActive(true); await Promise.resolve(); await Promise.resolve(); expect(request).toHaveBeenCalledOnce(); client.dispose();
    const unsupported = new VoiceWakeLock(undefined,page); expect(() => unsupported.setActive(true)).not.toThrow(); unsupported.dispose();
  });
});
