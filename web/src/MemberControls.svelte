<script lang="ts">
  import { Api, can } from './lib/api';
  import type { Snapshot, Member } from './lib/api';
  import { canManageRole, canModerate } from './lib/permissions';
  export let onDirect:((id:string)=>void)|undefined=undefined;
  export let api: Api;
  export let snapshot: Snapshot;
  export let member: Member;
  export let onError: (error: unknown) => void;
  export let onClose: () => void;
  let busy = false, error = '', success = '';
  let action: 'timeout' | 'clear' | 'kick' | 'ban' | null = null;
  let reason = '', minutes = '10', hours = '', purge = '0', banIp = false, banDevice = false;
  $: eligible = canModerate(snapshot, member);
  $: canKick = eligible && can(snapshot.permissions, 5, snapshot.is_owner);
  $: canBan = eligible && can(snapshot.permissions, 6, snapshot.is_owner);
  $: roles = [...snapshot.roles].sort((a, b) => b.position - a.position);
  async function assign(roleId: string, enabled: boolean) {
    busy = true; error = ''; success = '';
    try { await api.request(`/members/${member.user.id}/roles/${roleId}`, enabled ? 'PUT' : 'DELETE'); member = { ...member, role_ids: enabled ? [...new Set([...member.role_ids, roleId])] : member.role_ids.filter(id => id !== roleId) }; success = 'Role updated.'; }
    catch (cause) { error = cause instanceof Error ? cause.message : 'Role update failed.'; onError(cause); }
    finally { busy = false; }
  }
  async function moderate(event: SubmitEvent) {
    event.preventDefault(); if (!action || busy) return;
    busy = true; error = ''; success = '';
    try {
      const path = `/members/${member.user.id}`;
      if (action === 'timeout') await api.request(`${path}/timeout`, 'PUT', { minutes: Number(minutes), reason: reason.trim() });
      if (action === 'clear') await api.request(`${path}/timeout`, 'DELETE');
      if (action === 'kick') await api.request(`${path}/kick`, 'POST', { reason: reason.trim() });
      if (action === 'ban') await api.request('/bans', 'POST', { user_id: member.user.id, reason: reason.trim(), ...(hours ? { duration_hours: Number(hours) } : {}), ban_ip: banIp, ban_device: banDevice, delete_messages_hours: Number(purge) });
      if (action === 'kick' || action === 'ban') onClose();
      else { success = action === 'clear' ? 'Timeout cleared.' : 'Timeout applied.'; action = null; }
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Moderation failed.'; onError(cause); }
    finally { busy = false; }
  }
</script>

<h2 id="dialog-title">{member.user.display_name}</h2><p class="muted small">@{member.user.username} · {member.is_owner ? 'Server owner' : member.online ? 'Online' : 'Offline'}</p>
{#if member.timed_out_until && Date.parse(member.timed_out_until) > Date.now()}<div class="notice">Timed out until {new Date(member.timed_out_until).toLocaleString()}</div>{/if}
{#if error}<div class="error" role="alert">{error}</div>{/if}{#if success}<div class="notice" role="status">{success}</div>{/if}
{#if onDirect&&member.user.id!==snapshot.user.id}<button class="primary" disabled={busy||!can(snapshot.permissions,16,snapshot.is_owner)} onclick={()=>onDirect?.(member.user.id)}>Send direct message</button>{/if}
<h3 class="section-heading">Roles</h3>
{#each roles as role}<label class="check-label"><input type="checkbox" checked={role.is_default || member.role_ids.includes(role.id)} disabled={busy || role.is_default || member.is_owner || !canManageRole(snapshot, role)} onchange={event => assign(role.id, event.currentTarget.checked)}/><span class="role-dot" style:background={role.color || '#8c9ca5'}></span>{role.name}</label>{/each}
{#if canKick || canBan}<h3 class="section-heading">Moderation</h3><div class="button-row">{#if canKick}<button class="secondary" disabled={busy} onclick={() => action = 'timeout'}>Timeout</button><button class="secondary" disabled={busy} onclick={() => action = 'clear'}>Clear timeout</button><button class="secondary danger" disabled={busy} onclick={() => action = 'kick'}>Kick</button>{/if}{#if canBan}<button class="secondary danger" disabled={busy} onclick={() => action = 'ban'}>Ban</button>{/if}</div>{/if}
{#if action}<form class="confirm-box" onsubmit={moderate}>
  <h3>{action === 'timeout' ? 'Timeout' : action === 'clear' ? 'Clear timeout for' : action === 'kick' ? 'Kick' : 'Ban'} {member.user.display_name}?</h3>
  <p>{action === 'kick' ? 'They will be signed out and lose their roles. They can sign in again.' : action === 'ban' ? 'They will be signed out and cannot rejoin while the ban is active.' : action === 'clear' ? 'They will be able to send, edit, react, and upload again.' : 'They can read messages but cannot send, edit, react, or upload until the timeout ends.'}</p>
  {#if action === 'timeout'}<label>Duration<select bind:value={minutes}><option value="1">1 minute</option><option value="10">10 minutes</option><option value="60">1 hour</option><option value="1440">1 day</option><option value="10080">7 days</option></select></label>{/if}
  {#if action !== 'clear'}<label>Reason (optional)<textarea bind:value={reason} maxlength="500"></textarea></label>{/if}
  {#if action === 'ban'}<label>Duration<select bind:value={hours}><option value="">Permanent</option><option value="24">1 day</option><option value="168">7 days</option><option value="720">30 days</option></select></label><label class="check-label"><input type="checkbox" bind:checked={banIp}/>Also ban their IP addresses</label><p class="small danger">This may block other people on shared Wi-Fi or mobile networks. Tunnel IPs may be shared by everyone.</p><label class="check-label"><input type="checkbox" bind:checked={banDevice}/>Also ban this device</label><label>Delete recent messages<select bind:value={purge}><option value="0">Keep messages</option><option value="1">Last hour</option><option value="24">Last day</option><option value="168">Last 7 days</option></select></label>{#if purge !== '0'}<p class="small danger">These messages will be permanently deleted.</p>{/if}{/if}
  <div class="button-row"><button type="button" class="secondary" disabled={busy} onclick={() => action = null}>Cancel</button><button class="primary" disabled={busy}>Confirm {action === 'clear' ? 'clear timeout' : action}</button></div>
</form>{/if}
