<script lang="ts">
  import { can, type Api, type Channel, type Snapshot } from './lib/api';
  export let api: Api;
  export let parent: Channel;
  export let snapshot: Snapshot;
  export let onOpen: (channel: Channel) => void;
  export let onError: (error: unknown) => void = () => {};
  let showArchived = false, busy = '', error = '';
  $: threads = snapshot.channels.filter(channel => channel.parent_id === parent.id && (showArchived || !channel.archived)).sort((a, b) => a.name.localeCompare(b.name));
  $: manage = can(snapshot.permissions, 3, snapshot.is_owner);
  async function archive(thread: Channel) {
    if (busy) return; busy = thread.id; error = '';
    try { const updated = await api.request<Channel>(`/channels/${thread.id}`, 'PATCH', { archived: !thread.archived }); snapshot = { ...snapshot, channels: snapshot.channels.map(channel => channel.id === updated.id ? updated : channel) }; }
    catch (cause) { error = cause instanceof Error ? cause.message : 'Could not update the thread.'; onError(cause); }
    finally { busy = ''; }
  }
</script>
<h2 id="dialog-title">Threads in #{parent.name}</h2>
<p class="muted small">Use “Start thread” on a message to begin a separate conversation.</p>
<label class="check-label"><input type="checkbox" bind:checked={showArchived}/>Include archived threads</label>
{#if error}<div class="error" role="alert">{error}</div>{/if}
{#if !threads.length}<p class="muted">No {showArchived ? '' : 'active '}threads here yet.</p>{/if}
{#each threads as thread (thread.id)}<div class="thread-row"><button class="thread-open" onclick={() => onOpen(thread)}><strong>{thread.name}</strong><span class="small muted">{thread.archived ? 'Archived · Read only' : 'Open thread'}</span></button>{#if manage}<button class="secondary" disabled={!!busy} onclick={() => archive(thread)}>{thread.archived ? 'Reopen' : 'Archive'}</button>{/if}</div>{/each}
<style>.thread-row{display:flex;align-items:center;gap:10px;border-top:1px solid var(--c-3a4a53,#3a4a53);padding:12px 0}.thread-open{flex:1;text-align:left;display:flex;flex-direction:column;gap:5px;padding:8px}</style>
