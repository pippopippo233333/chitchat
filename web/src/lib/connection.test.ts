import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Api } from './api';
import { LiveConnection } from './connection';
class FakeSocket {
  static OPEN = 1;
  static instances: FakeSocket[] = [];
  readyState = 1;
  sent: string[] = [];
  onopen?: () => void;
  onmessage?: (event: { data: string }) => void;
  onclose?: (event: { code: number }) => void;
  onerror?: () => void;
  constructor(public url: string) { FakeSocket.instances.push(this); }
  send(value: string) { this.sent.push(value); }
  close(code = 1000) { this.onclose?.({ code }); }
  receive(type: string, data: unknown) { this.onmessage?.({ data: JSON.stringify({ type, data }) }); }
}
beforeEach(() => { vi.useFakeTimers(); FakeSocket.instances = []; vi.stubGlobal('WebSocket', FakeSocket); });
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });
describe('WebSocket lifecycle', () => {
  it('identifies in a frame and reconnects with a new ready snapshot', () => {
    const events = vi.fn(), status = vi.fn(); const connection = new LiveConnection(new Api('http://localhost', 'dev', 'private'), events, status, vi.fn());
    connection.connect(); const first = FakeSocket.instances[0]; first.onopen?.();
    expect(JSON.parse(first.sent[0])).toEqual({ type: 'identify', data: { token: 'private' } });
    first.receive('ready', { channels: [] }); expect(status).toHaveBeenLastCalledWith('connected');
    first.close(1006); vi.advanceTimersByTime(1000); expect(FakeSocket.instances).toHaveLength(2);
    connection.stop(); vi.advanceTimersByTime(20000); expect(FakeSocket.instances).toHaveLength(2);
  });
  it.each([4001, 4003])('never retries access-denied close code %i', code => {
    const fatal = vi.fn(); const connection = new LiveConnection(new Api('http://localhost', 'dev'), vi.fn(), vi.fn(), fatal);
    connection.connect(); const socket = FakeSocket.instances[0]; socket.receive('error', { message: 'Access removed' }); socket.close(code);
    vi.advanceTimersByTime(60000); expect(FakeSocket.instances).toHaveLength(1); expect(fatal).toHaveBeenCalledWith('Access removed');
  });
  it('throttles typing and ignores frames after stopping a session', () => {
    const events = vi.fn(); const connection = new LiveConnection(new Api('http://localhost', 'dev'), events, vi.fn(), vi.fn());
    connection.connect(); const socket = FakeSocket.instances[0]; connection.typing('1'); connection.typing('1'); expect(socket.sent).toHaveLength(1);
    vi.advanceTimersByTime(5000); connection.typing('1'); expect(socket.sent).toHaveLength(2);
    connection.stop(); socket.receive('message_create', { id: '1' }); expect(events).not.toHaveBeenCalled();
  });
});
