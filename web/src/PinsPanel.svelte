<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { can, type Api, type Channel, type Message, type PinnedMessage, type Snapshot } from './lib/api';
  import MessageText from './MessageText.svelte';
  import Avatar from './Avatar.svelte';
  export let api: Api;
  export let channel: Channel;
  export let snapshot: Snapshot;
  export let onJump: (message: Message) => void;
  export let onLink: (url: string) => void;
  export let onError: (error: unknown) => void = () => {};
  let pins: PinnedMessage[] = [], loading = true, error = '', busy = '';
  const abort = new AbortController();
  $: manage = channel.type === 'dm' || can(snapshot.permissions, 2, snapshot.is_owner);
  export async function refresh() {
    try { pins = await api.request<PinnedMessage[]>(`/channels/${channel.id}/pins`, 'GET', undefined, abort.signal); }
    catch (cause) { if (!abort.signal.aborted) { error = cause instanceof Error ? cause.message : 'Could not load pins.'; onError(cause); } }
    finally { loading = false; }
  }
  async function unpin(id: string) {
    if (busy) return; busy = id; error = '';
    try { await api.request(`/channels/${channel.id}/pins/${id}`, 'DELETE'); pins = pins.filter(pin => pin.message.id !== id); }
    catch (cause) { error = cause instanceof Error ? cause.message : 'Could not remove the pin.'; onError(cause); }
    finally { busy = ''; }
  }
  onMount(() => { void refresh(); });
  onDestroy(() => abort.abort());
</script>

<h2 id="dialog-title">Pinned messages</h2><p class="muted small">{channel.type === 'dm' ? channel.name : `#${channel.name}`}</p>
{#if loading}<p class="muted" role="status">Loading pins…</p>{/if}
{#if error}<div class="error" role="alert">{error}</div>{/if}
{#if !loading && !pins.length}<p class="muted">No pinned messages yet.</p>{/if}
{#each pins as pin (pin.message.id)}
  <article class="pin-item">
    <div class="pin-author"><Avatar user={snapshot.members.find(member => member.user.id === pin.message.author.id)?.user ?? pin.message.author} origin={api.origin} size={28}/><strong>{pin.message.author.display_name}</strong><span class="small muted">{new Date(pin.message.created_at).toLocaleString()}</span></div>
    <div class="pin-content"><MessageText content={pin.message.content} {onLink} members={snapshot.members} roles={snapshot.roles} channels={snapshot.channels} emojis={snapshot.emojis ?? []} origin={api.origin}/></div>
    {#if pin.message.attachments.length}<p class="small muted">{pin.message.attachments.length} attachment(s) · Open the message to view</p>{/if}
    <div class="button-row"><button class="secondary" onclick={() => onJump(pin.message)}>Jump to message</button>{#if manage}<button class="secondary" disabled={!!busy} onclick={() => unpin(pin.message.id)}>{busy === pin.message.id ? 'Removing…' : 'Unpin'}</button>{/if}</div>
  </article>
{/each}
<style>.pin-item{padding:16px 0;border-top:1px solid var(--c-3a4a53,#3a4a53)}.pin-author{display:flex;gap:10px;align-items:center;flex-wrap:wrap}.pin-content{white-space:pre-wrap;overflow-wrap:anywhere;margin:12px 0;max-height:220px;overflow:auto}</style>
