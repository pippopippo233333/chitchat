<script lang="ts">
  import DiscoverySettings from './DiscoverySettings.svelte';
  import HostServers from './HostServers.svelte';
  import type { HostServer, CreatedServer } from './lib/host-servers';
  import { ApiError } from './lib/api';
  import ResourceDashboard from './ResourceDashboard.svelte';
  import AiSettings from './AiSettings.svelte';
  import { onMount } from 'svelte';
  import { Api, can } from './lib/api';
  import type { Snapshot, Settings, Invite, IpRule, Ban, AuditEntry, Role } from './lib/api';
  import { permissionNames, canManageRole, togglePermission } from './lib/permissions';
  import { bytes } from './lib/media';
  export let api: Api;
  export let snapshot: Snapshot;
  export let onCreated: (result: CreatedServer) => void;
  export let onOpen: (server: import('./lib/api').ServerSummary) => void;
  let hostServers: HostServer[] | null = null;
  let hostError = '';
  export let onError: (error: unknown) => void;
  let tab = '';
  let settings: Settings | null = null;
  let invites: Invite[] = [], ipRules: IpRule[] = [], bans: Ban[] = [], audit: AuditEntry[] = [];
  let loading = false, busy = false, error = '', success = '';
  let password = '', removePassword = false, allowAcknowledged = false;
  let cidr = '', ipAction: 'allow' | 'block' = 'allow', ipNote = '';
  let inviteExpiry = '86400', inviteUses = '10', copied = '';
  let roleId = '', roleName = '', roleColor = '#a4e4c2', rolePermissions = '259', rolePosition = 1;
  let confirm: { title: string; detail: string; action: () => Promise<void> } | null = null;
  let auditMore = true;
  $: tabs = [
    ...(can(snapshot.permissions, 10, snapshot.is_owner) ? ['Overview', ...(snapshot.is_owner ? ['Security'] : []), 'Media', 'Moderation'] : []),
    ...(snapshot.is_owner ? ['Resources', ...(hostServers ? ['Servers on this host', 'Discovery'] : []), 'AI moderation'] : []),
    ...(can(snapshot.permissions, 11, snapshot.is_owner) ? ['Invites'] : []),
    ...(can(snapshot.permissions, 4, snapshot.is_owner) ? ['Roles'] : []),
    ...(can(snapshot.permissions, 6, snapshot.is_owner) ? ['Bans'] : []),
    ...([5, 6, 10].some(bit => can(snapshot.permissions, bit, snapshot.is_owner)) ? ['Audit log'] : []),
  ];
  $: sortedRoles = [...snapshot.roles].sort((a, b) => b.position - a.position);
  $: selectedRole = snapshot.roles.find(role => role.id === roleId);
  $: if (tab && !tabs.includes(tab)) { tab = ''; settings = null; }
  const mediaFields = [{ key: 'max_file_mb', label: 'Maximum file size (MB)', min: 1, max: 2048 }, { key: 'user_daily_mb', label: 'Per-user daily quota (MB)', min: 0, max: 1000000 }, { key: 'user_total_mb', label: 'Per-user total quota (MB)', min: 0, max: 10000000 }, { key: 'server_total_mb', label: 'Server storage cap (MB)', min: 0, max: 100000000 }, { key: 'media_retention_days', label: 'Auto-delete media after days (0 = keep)', min: 0, max: 36500 }] as const;
  const moderationFields = [{ key: 'new_account_restrict_hours', label: 'New-account link/media restriction (hours, 0 = off)', min: 0, max: 720 }, { key: 'max_accounts_per_ip', label: 'Accounts per IP (0 = unlimited)', min: 0, max: 100 }, { key: 'spam_messages_per_5s', label: 'Messages per 5 seconds', min: 1, max: 50 }, { key: 'spam_chars_per_minute', label: 'Characters per minute', min: 500, max: 100000 }, { key: 'spam_max_repeats', label: 'Identical messages per minute', min: 1, max: 20 }, { key: 'spam_timeout_minutes', label: 'Auto-mute duration (minutes, 0 = cooldown only)', min: 0, max: 1440 }] as const;
  function fail(cause: unknown) { error = cause instanceof Error ? cause.message : 'The request failed.'; onError(cause); }
  async function run(action: () => Promise<void>) { if (busy) return; busy = true; error = ''; success = ''; try { await action(); success = 'Saved.'; } catch (cause) { fail(cause); } finally { busy = false; } }
  async function selectTab(next: string) {
    if (busy) return; tab = next; error = ''; success = ''; confirm = null; loading = true;
    try {
      if (['Overview', 'Security', 'Media', 'Moderation'].includes(next)) settings = await api.request<Settings>('/settings');
      if (next === 'Security') ipRules = await api.request<IpRule[]>('/ip-rules');
      if (next === 'Invites') invites = await api.request<Invite[]>('/invites');
      if (next === 'Bans') bans = await api.request<Ban[]>('/bans');
      if (next === 'Audit log') { audit = await api.request<AuditEntry[]>('/audit-log?limit=50'); auditMore = audit.length === 50; }
    } catch (cause) { fail(cause); } finally { loading = false; }
  }
  async function saveSettings(event: SubmitEvent) {
    event.preventDefault(); if (!settings) return;
    const values = settings;
    await run(async () => {
      const body = tab === 'Overview' ? { name: values.name, description: values.description, listed: values.listed, ...(snapshot.is_owner ? { discoverable: !!values.discoverable } : {}), registration_open: values.registration_open, invite_only: values.invite_only }
        : tab === 'Media' ? { ...Object.fromEntries(mediaFields.map(field => [field.key, Number(values[field.key])])), voice_messages: values.voice_messages ?? true }
        : Object.fromEntries(moderationFields.map(field => [field.key, Number(values[field.key])]));
      settings = await api.request<Settings>('/settings', 'PATCH', body);
    });
  }
  function editRole(role?: Role) { roleId = role?.id ?? ''; roleName = role?.name ?? ''; roleColor = role?.color ?? '#a4e4c2'; rolePermissions = role?.permissions ?? '259'; rolePosition = role?.position ?? 1; }
  async function saveRole(event: SubmitEvent) {
    event.preventDefault(); await run(async () => {
      const body = selectedRole?.is_default ? { color: roleColor, permissions: rolePermissions } : { name: roleName.trim(), color: roleColor, permissions: rolePermissions, position: Number(rolePosition) };
      const role = await api.request<Role>(roleId ? `/roles/${roleId}` : '/roles', roleId ? 'PATCH' : 'POST', body);
      snapshot.roles = [...snapshot.roles.filter(item => item.id !== role.id), role]; snapshot = { ...snapshot }; editRole(role);
    });
  }
  function inviteLink(code: string) { return `${api.origin}/s/${encodeURIComponent(api.slug)}/invite/${encodeURIComponent(code)}`; }
  async function copyInvite(code: string) { try { await navigator.clipboard.writeText(inviteLink(code)); copied = code; } catch { error = 'Clipboard access was unavailable. Select and copy the invite address below.'; } }
  async function moreAudit() { if (!audit.length) return; await run(async () => { const page = await api.request<AuditEntry[]>(`/audit-log?limit=50&before=${encodeURIComponent(audit.at(-1)!.id)}`); audit = [...audit, ...page]; auditMore = page.length === 50; }); }
  function actor(id: string | null) { return id === null ? 'Automatic' : snapshot.members.find(member => member.user.id === id)?.user.display_name ?? id; }
  onMount(() => { if (tabs.length) void selectTab(tabs[0]); if(snapshot.is_owner)void api.request<HostServer[]>('/host/servers').then(value=>hostServers=value).catch(cause=>{if(!(cause instanceof ApiError && cause.code==='host_admin_only'))hostError=cause instanceof Error?cause.message:'Could not load host servers.';}); });
</script>

<h2 id="dialog-title">Server settings</h2><p class="muted small">{['Servers on this host','Discovery'].includes(tab) ? 'Host administrator · Manage communities on this host.' : `${snapshot.server.name} · Changes apply to this community.`}</p>
{#if hostError}<p class="error" role="alert">{hostError}</p>{/if}
<nav class="settings-tabs" aria-label="Settings sections">{#each tabs as item}<button class:active={tab === item} aria-current={tab === item ? 'page' : undefined} disabled={busy || loading} onclick={() => selectTab(item)}>{item}</button>{/each}</nav>
{#if error}<div class="error" role="alert">{error}</div>{/if}{#if success}<div class="notice" role="status">{success}</div>{/if}
{#if confirm}<div class="confirm-box" role="alert"><h3>{confirm.title}</h3><p>{confirm.detail}</p><div class="button-row"><button class="secondary" disabled={busy} onclick={() => confirm = null}>Cancel</button><button class="primary" disabled={busy} onclick={() => { const action = confirm!.action; void run(async () => { await action(); confirm = null; }); }}>Confirm</button></div></div>{/if}
{#if loading}<p class="loading" role="status">Loading settings…</p>
{:else if tab === 'Discovery' && hostServers}
  <DiscoverySettings {api}/>
{:else if tab === 'Servers on this host' && hostServers}
  <HostServers {api} servers={hostServers} {onCreated} {onOpen} {onError}/>
{:else if tab === 'Resources' && snapshot.is_owner}
  <ResourceDashboard {api}/>
{:else if tab === 'AI moderation' && snapshot.is_owner}
  <AiSettings {api} {snapshot}/>
{:else if tab === 'Overview' && settings}
  <form onsubmit={saveSettings}><label>Server name<input bind:value={settings.name} required maxlength="64"/></label><label>Description<textarea bind:value={settings.description} maxlength="500"></textarea></label><label class="check-label"><input type="checkbox" bind:checked={settings.listed}/>List on this host’s landing page</label>{#if snapshot.is_owner}<label class="check-label"><input type="checkbox" bind:checked={settings.discoverable}/>Show in the server browser</label><p class="small muted">Shares this community’s name, description and member count with users on other hosts when host discovery is enabled.</p>{/if}<label class="check-label"><input type="checkbox" bind:checked={settings.registration_open}/>Allow new accounts</label><label class="check-label"><input type="checkbox" bind:checked={settings.invite_only}/>Require an invite for registration</label><p class="muted small">Closing registration also blocks invites. Existing members can still sign in.</p><button class="primary" disabled={busy}>Save overview</button></form>
{:else if tab === 'Security' && settings && snapshot.is_owner}
  <h3 class="section-heading">Community access password</h3><p class="small muted">{settings.has_password ? 'A password is required for new accounts.' : 'No community password is set.'} Invites do not bypass it.</p>
  <form onsubmit={event => { event.preventDefault(); void run(async () => { settings = await api.request<Settings>('/settings/password', 'PUT', { password: removePassword ? null : password }); password = ''; removePassword = false; }); }}><label class="check-label"><input type="checkbox" bind:checked={removePassword}/>Remove the community password</label>{#if !removePassword}<label>New community password<input type="password" bind:value={password} required minlength="4" maxlength="128" autocomplete="new-password"/></label>{/if}<button class="primary" disabled={busy}>Update access password</button></form>
  <h3 class="section-heading">IP access rules</h3><p class="notice">Behind tunnels, visitors can all appear with the tunnel’s IP. These rules cannot reliably identify visitors there. A block can affect shared networks.</p>
  <form onsubmit={event => { event.preventDefault(); void run(async () => { settings = await api.request<Settings>('/settings', 'PATCH', { ip_mode: settings!.ip_mode }); allowAcknowledged = false; }); }}><label class="check-label"><input type="checkbox" checked={settings.ip_mode === 'allowlist'} onchange={event => { settings!.ip_mode = event.currentTarget.checked ? 'allowlist' : 'off'; }}/>Only permit listed IP addresses</label>{#if settings.ip_mode === 'allowlist'}<label class="check-label"><input type="checkbox" bind:checked={allowAcknowledged}/>I understand only addresses matching an allow rule can join. Loopback remains allowed.</label>{/if}<button class="secondary" disabled={busy || (settings.ip_mode === 'allowlist' && !allowAcknowledged)}>Save IP mode</button></form>
  {#each ipRules as rule}<div class="settings-row"><div><strong>{rule.action}: {rule.cidr}</strong><p>{rule.note}</p></div><button class="secondary" disabled={busy} onclick={() => confirm = { title: 'Remove IP rule?', detail: `${rule.action}: ${rule.cidr}`, action: async () => { await api.request(`/ip-rules/${rule.id}`, 'DELETE'); ipRules = ipRules.filter(item => item.id !== rule.id); } }}>Remove</button></div>{/each}
  <form onsubmit={event => { event.preventDefault(); void run(async () => { const rule = await api.request<IpRule>('/ip-rules', 'POST', { cidr: cidr.trim(), action: ipAction, note: ipNote.trim() }); ipRules = [...ipRules, rule]; cidr = ''; ipNote = ''; }); }}><label>IP address or subnet<input bind:value={cidr} required placeholder="203.0.113.7 or 203.0.113.0/24"/></label><label>Action<select bind:value={ipAction}><option value="allow">Allow</option><option value="block">Block</option></select></label><label>Note<input bind:value={ipNote} maxlength="500"/></label><button class="primary" disabled={busy}>Add IP rule</button></form>
{:else if (tab === 'Media' || tab === 'Moderation') && settings}
  <form onsubmit={saveSettings}>{#if tab === 'Media'}<label class="check-label"><input type="checkbox" checked={settings.voice_messages ?? true} onchange={event=>settings!.voice_messages=event.currentTarget.checked}/>Allow voice messages</label><p class="small muted">Members can record up to 5 minutes. The channel must also allow attachments.</p><p class="small muted">Storage used: {bytes(settings.storage_used_bytes)}. Owners are exempt from per-user quotas, but file and server caps still apply.</p>{:else}<p class="small muted">IP account limits can affect shared networks. Owners are exempt from spam limits.</p>{/if}
    {#each tab === 'Media' ? mediaFields : moderationFields as field}<label>{field.label}<input type="number" min={field.min} max={field.max} step="1" required bind:value={settings[field.key]}/></label>{/each}
    <button class="primary" disabled={busy}>Save {tab.toLowerCase()} settings</button>
  </form>
{:else if tab === 'Invites'}
  <form onsubmit={event => { event.preventDefault(); void run(async () => { const invite = await api.request<Invite>('/invites', 'POST', { ...(inviteExpiry ? { expires_in: Number(inviteExpiry) } : {}), ...(inviteUses ? { max_uses: Number(inviteUses) } : {}) }); invites = [invite, ...invites]; }); }}><div class="form-grid"><label>Expires after<select bind:value={inviteExpiry}><option value="3600">1 hour</option><option value="86400">1 day</option><option value="604800">7 days</option><option value="">Never</option></select></label><label>Maximum uses<select bind:value={inviteUses}><option value="1">1 use</option><option value="10">10 uses</option><option value="100">100 uses</option><option value="">Unlimited</option></select></label></div><button class="primary" disabled={busy}>Create invite</button></form>
  {#each invites as invite}<div class="invite-row"><label>Invite address<input readonly value={inviteLink(invite.code)} aria-label={`Invite ${invite.code}`}/></label><p class="muted small">{invite.uses} used{invite.max_uses ? ` / ${invite.max_uses}` : ''} · {invite.expires_at ? `Expires ${new Date(invite.expires_at).toLocaleString()}` : 'No expiry'}</p><div class="button-row"><button class="secondary" onclick={() => copyInvite(invite.code)}>{copied === invite.code ? 'Copied' : 'Copy link'}</button><button class="secondary danger" disabled={busy} onclick={() => confirm = { title: 'Revoke invite?', detail: 'This link will no longer allow new accounts to join.', action: async () => { await api.request(`/invites/${encodeURIComponent(invite.code)}`, 'DELETE'); invites = invites.filter(item => item.code !== invite.code); } }}>Revoke</button></div></div>{:else}<p class="loading">No usable invites.</p>{/each}
{:else if tab === 'Roles'}
  <p class="small muted">Higher roles outrank lower roles. Administrators receive all permissions; owner-only security actions remain restricted.</p><div class="role-list">{#each sortedRoles as role}<button class="secondary" disabled={!canManageRole(snapshot, role)} onclick={() => editRole(role)}><span class="role-dot" style:background={role.color || '#8c9ca5'}></span>{role.name} · rank {role.position}</button>{/each}<button class="secondary" onclick={() => editRole()}>+ New role</button></div>
  <form onsubmit={saveRole}><label>Role name<input bind:value={roleName} required maxlength="64" disabled={selectedRole?.is_default}/></label><div class="form-grid"><label>Color<input type="color" bind:value={roleColor}/></label><label>Rank<input type="number" min="1" max={Math.max(1, snapshot.roles.length)} step="1" bind:value={rolePosition} disabled={selectedRole?.is_default}/></label></div><div class="permission-grid">{#each permissionNames as name, bit}<label class="check-label"><input type="checkbox" checked={(BigInt(rolePermissions) & (1n << BigInt(bit))) !== 0n} disabled={!can(snapshot.permissions, bit, snapshot.is_owner) || (!!selectedRole && !canManageRole(snapshot, selectedRole))} onchange={event => rolePermissions = togglePermission(rolePermissions, bit, event.currentTarget.checked)}/>{name}</label>{/each}</div><button class="primary" disabled={busy || (!!selectedRole && !canManageRole(snapshot, selectedRole))}>Save role</button></form>
  {#if selectedRole && !selectedRole.is_default}<button class="secondary danger" disabled={busy || !canManageRole(snapshot, selectedRole)} onclick={() => { const role = selectedRole!; confirm = { title: `Delete ${role.name}?`, detail: 'Its permissions and assignments will be removed from all members.', action: async () => { await api.request(`/roles/${role.id}`, 'DELETE'); snapshot.roles = snapshot.roles.filter(item => item.id !== role.id); snapshot = { ...snapshot }; editRole(); } }; }}>Delete role</button>{/if}
{:else if tab === 'Bans'}
  {#each bans as ban}<div class="settings-row"><div><strong>{ban.user?.display_name ?? 'Deleted account'}</strong><p>{ban.reason || 'No reason recorded'}</p><p>{ban.expires_at ? `Until ${new Date(ban.expires_at).toLocaleString()}` : 'Permanent'} · {ban.ip_banned ? 'IP ban' : ''} {ban.device_banned ? 'Device ban' : ''}</p>{#if snapshot.is_owner && ban.ips?.length}<p>{ban.ips.join(', ')}</p>{/if}</div><button class="secondary" disabled={busy} onclick={() => confirm = { title: 'Lift this ban?', detail: 'The account will be able to sign in again.', action: async () => { await api.request(`/bans/${ban.id}`, 'DELETE'); bans = bans.filter(item => item.id !== ban.id); } }}>Unban</button></div>{:else}<p class="loading">No active bans.</p>{/each}
{:else if tab === 'Audit log'}
  {#each audit as entry}<article class="audit-entry"><strong>{entry.action.replaceAll('_', ' ')}</strong><p>{actor(entry.actor_id)} · {new Date(entry.created_at).toLocaleString()}</p>{#if entry.target_id}<p>Target: {actor(entry.target_id)}</p>{/if}<pre>{JSON.stringify(entry.details, null, 2)}</pre></article>{:else}<p class="loading">No audit entries.</p>{/each}{#if auditMore}<button class="secondary" disabled={busy} onclick={moreAudit}>Load older entries</button>{/if}
{/if}
