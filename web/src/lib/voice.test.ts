import { afterEach, describe, expect, it } from 'vitest';
import { VoiceClient, roomUploadMbps, captureOptions } from './voice';

class Track { id = Math.random().toString(); enabled = true; stopped = false; onended?: () => void; constructor(public kind = 'audio') {} stop() { this.stopped = true; } }
class Stream { id = 'stream'; constructor(public tracks: Track[] = []) {} getTracks() { return this.tracks; } getAudioTracks() { return this.tracks.filter(track => track.kind === 'audio'); } getVideoTracks() { return this.tracks.filter(track => track.kind === 'video'); } addTrack(track: Track) { this.tracks.push(track); } removeTrack(track: Track) { this.tracks = this.tracks.filter(item => item !== track); } }
class Sender { params = { encodings: [{}] }; constructor(public track: Track | null) {} async replaceTrack(track: Track | null) { this.track = track; } getParameters() { return this.params; } async setParameters(params: typeof this.params) { this.params = params; } }
class Peer {
  connectionState = 'new'; signalingState = 'stable'; localDescription?: RTCSessionDescriptionInit;
  onconnectionstatechange?: () => void; ontrack?: (event: any) => void;
  remote: RTCSessionDescriptionInit[] = []; senders: Sender[] = [];
  closed = false;
  addTrack(track: Track) { const sender = new Sender(track); this.senders.push(sender); return sender; }
  transceivers: { kind: string; direction?: string }[] = [];
  getTransceivers() { return this.senders.map((sender, index) => ({ sender, mid: String(index) })); }
  addTransceiver(kind: string | Track, options?: RTCRtpTransceiverInit) { this.transceivers.push({ kind: typeof kind === 'string' ? kind : kind.kind, direction: options?.direction }); return { sender: typeof kind === 'string' ? new Sender(null) : this.addTrack(kind) }; }
  async createOffer() { return { type: 'offer', sdp: 'offer' }; }
  async createAnswer() { return { type: 'answer', sdp: 'answer' }; }
  async setLocalDescription(description: RTCSessionDescriptionInit) { this.localDescription = description; this.signalingState = description.type === 'offer' ? 'have-local-offer' : 'stable'; }
  async setRemoteDescription(description: RTCSessionDescriptionInit) { this.remote.push(description); await Promise.resolve(); this.signalingState = description.type === 'offer' ? 'have-remote-offer' : 'stable'; }
  close() { this.closed = true; }
  connected() { this.connectionState = 'connected'; this.onconnectionstatechange?.(); }
}
const clients: VoiceClient[] = [];
afterEach(() => { clients.splice(0).forEach(client => client.leave(false)); });
function fixture(withSharedAudio = false, missingMic = false) {
  const peer = new Peer(), micTrack = new Track(), screenTrack = new Track('video'), sharedAudio = new Track('audio'); const signals: { type: string; data: any }[] = [];
  const client = new VoiceClient((type, data) => { signals.push({ type, data }); return true; }, { peer: () => peer as unknown as RTCPeerConnection, microphone: async () => { if (missingMic) throw new Error('No microphone'); return new Stream([micTrack]) as unknown as MediaStream; }, display: async () => new Stream(withSharedAudio ? [screenTrack, sharedAudio] : [screenTrack]) as unknown as MediaStream, stream: () => new Stream() as unknown as MediaStream });
  clients.push(client); return { client, peer, micTrack, screenTrack, sharedAudio, signals };
}
describe('voice lifecycle and negotiation', () => {
  it('requests window/system audio options only when shared sound is selected', () => {
    expect(captureOptions('720p',true)).toMatchObject({audio:{channelCount:{ideal:2},echoCancellation:false,noiseSuppression:false,autoGainControl:false},systemAudio:'include',windowAudio:'window'});
    expect(captureOptions('720p',false)).toMatchObject({audio:false,systemAudio:'exclude',windowAudio:'exclude'});
  });
  it('joins once, mutes physical audio for PTT/deaf, and releases all media on leave', async () => {
    const { client, peer, micTrack, signals } = fixture(); client.setPtt(true);
    await client.join('room'); expect(signals.map(signal => signal.type)).toEqual(['voice_join']); expect(micTrack.enabled).toBe(false);
    await client.handleSdp({ type: 'answer', sdp: 'initial' }); peer.connected(); expect(client.view.phase).toBe('connected');
    client.press(true); expect(micTrack.enabled).toBe(true); client.toggleDeaf(); expect(micTrack.enabled).toBe(false);
    client.leave(); expect(peer.closed).toBe(true); expect(micTrack.stopped).toBe(true); expect(client.view.channelId).toBeNull();
  });
  it('ignores colliding host offers and answers stable host offers in order', async () => {
    const { client, peer, signals } = fixture(); await client.join('room');
    await client.handleSdp({ type: 'offer', sdp: 'discarded-glare' }); expect(peer.remote).toHaveLength(0);
    await client.handleSdp({ type: 'answer', sdp: 'initial' });
    await Promise.all([client.handleSdp({ type: 'offer', sdp: 'host-one' }), client.handleSdp({ type: 'offer', sdp: 'host-two' })]);
    expect(peer.remote.map(description => description.sdp)).toEqual(['initial', 'host-one', 'host-two']);
    expect(signals.filter(signal => signal.type === 'voice_sdp').map(signal => signal.data.type)).toEqual(['answer', 'answer']);
  });
  it('stops a microphone obtained after the user already left', async () => {
    let finish!: (stream: MediaStream) => void; const track = new Track(); const signals: string[] = [];
    const client = new VoiceClient(type => { signals.push(type); return true; }, { peer: () => new Peer() as unknown as RTCPeerConnection, microphone: () => new Promise(resolve => finish = resolve) }); clients.push(client);
    const pending = client.join('room'); client.leave(); finish(new Stream([track]) as unknown as MediaStream); await pending;
    expect(track.stopped).toBe(true); expect(signals).not.toContain('voice_join');
  });
  it('maps remote audio and video separately to the server session id', async () => {
    const { client, peer } = fixture(); await client.join('room');
    peer.ontrack?.({ streams: [{ id: 'person-session' }], track: new Track('audio') }); peer.ontrack?.({ streams: [{ id: 'person-session' }], track: new Track('video') });
    expect(client.view.remotes).toHaveLength(1); expect(client.view.remotes[0].audio.getTracks()).toHaveLength(1); expect(client.view.remotes[0].video.getTracks()).toHaveLength(1);
    client.syncStates([], 'me'); expect(client.view.remotes).toHaveLength(0);
  });
  it('shares with one renegotiation, detaches tracks on stop, and reuses the sender', async () => {
    const { client, peer, screenTrack, signals } = fixture(); await client.join('room', { speak: true, stream: true }); await client.handleSdp({ type: 'answer', sdp: 'initial' }); peer.connected();
    const sharing = client.startShare('720p'); for (let i = 0; i < 15 && !signals.some(signal => signal.type === 'voice_sdp' && signal.data.type === 'offer'); i++) await Promise.resolve();
    await client.handleSdp({ type: 'answer', sdp: 'screen-answer' }); await sharing; expect(client.view.sharing).toBe(true);
    expect(peer.senders[1].params.encodings[0]).toMatchObject({ maxBitrate: 2500000, maxFramerate: 30 });
    await client.stopShare(); expect(screenTrack.stopped).toBe(true); expect(peer.senders[1].track).toBeNull();
    const offers = signals.filter(signal => signal.type === 'voice_sdp' && signal.data.type === 'offer').length;
    await client.startShare('1080p'); expect(peer.senders).toHaveLength(2); expect(signals.filter(signal => signal.type === 'voice_sdp' && signal.data.type === 'offer')).toHaveLength(offers);
    client.setCapabilities(true, false); expect(client.view.sharing).toBe(false);
  });
  it('joins listen-only without asking for a microphone and handles fatal voice endings', async () => {
    const { client, signals, micTrack } = fixture(); await client.join('room', { speak: false, stream: false });
    expect(client.view.microphone).toBe(false); expect(signals[0].data.self_mute).toBe(true);
    client.handleEnd('Room is full.'); expect(client.view.error).toBe('Room is full.'); expect(client.view.phase).toBe('idle'); expect(micTrack.stopped).toBe(false);
  });
  it('estimates SFU upload bandwidth at room capacity', () => { expect(roomUploadMbps(10)).toBe(3.6); expect(roomUploadMbps(20)).toBe(15.2); });
  it.each(['NotFoundError', 'NotAllowedError', 'NotReadableError'])('joins receive-only after microphone %s and can still share', async name => {
    const peer = new Peer(); const signals: { type: string; data: any }[] = [];
    const client = new VoiceClient((type, data) => { signals.push({ type, data }); return true; }, {
      peer: () => peer as unknown as RTCPeerConnection,
      microphone: async () => { throw Object.assign(new Error('Microphone unavailable'), { name }); },
      display: async () => new Stream([new Track('video')]) as unknown as MediaStream,
    }); clients.push(client);
    await client.join('room', { speak: true, stream: true });
    expect(client.view.phase).toBe('joining'); expect(client.view.microphone).toBe(false);
    expect(client.view.microphoneNotice).toContain('No microphone, you can listen only');
    expect(peer.transceivers).toEqual([{ kind: 'audio', direction: 'recvonly' }]);
    expect(signals[0]).toMatchObject({ type: 'voice_join', data: { self_mute: true } });
    await client.handleSdp({ type: 'answer', sdp: 'initial' }); peer.connected();
    client.toggleMute(); client.setPtt(true); client.press(true);
    expect(signals.filter(signal => signal.type === 'voice_state').every(signal => signal.data.self_mute !== false)).toBe(true);
    const sharing = client.startShare('720p');
    for (let i = 0; i < 15 && !signals.some(signal => signal.type === 'voice_sdp'); i++) await Promise.resolve();
    await client.handleSdp({ type: 'answer', sdp: 'screen-answer' }); await sharing;
    expect(client.view.sharing).toBe(true); expect(client.view.phase).toBe('connected');
  });
  it('does not join when a pending microphone rejection arrives after leaving', async () => {
    let reject!: (error: Error) => void; const signals: string[] = [];
    const client = new VoiceClient(type => { signals.push(type); return true; }, { peer: () => new Peer() as unknown as RTCPeerConnection, microphone: () => new Promise((_, fail) => reject = fail) }); clients.push(client);
    const joining = client.join('room'); client.leave(); reject(new Error('Permission denied')); await joining;
    expect(client.view.phase).toBe('idle'); expect(signals).not.toContain('voice_join');
  });
  it('keeps shared sound independent of microphone mute/PTT and honors shared mute/deafen', async () => {
    const { client, peer, signals, micTrack, sharedAudio } = fixture(true);
    await client.join('room', { speak: true, stream: true }); await client.handleSdp({ type: 'answer', sdp: 'initial' }); peer.connected();
    client.toggleMute(); expect(micTrack.enabled).toBe(false);
    const sharing = client.startShare('720p');
    for (let i=0; i<15 && !signals.some(signal => signal.type === 'voice_sdp'); i++) await Promise.resolve();
    await client.handleSdp({ type: 'answer', sdp: 'share' }); await sharing;
    expect(signals).toContainEqual({ type: 'voice_state', data: { streaming: true, screen_mids: ['1', '2'] } });
    expect(peer.transceivers).toEqual([{kind:'video',direction:'sendonly'},{kind:'audio',direction:'sendonly'}]);
    expect(peer.senders[2].params.encodings[0]).toMatchObject({ maxBitrate: 128000 });
    expect(client.view.shareAudioAvailable).toBe(true); expect(sharedAudio.enabled).toBe(true); expect(micTrack.enabled).toBe(false);
    client.setPtt(true); client.press(false); expect(sharedAudio.enabled).toBe(true);
    client.toggleShareAudio(); expect(sharedAudio.enabled).toBe(false); client.toggleShareAudio(); expect(sharedAudio.enabled).toBe(true);
    client.toggleDeaf(); expect(sharedAudio.enabled).toBe(false); client.toggleDeaf(); expect(sharedAudio.enabled).toBe(true);
    client.setCapabilities(true, false); expect(sharedAudio.stopped).toBe(true); expect(client.view.shareAudioAvailable).toBe(false);
  });
  it('does not transmit shared audio when capture audio is opted out', async () => {
    const { client, peer, signals, sharedAudio } = fixture(true);
    await client.join('room', { speak: true, stream: true }); await client.handleSdp({ type: 'answer', sdp: 'initial' }); peer.connected();
    const sharing = client.startShare('720p', false);
    for (let i=0; i<15 && !signals.some(signal => signal.type === 'voice_sdp'); i++) await Promise.resolve();
    await client.handleSdp({ type: 'answer', sdp: 'share' }); await sharing;
    expect(sharedAudio.stopped).toBe(true); expect(peer.senders).toHaveLength(2); expect(client.view.shareAudioAvailable).toBe(false);
  });

  it('shares sound with a missing microphone while keeping self_mute true', async () => {
    const { client, peer, signals, sharedAudio } = fixture(true, true);
    await client.join('room', { speak: true, stream: true }); await client.handleSdp({ type: 'answer', sdp: 'initial' }); peer.connected();
    const sharing = client.startShare('720p');
    for (let i=0; i<15 && !signals.some(signal => signal.type === 'voice_sdp'); i++) await Promise.resolve();
    await client.handleSdp({ type: 'answer', sdp: 'share' }); await sharing;
    expect(client.view.microphone).toBe(false); expect(sharedAudio.enabled).toBe(true);
    expect(signals).toContainEqual({ type: 'voice_state', data: { streaming: true, screen_mids: ['0', '1'] } });
    expect(signals.filter(signal => signal.type === 'voice_state').every(signal => signal.data.self_mute !== false)).toBe(true);
    await client.stopShare(); expect(sharedAudio.stopped).toBe(true); expect(peer.senders.every(sender => sender.track === null)).toBe(true);
  });
  it('keeps multiple remote audio tracks separately playable and removes ended tracks', async () => {
    const { client, peer } = fixture(); await client.join('room');
    const mic = new Track(), sound = new Track();
    peer.ontrack?.({ streams: [{id: 'same-session'}], track: mic }); peer.ontrack?.({ streams: [{id: 'same-session'}], track: sound });
    expect(client.view.remotes[0].audioTracks).toHaveLength(2);
    expect(client.view.remotes[0].audioTracks.map(item => item.stream.getAudioTracks().length)).toEqual([1,1]);
    sound.onended?.(); expect(client.view.remotes[0].audioTracks.map(item => item.id)).toEqual([mic.id]);
  });

});
