import { writable } from 'svelte/store';
import type { VoiceState } from './api';

export interface RemoteMedia { sessionId: string; audio: MediaStream; audioTracks: { id: string; stream: MediaStream }[]; video: MediaStream }
export interface VoiceView { phase: 'idle' | 'joining' | 'connected'; channelId: string | null; microphone: boolean; microphoneNotice: string; muted: boolean; deafened: boolean; suppressed: boolean; ptt: boolean; pressed: boolean; speaking: boolean; sharing: boolean; shareAudioAvailable: boolean; shareAudioMuted: boolean; shareBusy: boolean; error: string; playbackBlocked: boolean; remotes: RemoteMedia[] }
export const initialVoice = (): VoiceView => ({ phase: 'idle', channelId: null, microphone: false, microphoneNotice: '', muted: false, deafened: false, suppressed: false, ptt: false, pressed: false, speaking: false, sharing: false, shareAudioAvailable: false, shareAudioMuted: false, shareBusy: false, error: '', playbackBlocked: false, remotes: [] });
export function roomUploadMbps(users: number) { return users * (users - 1) * 40 / 1000; }
export function secureVoiceAvailable() { return !!globalThis.isSecureContext && !!navigator.mediaDevices?.getUserMedia && typeof RTCPeerConnection !== 'undefined'; }
export const sharePresets = { '720p': { width: 1280, height: 720, bitrate: 2500000 }, '1080p': { width: 1920, height: 1080, bitrate: 4500000 } };

export const shareAudioPresets = { standard: 64000, high: 128000, music: 192000 };

export interface ScreenCaptureOptions extends DisplayMediaStreamOptions { systemAudio?: 'include' | 'exclude'; windowAudio?: 'window' | 'system' | 'exclude' }
export function captureOptions(preset: keyof typeof sharePresets, includeAudio: boolean): ScreenCaptureOptions {
  const quality = sharePresets[preset];
  return { video: { width: { ideal: quality.width, max: quality.width }, height: { ideal: quality.height, max: quality.height }, frameRate: { ideal: 30, max: 30 } }, audio: includeAudio ? { channelCount: { ideal: 2 }, echoCancellation: false, noiseSuppression: false, autoGainControl: false } : false, systemAudio: includeAudio ? 'include' : 'exclude', windowAudio: includeAudio ? 'window' : 'exclude' };
}

interface VoiceDependencies {
  peer?: () => RTCPeerConnection;
  microphone?: () => Promise<MediaStream>;
  display?: (constraints: DisplayMediaStreamOptions) => Promise<MediaStream>;
  stream?: () => MediaStream;
}
export class VoiceClient {
  readonly state = writable<VoiceView>(initialVoice());
  view = initialVoice();
  private pc?: RTCPeerConnection;
  private mic?: MediaStream;
  private screen?: MediaStream;
  private videoSender?: RTCRtpSender;
  private screenAudioSender?: RTCRtpSender;
  private context?: AudioContext;
  private meter?: ReturnType<typeof setInterval>;
  private connectTimer?: ReturnType<typeof setTimeout>;
  private disconnectTimer?: ReturnType<typeof setTimeout>;
  private generation = 0;
  private queue = Promise.resolve();
  private answerWaiter?: { resolve: () => void; reject: (error: Error) => void; timer: ReturnType<typeof setTimeout> };
  private canSpeak = true;
  private canStream = false;
  private lastSentMute = true;
  private joinedSignal = false;
  private level = 0;
  constructor(private signal: (type: string, data: unknown) => boolean, private dependencies: VoiceDependencies = {}) {}
  private update(patch: Partial<VoiceView>) { this.view = { ...this.view, ...patch }; this.state.set(this.view); }
  private send(type: string, data: unknown) { if (!this.signal(type, data)) throw new Error('Live connection lost. Reconnect before joining voice.'); }
  private serialize(action: () => Promise<void>) { const version = this.generation; const next = this.queue.then(async () => { if (version === this.generation) await action(); }); this.queue = next.catch(() => {}); return next; }
  private effectiveMute() { return !this.view.microphone || this.view.muted || this.view.deafened || this.view.suppressed || !this.canSpeak || (this.view.ptt && !this.view.pressed); }
  private applyMute() {
    const muted = this.effectiveMute(); this.mic?.getAudioTracks().forEach(track => track.enabled = !muted);
    if (this.joinedSignal && this.view.channelId && this.pc && muted !== this.lastSentMute) { this.lastSentMute = muted; this.signal('voice_state', { self_mute: muted, self_deaf: this.view.deafened }); }
    this.applyShareAudio();
    if (muted) this.speaking(false);
  }
  private applyShareAudio() {
    const enabled = !this.view.shareAudioMuted && !this.view.deafened && !this.view.suppressed && this.canStream;
    this.screen?.getAudioTracks().forEach(track => track.enabled = enabled);
  }
  toggleShareAudio() { this.update({ shareAudioMuted: !this.view.shareAudioMuted }); this.applyShareAudio(); }
  setCapabilities(speak: boolean, stream: boolean, suppressed = false) {
    this.canSpeak = speak; this.canStream = stream; this.update({ suppressed }); this.applyMute();
    if ((!stream || suppressed) && (this.screen || this.view.shareBusy)) void this.stopShare();
  }
  async join(channelId: string, options: { speak: boolean; stream: boolean; suppressed?: boolean; muted?: boolean } = { speak: true, stream: false }) {
    this.leave(); const version = this.generation;
    this.canSpeak = options.speak; this.canStream = options.stream;
    this.update({ phase: 'joining', channelId, error: '', suppressed: !!options.suppressed, muted: options.muted ?? this.view.muted });
    try {
      const pc = this.dependencies.peer?.() ?? new RTCPeerConnection({ iceServers: [], bundlePolicy: 'max-bundle' }); this.pc = pc;
      pc.ontrack = event => {
        if (version !== this.generation || this.pc !== pc) return;
        const sessionId = event.streams[0]?.id;
        if (!sessionId) return;
        let remote = this.view.remotes.find(item => item.sessionId === sessionId);
        if (!remote) { remote = { sessionId, audio: this.dependencies.stream?.() ?? new MediaStream(), audioTracks: [], video: this.dependencies.stream?.() ?? new MediaStream() }; }
        const target = event.track.kind === 'video' ? remote.video : remote.audio;
        if (!target.getTracks().some(track => track.id === event.track.id)) target.addTrack(event.track);
        if (event.track.kind === 'audio' && !remote.audioTracks.some(item => item.id === event.track.id)) {
          const stream = this.dependencies.stream?.() ?? new MediaStream(); stream.addTrack(event.track);
          remote.audioTracks = [...remote.audioTracks, { id: event.track.id, stream }];
        }
        this.update({ remotes: [...this.view.remotes.filter(item => item.sessionId !== sessionId), remote] });
        event.track.onended = () => { target.removeTrack(event.track); remote!.audioTracks = remote!.audioTracks.filter(item => item.id !== event.track.id); if (version === this.generation) this.update({ remotes: [...this.view.remotes] }); };
      };
      pc.onconnectionstatechange = () => {
        if (this.pc !== pc) return;
        if (pc.connectionState === 'connected') { clearTimeout(this.connectTimer); clearTimeout(this.disconnectTimer); this.update({ phase: 'connected' }); }
        else if (pc.connectionState === 'failed') this.fail('Voice could not connect. Check that the host is reachable over UDP or TCP.');
        else if (pc.connectionState === 'disconnected') { clearTimeout(this.disconnectTimer); this.disconnectTimer = setTimeout(() => { if (this.pc === pc && pc.connectionState === 'disconnected') this.fail('Voice connection lost. Join again when the network is ready.'); }, 10000); }
      };
      if (options.speak && !options.suppressed) {
        let mic: MediaStream | undefined;
        try { mic = await (this.dependencies.microphone?.() ?? navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true }, video: false })); }
        catch {
          if (version !== this.generation) return;
          this.update({ muted: true, microphoneNotice: 'No microphone, you can listen only. Screen sharing can still include sound when your browser supports it. Your microphone may be missing, unavailable, or permission was refused.' });
        }
        if (version !== this.generation) { mic?.getTracks().forEach(track => track.stop()); return; }
        if (mic?.getAudioTracks().length) {
          this.mic = mic; this.update({ microphone: true }); mic.getAudioTracks().forEach(track => pc.addTrack(track, mic));
          this.startMeter(mic);
        } else {
          mic?.getTracks().forEach(track => track.stop());
          this.update({ muted: true, microphoneNotice: 'No microphone, you can listen only. Screen sharing can still include sound when your browser supports it. Your microphone may be missing, unavailable, or permission was refused.' });
          pc.addTransceiver('audio', { direction: 'recvonly' });
        }
      } else pc.addTransceiver('audio', { direction: 'recvonly' });
      this.applyMute(); this.lastSentMute = this.effectiveMute();
      await this.serialize(async () => { await pc.setLocalDescription(await pc.createOffer()); if (pc !== this.pc || version !== this.generation) return; this.send('voice_join', { channel_id: channelId, offer: { type: 'offer', sdp: pc.localDescription!.sdp }, self_mute: this.effectiveMute(), self_deaf: this.view.deafened }); this.joinedSignal = true; });
      if (version !== this.generation) return;
      this.connectTimer = setTimeout(() => { if (version === this.generation && this.view.phase === 'joining') this.fail('Voice timed out. The host’s voice connection may be blocked or unavailable.'); }, 20000);
    } catch (error) { if (version === this.generation) this.fail(error instanceof Error ? error.message : 'Could not open the microphone.'); }
  }
  handleSdp(description: RTCSessionDescriptionInit) {
    const pc = this.pc; if (!pc || !['offer', 'answer'].includes(description.type) || !description.sdp) return Promise.resolve();
    return this.serialize(async () => {
      if (pc !== this.pc) return;
      // The SFU is polite: it discards a colliding host offer, answers ours, and re-offers later.
      if (description.type === 'offer' && pc.signalingState === 'have-local-offer') return;
      await pc.setRemoteDescription(description);
      if (pc !== this.pc) return;
      if (description.type === 'offer') { await pc.setLocalDescription(await pc.createAnswer()); if (pc === this.pc) this.send('voice_sdp', { type: 'answer', sdp: pc.localDescription!.sdp }); }
      else {
        if (this.answerWaiter) { clearTimeout(this.answerWaiter.timer); this.answerWaiter.resolve(); this.answerWaiter = undefined; }
      }
    }).catch(error => { if (this.pc === pc) this.fail(error instanceof Error ? error.message : 'Voice negotiation failed.'); });
  }
  handleEnd(message: string) { this.leave(false); this.update({ error: message || 'Voice connection ended.' }); }
  syncStates(states: VoiceState[], selfId: string) {
    const self = states.find(state => state.user_id === selfId);
    if (self && self.channel_id === this.view.channelId) {
      this.update({ suppressed: self.suppressed }); this.applyMute();
      if (!self.streaming && this.view.sharing) void this.stopShare();
    }
    const sessions = new Set(states.filter(state => state.channel_id === this.view.channelId).map(state => state.session_id));
    const retained = this.view.remotes.filter(remote => sessions.has(remote.sessionId));
    if (retained.length !== this.view.remotes.length) this.update({ remotes: retained });
  }
  toggleMute() { this.update({ muted: !this.view.muted }); this.applyMute(); }
  toggleDeaf() { this.update({ deafened: !this.view.deafened }); if (this.joinedSignal) this.signal('voice_state', { self_deaf: this.view.deafened, self_mute: this.effectiveMute() }); this.applyMute(); }
  setPtt(enabled: boolean) { this.update({ ptt: enabled, pressed: false }); this.applyMute(); }
  press(pressed: boolean) { if (!this.view.ptt || this.view.pressed === pressed) return; this.update({ pressed }); this.applyMute(); }
  private speaking(speaking: boolean) { if (this.view.speaking === speaking) return; this.update({ speaking }); if (this.pc && this.joinedSignal) this.signal('voice_speaking', { speaking }); }
  private startMeter(stream: MediaStream) {
    try {
      if (typeof AudioContext === 'undefined') return;
      const context = new AudioContext(); this.context = context; const analyser = context.createAnalyser(); analyser.fftSize = 512; context.createMediaStreamSource(stream).connect(analyser); void context.resume().catch(() => {});
      const samples = new Float32Array(analyser.fftSize); let lastSound = 0;
      this.meter = setInterval(() => { analyser.getFloatTimeDomainData(samples); const rms = Math.sqrt(samples.reduce((sum, sample) => sum + sample * sample, 0) / samples.length); this.level = rms; if (rms > 0.025) lastSound = Date.now(); this.speaking(!this.effectiveMute() && Date.now() - lastSound < 250); }, 100);
    } catch { /* Voice still works without an activity meter. */ }
  }
  async startShare(preset: keyof typeof sharePresets, includeAudio = true, audioQuality: keyof typeof shareAudioPresets = 'high') {
    if (!this.pc || this.view.phase !== 'connected' || !this.canStream || this.view.suppressed || this.view.shareBusy || this.screen) return;
    const pc = this.pc, version = this.generation, quality = sharePresets[preset]; this.update({ shareBusy: true, error: '' });
    let stream: MediaStream | undefined;
    try {
      const constraints = captureOptions(preset, includeAudio);
      stream = await (this.dependencies.display?.(constraints) ?? navigator.mediaDevices.getDisplayMedia(constraints));
      if (version !== this.generation || this.view.suppressed || !this.canStream) { stream.getTracks().forEach(track => track.stop()); return; }
      this.screen = stream; const video = stream.getVideoTracks()[0]; if (!video) throw new Error('No screen was selected.');
      video.onended = () => { if (this.screen === stream) void this.stopShare(); };
      let renegotiate = false;
      if (this.videoSender) await this.videoSender.replaceTrack(video); else { this.videoSender = pc.addTransceiver(video, { direction: 'sendonly', streams: [stream] }).sender; renegotiate = true; }
      if (!includeAudio) stream.getAudioTracks().forEach(track => { track.stop(); stream!.removeTrack(track); });
      const audio = stream.getAudioTracks()[0];
      this.update({ shareAudioAvailable: !!audio, shareAudioMuted: false }); this.applyShareAudio();
      if (audio) { if (this.screenAudioSender) await this.screenAudioSender.replaceTrack(audio); else { this.screenAudioSender = pc.addTransceiver(audio, { direction: 'sendonly', streams: [stream] }).sender; renegotiate = true; } }
      if (renegotiate) {
        let answered!: Promise<void>;
        await this.serialize(async () => {
          if (pc.signalingState !== 'stable') throw new Error('Voice is negotiating. Try sharing again in a moment.');
          await pc.setLocalDescription(await pc.createOffer());
          if (pc !== this.pc || version !== this.generation) return;
          this.send('voice_sdp', { type: 'offer', sdp: pc.localDescription!.sdp });
          answered = new Promise<void>((resolve, reject) => { const timer = setTimeout(() => { this.answerWaiter = undefined; reject(new Error('Screen sharing negotiation timed out.')); }, 15000); this.answerWaiter = { resolve, reject, timer }; });
          void answered.catch(() => {});
        });
        await answered;
      }
      if (version !== this.generation || this.screen !== stream) return;
      const params = this.videoSender!.getParameters(); if (params.encodings?.length) { params.encodings.forEach(encoding => { encoding.maxBitrate = quality.bitrate; encoding.maxFramerate = 30; }); await this.videoSender!.setParameters(params); }
      if (audio && this.screenAudioSender) { const params = this.screenAudioSender.getParameters(); if (params.encodings?.length) { params.encodings.forEach(encoding => encoding.maxBitrate = shareAudioPresets[audioQuality]); await this.screenAudioSender.setParameters(params); } }
      const screenMids = pc.getTransceivers().filter(item => item.sender === this.videoSender || item.sender === this.screenAudioSender).map(item => item.mid).filter((mid): mid is string => mid !== null);
      if (screenMids.length === 0 || (audio && !pc.getTransceivers().some(item => item.sender === this.screenAudioSender && item.mid !== null))) throw new Error('Screen audio could not be identified. Stop sharing and try again.');
      this.send('voice_state', { streaming: true, screen_mids: screenMids }); this.update({ sharing: true });
    } catch (error) { if (version === this.generation) { await this.stopShare(); this.update({ error: error instanceof Error ? error.message : 'Screen sharing was cancelled.' }); } else stream?.getTracks().forEach(track => track.stop()); }
    finally { if (version === this.generation) this.update({ shareBusy: false }); }
  }
  async stopShare() {
    const stream = this.screen; this.screen = undefined; this.update({ sharing: false, shareAudioAvailable: false });
    this.signal('voice_state', { streaming: false }); stream?.getTracks().forEach(track => track.stop());
    await Promise.allSettled([this.videoSender?.replaceTrack(null), this.screenAudioSender?.replaceTrack(null)]);
  }
  playbackBlocked() { this.update({ playbackBlocked: true }); }
  playbackResumed() { this.update({ playbackBlocked: false }); }
  dismissError() { this.update({ error: '' }); }
  private fail(message: string) { this.leave(); this.update({ error: message }); }
  leave(notify = true) {
    if (notify && this.view.channelId) this.signal('voice_leave', {});
    this.generation++; clearTimeout(this.connectTimer); clearTimeout(this.disconnectTimer); clearInterval(this.meter);
    this.joinedSignal = false;
    const pc = this.pc; this.pc = undefined; pc?.close();
    this.mic?.getTracks().forEach(track => track.stop()); this.mic = undefined;
    const screen = this.screen; this.screen = undefined; screen?.getTracks().forEach(track => track.stop());
    void this.context?.close().catch(() => {}); this.context = undefined;
    if (this.answerWaiter) { clearTimeout(this.answerWaiter.timer); this.answerWaiter.reject(new Error('Voice connection ended.')); this.answerWaiter = undefined; }
    this.videoSender = undefined; this.screenAudioSender = undefined; this.queue = Promise.resolve();
    this.update({ ...initialVoice(), muted: this.view.muted, deafened: this.view.deafened, ptt: this.view.ptt });
  }
  peerForDiagnostics() { return this.pc; }
  activityForDiagnostics() { return { level: this.level, context: this.context?.state, speaking: this.view.speaking, tracks: this.mic?.getAudioTracks().map(track => ({ enabled: track.enabled, state: track.readyState })) }; }
}
