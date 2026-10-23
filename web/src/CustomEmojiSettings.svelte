<script lang="ts">
  import { can, ApiError, type Api, type CustomEmoji, type Snapshot } from './lib/api';
  import { emojiImageUrl } from './lib/custom-emoji';
  export let api: Api;
  export let snapshot: Snapshot;
  export let onChanged: (emojis: CustomEmoji[]) => void;
  export let onError: (error: unknown) => void = () => {};
  let name = '', file: File | null = null, busy = false, error = '', confirmId = '';
  $: emojis = snapshot.emojis ?? [];
  $: editable = can(snapshot.permissions, 10, snapshot.is_owner);
  async function add(event: SubmitEvent) {
    event.preventDefault(); if (!file || busy || !editable) return;
    if (file.size > 256 * 1024) { error = 'Emoji images can be at most 256 KB.'; return; }
    busy = true; error = '';
    try {
      const response = await fetch(`${api.origin}${api.prefix}/emojis?${new URLSearchParams({name:name.trim()})}`, {method:'POST',credentials:'omit',headers:{Authorization:`Bearer ${api.token}`,'Content-Type':file.type||'application/octet-stream'},body:file});
      if (!response.ok) { const payload = await response.json().catch(() => null); throw new ApiError(response.status,payload?.error?.code??'upload_failed',payload?.error?.message??'Could not add emoji.'); }
      const emoji: CustomEmoji = await response.json();
      onChanged([...emojis.filter(item => item.id !== emoji.id),emoji]); name = ''; file = null;
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Could not add emoji.'; onError(cause); }
    finally { busy = false; }
  }
  async function remove(id: string) {
    if (busy) return; busy = true; error = '';
    try { await api.request(`/emojis/${id}`, 'DELETE'); onChanged(emojis.filter(item => item.id !== id)); confirmId = ''; }
    catch (cause) { error = cause instanceof Error ? cause.message : 'Could not remove emoji.'; onError(cause); }
    finally { busy = false; }
  }
</script>
<h2 id="dialog-title">Server emoji</h2><p class="muted small">Use these emoji in messages and reactions. {emojis.length}/100 added.</p>
{#if editable}<form onsubmit={add}><label>Name<input bind:value={name} pattern={'[a-zA-Z0-9_]{2,32}'} minlength="2" maxlength="32" placeholder="party_time" required disabled={busy}/></label><label>Emoji image<input type="file" accept="image/png,image/jpeg,image/gif,image/webp" disabled={busy} onchange={event => file = event.currentTarget.files?.[0] ?? null}/></label><p class="muted small">Up to 256 KB; saved as a still image up to 128 pixels.</p><button class="primary" disabled={busy || !file || !name.trim()}>{busy ? 'Saving…' : 'Add emoji'}</button></form>{/if}
{#if error}<div class="error" role="alert">{error}</div>{/if}
{#each emojis as emoji (emoji.id)}<div class="emoji-row">{#if emojiImageUrl(emoji.url,api.origin)}<img src={emojiImageUrl(emoji.url,api.origin)} alt={`:${emoji.name}:`}/>{/if}<span>:{emoji.name}:</span>{#if editable}<button class="secondary" disabled={busy} onclick={() => confirmId = emoji.id}>Remove</button>{/if}</div>{#if confirmId === emoji.id}<div class="confirm-box"><p>Remove :{emoji.name}:? Existing uses will show a text placeholder.</p><div class="button-row"><button class="secondary" disabled={busy} onclick={() => confirmId = ''}>Keep emoji</button><button class="primary" disabled={busy} onclick={() => remove(emoji.id)}>Remove emoji</button></div></div>{/if}{/each}
<style>.emoji-row{display:flex;gap:14px;align-items:center;padding:12px 0;border-top:1px solid var(--c-3a4a53,#3a4a53)}.emoji-row img{width:32px;height:32px;object-fit:contain}.emoji-row span{flex:1}</style>
