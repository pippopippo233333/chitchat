import type { Api } from './api';

export type ConnectionStatus = 'connecting' | 'connected' | 'reconnecting' | 'offline';
export interface Envelope { type: string; data: any }

export class LiveConnection {
  private socket?: WebSocket;
  private timer?: ReturnType<typeof setTimeout>;
  private attempt = 0;
  private stopped = false;
  private lastTyping = 0;
  private lastError = '';
  private handshake?: ReturnType<typeof setTimeout>;
  constructor(private api: Api, private onEvent: (event: Envelope) => void, private onStatus: (status: ConnectionStatus) => void, private onFatal: (message: string) => void) { if (typeof document !== 'undefined') document.addEventListener('visibilitychange', this.visibility); }
  connect() {
    if (this.stopped) return;
    this.onStatus(this.attempt ? 'reconnecting' : 'connecting');
    const socket = new WebSocket(this.api.socketUrl());
    this.socket = socket;
    this.handshake = setTimeout(() => socket.close(4000, 'Handshake timeout'), 15000);
    socket.onopen = () => { if (!this.stopped) socket.send(JSON.stringify({ type: 'identify', data: { token: this.api.token } })); };
    socket.onmessage = event => {
      if (this.stopped) return;
      let frame: Envelope;
      try { frame = JSON.parse(event.data); } catch { return; }
      if (!frame || typeof frame.type !== 'string' || !frame.data) return;
      if (frame.type === 'error') this.lastError = frame.data.message ?? '';
      if (frame.type === 'ready') { clearTimeout(this.handshake); this.attempt = 0; this.lastError = ''; this.onStatus('connected'); this.visibility(); }
      this.onEvent(frame);
    };
    socket.onclose = event => {
      clearTimeout(this.handshake);
      if (this.stopped) return;
      if (event.code === 4001 || event.code === 4003) {
        this.stopped = true;
        this.onStatus('offline');
        this.onFatal(this.lastError || (event.code === 4001 ? 'Your session expired. Please sign in again.' : 'Your access to this server has been removed.'));
        return;
      }
      this.onStatus('reconnecting');
      const delay = [1000, 2000, 5000, 10000][Math.min(this.attempt++, 3)];
      this.timer = setTimeout(() => this.connect(), delay);
    };
    socket.onerror = () => socket.close();
  }
  typing(channel: string) {
    if (this.socket?.readyState !== WebSocket.OPEN || Date.now() - this.lastTyping < 5000) return;
    this.lastTyping = Date.now();
    this.socket.send(JSON.stringify({ type: 'typing', data: { channel_id: channel } }));
  }
  send(type: string, data: unknown): boolean {
    if (this.stopped || this.socket?.readyState !== WebSocket.OPEN) return false;
    this.socket.send(JSON.stringify({ type, data })); return true;
  }
  // Tells the server whether this page is on screen: it sends Web Push only while none of yours is.
  private visibility = () => { if (typeof document !== 'undefined') this.send('visibility', { visible: document.visibilityState === 'visible' }); };
  stop() { if (typeof document !== 'undefined') document.removeEventListener('visibilitychange', this.visibility); this.stopped = true; clearTimeout(this.timer); clearTimeout(this.handshake); this.socket?.close(); }
}
