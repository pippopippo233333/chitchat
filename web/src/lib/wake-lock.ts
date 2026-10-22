interface ScreenLock { released: boolean; release(): Promise<void>; addEventListener(type: 'release', listener: () => void): void }
interface LockProvider { request(type: 'screen'): Promise<ScreenLock> }
interface Visibility { visibilityState: string; addEventListener(type: 'visibilitychange', listener: () => void): void; removeEventListener(type: 'visibilitychange', listener: () => void): void }
// A best-effort screen lock: denial must never interrupt chat or voice or trigger a retry loop.
export class VoiceWakeLock {
  private active = false;
  private disposed = false;
  private lock?: ScreenLock;
  private pending = false;
  private version = 0;
  constructor(private provider: LockProvider | undefined, private page: Visibility) { page.addEventListener('visibilitychange', this.visibility); }
  private visibility = () => { this.version++; this.drop(); if (this.page.visibilityState === 'visible') void this.acquire(); };
  setActive(active: boolean) { if (this.active === active || this.disposed) return; this.active = active; this.version++; if (!active) this.drop(); else void this.acquire(); }
  private drop() { const lock = this.lock; this.lock = undefined; if (lock && !lock.released) void lock.release().catch(() => {}); }
  private async acquire() {
    if (!this.provider || this.pending || this.lock || !this.active || this.disposed || this.page.visibilityState !== 'visible') return;
    const version = this.version; this.pending = true;
    try {
      const lock = await this.provider.request('screen');
      if (version !== this.version || !this.active || this.disposed || this.page.visibilityState !== 'visible') { await lock.release(); return; }
      this.lock = lock; lock.addEventListener('release', () => { if (this.lock === lock) this.lock = undefined; });
    } catch { /* Unsupported policy, low battery, or permission denial: use normal phone settings. */ }
    finally { this.pending = false; if (version !== this.version && this.active && !this.disposed) void this.acquire(); }
  }
  dispose() { this.disposed = true; this.active = false; this.version++; this.drop(); this.page.removeEventListener('visibilitychange', this.visibility); }
}
