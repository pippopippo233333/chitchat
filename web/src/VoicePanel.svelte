<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { can } from './lib/api';
  import type { Channel, Snapshot, VoiceState } from './lib/api';
  import { VoiceClient, secureVoiceAvailable, roomUploadMbps } from './lib/voice';
  import Icon from './Icon.svelte';
  import { panelHeight, readPreference, writePreference } from './lib/panels';
  import ScreenViewer from './ScreenViewer.svelte';
  export let panelScope = 'local';
  export let client: VoiceClient;
  export let snapshot: Snapshot;
  export let selected: Channel | undefined;
  export let states: VoiceState[];
  export let speaking: Record<string, boolean>;
  export let connected: boolean;
  export let onJoin: (channel: Channel) => void;
  let collapsed = false, size: number | null = null, measuredHeight = 360;
  let panel: HTMLElement;
  let drag: { id: number; y: number; height: number } | null = null;
  $: preferenceKey = `chitchat.voice.panel:${panelScope}`;
  $: collapsed = readPreference(preferenceKey + ':collapsed') === '1';
  $: savedSize = readPreference(preferenceKey + ':height');
  $: size = savedSize ? panelHeight(Number(savedSize), window.innerHeight) : null;
  function collapse(value: boolean) { collapsed = value; writePreference(preferenceKey + ':collapsed',value ? '1' : '0'); }
  function measurePanel(node: HTMLElement) { const observer = new ResizeObserver(() => { measuredHeight = node.getBoundingClientRect().height; }); observer.observe(node); return { destroy() { observer.disconnect(); } }; }
  function saveSize() { if (size !== null) writePreference(preferenceKey + ':height',String(size)); }
  function startResize(event: PointerEvent) { if (event.button !== 0) return; event.preventDefault(); drag = { id: event.pointerId, y: event.clientY, height: panel.getBoundingClientRect().height }; (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId); }
  function moveResize(event: PointerEvent) { if (drag?.id === event.pointerId) size = panelHeight(drag.height + event.clientY - drag.y,window.innerHeight); }
  function endResize(event: PointerEvent) { if (drag?.id !== event.pointerId) return; drag = null; saveSize(); }
  function keyResize(event: KeyboardEvent) {
    const current = size ?? panel?.getBoundingClientRect().height ?? 360;
    const value = event.key === 'ArrowUp' ? current + (event.shiftKey ? 50 : 20) : event.key === 'ArrowDown' ? current - (event.shiftKey ? 50 : 20) : event.key === 'Home' ? 180 : event.key === 'End' ? window.innerHeight - 150 : null;
    if (value === null) return; event.preventDefault(); size = panelHeight(value,window.innerHeight); saveSize();
  }
  function viewportResize() { if (size !== null) size = panelHeight(size,window.innerHeight); }
  const state = client.state;
  const secure = secureVoiceAvailable();
  const screenCaptureAvailable = typeof navigator.mediaDevices?.getDisplayMedia === 'function';
  let includeShareAudio = true;
  let audioQuality: 'standard' | 'high' | 'music' = 'high';
  let preset: '720p' | '1080p' = '720p';
  let mediaElements = new Set<HTMLMediaElement>();
  $: room = selected ?? snapshot.channels.find(channel => channel.id === $state.channelId);
  $: currentRoom = snapshot.channels.find(channel => channel.id === $state.channelId);
  $: occupants = states.filter(item => item.channel_id === room?.id);
  $: joined = $state.channelId === room?.id && !!room;
  $: mayConnect = !!room && can(room.my_permissions, 0, snapshot.is_owner) && can(room.my_permissions, 12, snapshot.is_owner);
  $: maySpeak = !!room && can(room.my_permissions, 13, snapshot.is_owner);
  $: mayStream = !!room && can(room.my_permissions, 14, snapshot.is_owner) && !$state.suppressed;
  $: full = !joined && occupants.length >= (room?.user_limit ?? 10);
  function name(id: string) { return snapshot.members.find(member => member.user.id === id)?.user.display_name ?? 'Member'; }
  function isSpeaking(id: string) { return id === snapshot.user.id ? $state.speaking : !!speaking[id]; }
  function bindMedia(node: HTMLMediaElement, options: { stream: MediaStream; muted: boolean }) {
    mediaElements.add(node);
    const update = (next: typeof options) => { if (node.srcObject !== next.stream) node.srcObject = next.stream; node.muted = next.muted; void node.play().catch(() => client.playbackBlocked()); };
    update(options); return { update, destroy() { node.pause(); node.srcObject = null; mediaElements.delete(node); } };
  }
  async function resumePlayback() { const results = await Promise.allSettled([...mediaElements].map(element => element.play())); if (results.every(result => result.status === 'fulfilled')) client.playbackResumed(); }
  function keyDown(event: KeyboardEvent) {
    if (event.code !== 'Space' || !$state.microphone || !$state.ptt || !$state.channelId || event.repeat || (event.target instanceof Element && event.target.closest('input, textarea, select, button, a, [contenteditable="true"]'))) return;
    event.preventDefault(); client.press(true);
  }
  function keyUp(event: KeyboardEvent) { if (event.code === 'Space') client.press(false); }
  function release() { client.press(false); }
  onMount(() => { window.addEventListener('keydown', keyDown); window.addEventListener('keyup', keyUp); window.addEventListener('blur', release); window.addEventListener('pointerup', release); window.addEventListener('pointercancel', release); });
  onDestroy(() => { release(); window.removeEventListener('keydown', keyDown); window.removeEventListener('keyup', keyUp); window.removeEventListener('blur', release); window.removeEventListener('pointerup', release); window.removeEventListener('pointercancel', release); });
</script>
<svelte:window onresize={viewportResize}/>

{#each $state.remotes as remote (remote.sessionId)}{#each remote.audioTracks as audio (audio.id)}<audio autoplay use:bindMedia={{ stream: audio.stream, muted: $state.deafened }}></audio>{/each}{/each}
{#if $state.error}<div class="banner error" role="alert"><span>{$state.error}</span><button class="icon-button" aria-label="Dismiss voice error" onclick={() => client.dismissError()}><Icon name="close" size={16}/></button></div>{/if}
{#if $state.playbackBlocked}<div class="banner notice"><span>Your browser paused voice playback.</span><button class="secondary" onclick={resumePlayback}>Enable audio</button></div>{/if}
{#if selected || $state.channelId}
  <section class="voice-panel" bind:this={panel} use:measurePanel class:voice-room={!!selected} class:voice-collapsed={collapsed} class:custom-height={size !== null && !collapsed} style:height={size !== null && !collapsed ? `${size}px` : undefined} aria-label="Voice controls">
    {#if collapsed}<div class="voice-compact-bar"><strong>{$state.channelId ? 'Voice · ' : 'Room · '}{currentRoom?.name ?? room?.name ?? 'Voice'}</strong>{#if $state.channelId}<button class="secondary" disabled={!$state.microphone || $state.suppressed} aria-pressed={$state.muted} onclick={() => client.toggleMute()}>{$state.muted ? 'Unmute mic' : 'Mute mic'}</button><button class="secondary" aria-pressed={$state.deafened} onclick={() => client.toggleDeaf()}>{$state.deafened ? 'Undeafen' : 'Deafen'}</button><button class="secondary danger" onclick={() => client.leave()}>Leave voice</button>{/if}<button class="secondary" aria-label="Expand voice panel" aria-expanded="false" onclick={() => collapse(false)}>Expand</button></div>
    {:else}
    <div class="voice-panel-content">
    <div class="voice-room-heading"><div><span class="eyebrow">{joined ? $state.phase === 'connected' ? 'Voice connected' : 'Connecting voice…' : 'Voice room'}</span><h2>{room?.name ?? 'Voice'}</h2><p>{occupants.length} / {room?.user_limit ?? 10} people</p></div><div class="button-row">{#if $state.channelId}<button class="secondary danger" onclick={() => client.leave()}>Leave voice</button>{/if}<button class="icon-button" aria-label="Collapse voice panel" title="Collapse voice panel; stay connected" aria-expanded="true" onclick={() => collapse(true)}><Icon name="close" size={18}/></button></div></div>
    {#if selected && currentRoom && !joined}<p class="notice">You’re connected to {currentRoom.name}. Joining this room will move your voice connection.</p>{/if}
    {#if !secure}<p class="notice">Voice needs a secure (https://) connection to this host. You can still see who is in the room.</p>
    {:else if !joined && room}<button class="primary" disabled={!mayConnect || !connected || full} onclick={() => onJoin(room!)}>{full ? 'Room is full' : mayConnect ? 'Join voice' : 'Voice access required'}</button>{/if}
    {#if selected}<div class="voice-people">{#each occupants as occupant}<div class="voice-person" class:is-speaking={isSpeaking(occupant.user_id)}><div class="avatar">{name(occupant.user_id).slice(0, 2).toUpperCase()}</div><strong>{name(occupant.user_id)}</strong><span>{occupant.suppressed ? 'Listen only' : occupant.self_deaf ? 'Deafened' : occupant.self_mute ? 'Muted' : isSpeaking(occupant.user_id) ? 'Speaking' : 'Listening'}{occupant.streaming ? ' · Sharing' : ''}</span></div>{:else}<p class="muted small">No one is here yet.</p>{/each}</div>{/if}
    {#if $state.channelId && (joined || !selected)}
      {#if $state.microphoneNotice}<p class="notice" role="status">{$state.microphoneNotice}</p>{/if}
      {#if $state.suppressed}<p class="notice">You’re listening only while your voice is suppressed or timed out.</p>{/if}
      <div class="voice-buttons"><button class="secondary" disabled={!$state.microphone || !maySpeak || $state.suppressed} aria-pressed={$state.muted} title="Mutes only your microphone" onclick={() => client.toggleMute()}>{$state.muted ? 'Unmute microphone' : 'Mute microphone'}</button><button class="secondary" aria-pressed={$state.deafened} onclick={() => client.toggleDeaf()}>{$state.deafened ? 'Undeafen' : 'Deafen'}</button>{#if !$state.microphone && maySpeak && !$state.suppressed && room}<button class="secondary" onclick={() => onJoin(room!)}>Rejoin with microphone</button>{/if}<label>Microphone mode<select disabled={!$state.microphone || !maySpeak || $state.suppressed} value={$state.ptt ? 'ptt' : 'activity'} onchange={event => client.setPtt(event.currentTarget.value === 'ptt')}><option value="activity">Voice activity</option><option value="ptt">Push to talk</option></select></label>{#if $state.ptt}<button class="secondary talk-button" class:held={$state.pressed} disabled={!$state.microphone || $state.muted || $state.deafened || $state.suppressed} onpointerdown={event => { event.preventDefault(); client.press(true); }} onpointerleave={release} onkeydown={event => { if (event.code === 'Space' || event.code === 'Enter') { event.preventDefault(); client.press(true); } }} onkeyup={release} onblur={release}>Hold to talk</button><span class="small muted">Or hold Space outside text fields.</span>{/if}</div>
      <div class="voice-buttons screen-share-controls"><label>Share quality<select bind:value={preset} disabled={$state.sharing || $state.shareBusy}><option value="720p">720p · 30 fps</option><option value="1080p">1080p · 30 fps</option></select></label><label>Shared sound quality<select bind:value={audioQuality} disabled={$state.sharing || $state.shareBusy || !includeShareAudio}><option value="standard">Standard · up to 64 kbit/s</option><option value="high">High · up to 128 kbit/s</option><option value="music">Music · up to 192 kbit/s</option></select></label><label class="check-label"><input type="checkbox" bind:checked={includeShareAudio} disabled={$state.sharing || $state.shareBusy}/>Include shared sound</label>{#if $state.sharing}<button class="secondary danger" onclick={() => client.stopShare()}>Stop sharing</button>{:else}<button class="secondary" disabled={!mayStream || $state.phase !== 'connected' || $state.shareBusy || !screenCaptureAvailable} aria-describedby={!screenCaptureAvailable ? 'screen-share-unavailable' : undefined} onclick={() => client.startShare(preset, includeShareAudio, audioQuality)}>{$state.shareBusy ? 'Starting share…' : 'Share screen'}</button>{/if}{#if !mayStream}<span class="small muted">Screen sharing requires Share screen permission and no timeout.</span>{/if}</div>
      {#if !screenCaptureAvailable}<p class="notice" id="screen-share-unavailable">Screen sharing is unavailable because this browser does not offer screen capture, as is common on mobile browsers. Use a desktop browser with screen-sharing support to share. You can still watch other people’s shared screens here.</p>{/if}
      {#if $state.sharing}{#if $state.shareAudioAvailable}<div class="voice-buttons"><button class="secondary" aria-pressed={$state.shareAudioMuted} onclick={() => client.toggleShareAudio()}>{$state.shareAudioMuted ? 'Unmute shared sound' : 'Mute shared sound'}</button><span class="small muted">Shared sound is independent of microphone mute and push-to-talk. Deafen mutes both.</span></div>{:else}<p class="notice">Your browser did not include sound. For Roblox or another desktop app in Edge/Chrome, stop and try Entire screen with Also share system audio enabled. This includes other PC sounds. A single window may not offer audio in your browser.</p>{/if}{:else}<p class="small muted">In the browser picker, enable audio sharing if offered. Sound capture depends on the browser and selected tab, window or screen. High/Music requests stereo without microphone noise processing; actual quality depends on the source and browser.</p>{/if}
      {#if selected}<p class="small muted voice-network-note">Voice uses UDP, with TCP fallback on networks that block it. At capacity this room may need about {roomUploadMbps(room?.user_limit ?? 10).toFixed(1)} Mbps of host upload if everyone talks. Microphone mute does not mute shared sound.</p>{/if}
      <div class="shared-screens">{#each $state.remotes as remote (remote.sessionId)}{@const owner = states.find(item => item.session_id === remote.sessionId && item.streaming)}{#if owner && remote.video.getVideoTracks().length}<ScreenViewer stream={remote.video} name={name(owner.user_id)} resumeAudio={resumePlayback} deafened={$state.deafened} playbackBlocked={$state.playbackBlocked}/>{/if}{/each}</div>
    {/if}
    </div>
    {/if}
    {#if !collapsed}<div class="voice-resize-handle" role="slider" tabindex="0" aria-label="Resize voice panel" aria-orientation="vertical" aria-valuemin="180" aria-valuemax={Math.max(180,window.innerHeight - 150)} aria-valuenow={panelHeight(size ?? measuredHeight,window.innerHeight)} title="Drag to resize, or use arrow keys" onpointerdown={startResize} onpointermove={moveResize} onpointerup={endResize} onpointercancel={endResize} onkeydown={keyResize}><span></span></div>{/if}
  </section>
{/if}

