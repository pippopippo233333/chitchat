<script lang="ts">
  import { onDestroy } from 'svelte';
  import Icon from './Icon.svelte';
  import { startRingtone, type Ring } from './lib/calls';
  export let rings: Ring[];
  export let sound = true;
  export let volume = 50;
  export let canAnswer = true;
  export let onAccept: (ring: Ring) => void;
  export let onDecline: (ring: Ring) => void;
  let stop: (() => void) | null = null;
  $: ringing = rings.length > 0 && sound;
  $: if (ringing && !stop) stop = startRingtone(volume); else if (!ringing && stop) { stop(); stop = null; }
  onDestroy(() => stop?.());
</script>

{#if rings.length}<div class="incoming-calls" role="alertdialog" aria-label="Incoming calls">
  {#each rings as ring (ring.channel_id)}<div class="incoming-call">
    <div class="incoming-call-icon"><Icon name="phone" size={20}/></div>
    <div class="incoming-call-text"><strong>{ring.from_name}</strong><span>{ring.invited ? 'invited you to a call' : 'is calling you'}</span></div>
    <button class="call-decline" aria-label={`Decline call from ${ring.from_name}`} title="Decline" onclick={() => onDecline(ring)}><Icon name="close" size={18}/></button>
    <button class="call-accept" aria-label={`Accept call from ${ring.from_name}`} title={canAnswer ? 'Accept' : 'Calls need a secure (https://) connection'} disabled={!canAnswer} onclick={() => onAccept(ring)}><Icon name="phone" size={18}/></button>
  </div>{/each}
</div>{/if}

<style>
  .incoming-calls{position:fixed;top:16px;right:16px;z-index:40;display:flex;flex-direction:column;gap:10px;max-width:calc(100vw - 32px)}
  .incoming-call{display:flex;align-items:center;gap:12px;background:#1b2b33;border:1px solid #4f7766;border-radius:14px;padding:12px 14px;box-shadow:0 20px 60px #0008;min-width:280px;animation:call-pulse 1.6s ease-in-out infinite}
  .incoming-call-icon{width:38px;height:38px;border-radius:50%;background:#2b463c;color:var(--accent);display:flex;align-items:center;justify-content:center}
  .incoming-call-text{flex:1;min-width:0;display:flex;flex-direction:column;font-size:13px}
  .incoming-call-text strong{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .incoming-call-text span{color:var(--muted);font-size:12px}
  .call-accept,.call-decline{width:40px;height:40px;border-radius:50%;display:flex;align-items:center;justify-content:center}
  .call-accept{background:#3fae74;color:#0f2419}.call-accept:hover{background:#52c487}
  .call-decline{background:#b84a4a;color:#fff}.call-decline:hover{background:#cc5c5c}
  @keyframes call-pulse{50%{border-color:#a4e4c2}}
  @media (prefers-reduced-motion:reduce){.incoming-call{animation:none}}
</style>
