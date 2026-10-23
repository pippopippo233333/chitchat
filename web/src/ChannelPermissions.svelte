<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { can, type Api, type Channel, type ChannelRule, type ChannelRules, type Role, type Snapshot } from './lib/api';
  import { canManageRole } from './lib/permissions';
  export let api: Api;
  export let channel: Channel;
  export let snapshot: Snapshot;
  export let onError: (error: unknown) => void = () => {};
  let rules: ChannelRule[] = [], selectedRole = '', loading = true, busy = false, error = '', success = '';
  let states: Record<number, string> = {1:'inherit',7:'inherit',8:'inherit'};
  const options = [{bit:1, name:'Write messages'}, {bit:7, name:'Upload files'}, {bit:8, name:'Add reactions'}];
  const abort = new AbortController();
  $: roles = [...snapshot.roles].sort((a, b) => b.position - a.position);
  $: role = roles.find(item => item.id === selectedRole);
  $: editable = !!role && (canManageRole(snapshot, role) || role.is_default && can(snapshot.permissions, 4, snapshot.is_owner));
  function select(id: string) {
    selectedRole = id; success = '';
    const rule = rules.find(item => item.role_id === id);
    states = Object.fromEntries(options.map(({bit}) => [bit, BigInt(rule?.allow ?? '0') & 1n << BigInt(bit) ? 'allow' : BigInt(rule?.deny ?? '0') & 1n << BigInt(bit) ? 'deny' : 'inherit']));
  }
  onMount(() => { void (async () => {
    try { const result = await api.request<ChannelRules>(`/channels/${channel.id}/permissions`, 'GET', undefined, abort.signal); rules = result.overwrites; select(roles.find(role => role.is_default)?.id ?? roles[0]?.id ?? ''); }
    catch (cause) { if (!abort.signal.aborted) { error = cause instanceof Error ? cause.message : 'Could not load posting rules.'; onError(cause); } }
    finally { loading = false; }
  })(); });
  onDestroy(() => abort.abort());
  async function save(reset = false) {
    if (!editable || busy) return; busy = true; error = ''; success = '';
    try {
      let allow = 0n, deny = 0n;
      for (const {bit} of options) { if (states[bit] === 'allow') allow |= 1n << BigInt(bit); if (states[bit] === 'deny') deny |= 1n << BigInt(bit); }
      if (reset || !(allow | deny)) { await api.request(`/channels/${channel.id}/permissions/${selectedRole}`, 'DELETE'); rules = rules.filter(rule => rule.role_id !== selectedRole); select(selectedRole); }
      else { const rule = {role_id:selectedRole,allow:allow.toString(),deny:deny.toString()}; await api.request(`/channels/${channel.id}/permissions/${selectedRole}`, 'PUT', rule); rules = [...rules.filter(item => item.role_id !== selectedRole), rule]; }
      success = 'Posting rules saved.';
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Could not save posting rules.'; onError(cause); }
    finally { busy = false; }
  }
</script>
<h2 id="dialog-title">Posting rules · {channel.name}</h2>
<p class="muted small">{channel.parent_id ? 'Threads inherit their parent channel’s rules. Set an override here to change them for this thread.' : 'Inherit follows the server role permissions.'} Owners and administrators can always post.</p>
{#if loading}<p role="status">Loading rules…</p>{:else}
  <label>Role<select value={selectedRole} onchange={event => select(event.currentTarget.value)} disabled={busy}>{#each roles as role}<option value={role.id}>{role.name}</option>{/each}</select></label>
  {#each options as option}<label>{option.name}<select bind:value={states[option.bit]} disabled={!editable || busy}><option value="inherit">Inherit</option><option value="allow" disabled={!can(snapshot.permissions, option.bit, snapshot.is_owner)}>Allow</option><option value="deny">Deny</option></select></label>{/each}
  <p class="muted small">For a role-only thread, deny writing or uploads to @everyone, then allow them for the roles you choose. Across a member’s roles, an explicit allow wins.</p>
  {#if !editable}<p class="muted small">You can change rules only for roles below your highest role.</p>{/if}
  {#if error}<div class="error" role="alert">{error}</div>{/if}{#if success}<div class="notice" role="status">{success}</div>{/if}
  <div class="button-row"><button class="secondary" disabled={!editable || busy} onclick={() => save(true)}>Reset role to inheritance</button><button class="primary" disabled={!editable || busy} onclick={() => save()}>{busy ? 'Saving…' : 'Save rules'}</button></div>
{/if}
