<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Api } from './lib/api';
  import type { AiFlag, Snapshot, Member } from './lib/api';
  import { mergeFlags } from './lib/ai';
  export let api: Api;
  export let snapshot: Snapshot;
  export let latest: AiFlag | null = null;
  export let onUpdate: (flag: AiFlag) => void;
  export let onMember: (member: Member) => void;
  let filter = 'open', rows: AiFlag[] = [], loading = true, busy = false, error = '', more = false;
  let cursor = '', generation = 0, alive = true;
  let updates = new Map<string, AiFlag>();
  let confirm: AiFlag | null = null;
  $: if (latest) { updates.set(latest.id, latest); rows = mergeFlags(rows, [latest], filter); }
  async function load(older = false) {
    const version = ++generation; loading = true; error = ''; confirm = null;
    if (!older) { rows = []; cursor = ''; updates.clear(); }
    try {
      const page = await api.request<AiFlag[]>(`/ai/flags?status=${filter}&limit=50${older && cursor ? `&before=${encodeURIComponent(cursor)}` : ''}`);
      if (!alive || version !== generation) return;
      cursor = page.at(-1)?.id ?? cursor; more = page.length === 50;
      rows = mergeFlags(older ? rows : [], [...page, ...updates.values()], filter);
    } catch (cause) { if (alive && version === generation) error = cause instanceof Error ? cause.message : 'Could not load AI flags.'; }
    finally { if (alive && version === generation) loading = false; }
  }
  async function resolve(flag: AiFlag, action: 'dismiss' | 'delete_message' | 'resolved') {
    if (busy) return; busy = true; error = '';
    try { const value = await api.request<AiFlag>(`/ai/flags/${encodeURIComponent(flag.id)}/resolve`, 'POST', { action }); if (alive) { updates.set(value.id, value); rows = mergeFlags(rows, [value], filter); confirm = null; onUpdate(value); } }
    catch (cause) { if (alive) error = cause instanceof Error ? cause.message : 'Could not resolve flag.'; }
    finally { if (alive) busy = false; }
  }
  onMount(() => { void load(); }); onDestroy(() => { alive = false; generation++; rows = []; updates.clear(); });
</script>
<p class="small muted">AI can make mistakes. Review the checked text and reason before taking action. Hidden messages retain a copy here.</p>
<div class="button-row"><label>Flag status<select bind:value={filter} disabled={busy} onchange={() => load()}><option value="open">Open</option><option value="dismissed">Dismissed</option><option value="deleted">Deleted</option><option value="resolved">Resolved</option><option value="all">All</option></select></label><button class="secondary" disabled={busy || loading} onclick={() => load()}>Refresh queue</button></div>
{#if error}<div class="error" role="alert">{error}</div>{/if}
{#if confirm}<div class="confirm-box" role="alert"><h3>Delete flagged message?</h3><p>This permanently deletes the message from the channel. The checked copy remains in the review queue.</p><div class="button-row"><button class="secondary" disabled={busy} onclick={() => confirm = null}>Cancel</button><button class="primary" disabled={busy} onclick={() => resolve(confirm!, 'delete_message')}>Confirm delete message</button></div></div>{/if}
{#each rows as flag (flag.id)}{@const member = snapshot.members.find(item => item.user.id === flag.author_id)}
  <article class="ai-flag"><div class="settings-row"><div><strong>{flag.author?.display_name ?? 'Deleted account'}</strong><p class="small muted">#{snapshot.channels.find(channel => channel.id === flag.channel_id)?.name ?? 'Deleted or inaccessible channel'} · {new Date(flag.created_at).toLocaleString()}</p></div><span class="ai-flag-status">{flag.status} · {flag.action.replaceAll('_', ' ')}</span></div>
  <blockquote>{flag.content}</blockquote><p><strong>{flag.categories.join(', ') || 'Uncategorized'}</strong> · {flag.provider}</p><p>{flag.reason}</p>
  {#if flag.scores}<details><summary>Classifier scores</summary><dl class="ai-scores">{#each Object.entries(flag.scores) as [name, score]}<div><dt>{name}</dt><dd>{score.toFixed(3)}</dd></div>{/each}</dl></details>{/if}
  {#if flag.status === 'open'}<div class="button-row"><button class="secondary" disabled={busy} onclick={() => resolve(flag, 'dismiss')}>Dismiss flag</button>{#if flag.message_id}<button class="secondary danger" disabled={busy} onclick={() => confirm = flag}>Delete message</button>{/if}<button class="secondary" disabled={busy} onclick={() => resolve(flag, 'resolved')}>Mark handled</button>{#if member}<button class="secondary" disabled={busy} onclick={() => onMember(member)}>Member actions</button>{/if}</div>{:else}<p class="small muted">{flag.resolved_at ? `Reviewed ${new Date(flag.resolved_at).toLocaleString()}` : 'Reviewed'}{flag.resolved_by ? ` by ${snapshot.members.find(item => item.user.id === flag.resolved_by)?.user.display_name ?? 'a moderator'}` : ''}</p>{/if}</article>
{:else}{#if !loading && !error}<p class="loading">No {filter === 'all' ? '' : filter + ' '}AI flags.</p>{/if}{/each}
{#if loading}<p role="status">Loading review queue…</p>{:else if more}<button class="secondary" disabled={busy} onclick={() => load(true)}>Load older flags</button>{/if}
