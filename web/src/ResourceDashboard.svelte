<script lang="ts">
  import { storageLabel } from './lib/resources';
  import { onMount, onDestroy } from 'svelte';
  import { Api, ApiError } from './lib/api';
  import type { HostResources, ServerStorage } from './lib/resources';
  import { giga, ramMb } from './lib/resources';
  import ResourceGauge from './ResourceGauge.svelte';
  import BackupCard from './BackupCard.svelte';
  import UpdateCard from './UpdateCard.svelte';
  export let api: Api;
  let resources: HostResources | null = null, storage: ServerStorage | null = null;
  let hostAdmin = true, polling = false, busy = false, loading = true, alive = true;
  let hostFormReady = false, storageFormReady = false;
  let error = '', hostError = '', success = '';
  let serverCapped = false, ramCapped = false, aiCapped = false;
  let serverCores = 1, serverRamGb = 2, aiCores = 1, storageGb = 10;
  let policy: 'delete_oldest' | 'refuse' = 'delete_oldest', acknowledgePrune = false;
  let pending: 'host' | 'storage' | null = null;
  let timer: ReturnType<typeof setInterval>;
  const abort = new AbortController();
  function message(cause: unknown) { return cause instanceof Error ? cause.message : 'The request failed.'; }
  function initHost(value: HostResources) { serverCapped = value.cpu.server.limit_cores !== null; serverCores = value.cpu.server.limit_cores ?? Math.min(2,value.cpu.cores_total); ramCapped = value.ram.server.limit_bytes !== null; serverRamGb = giga(value.ram.server.limit_bytes) ?? 2; aiCapped = value.cpu.ai.limit_cores !== null; aiCores = value.cpu.ai.limit_cores ?? Math.min(2,value.cpu.cores_total); hostFormReady = true; }
  async function refresh() {
    if (polling || busy || !alive) return; polling = true;
    try {
      const results = await Promise.allSettled([api.request<ServerStorage>('/storage','GET',undefined,abort.signal), ...(hostAdmin ? [api.request<HostResources>('/host/resources','GET',undefined,abort.signal)] : [])]);
      if (!alive) return;
      const disk = results[0]; if (disk.status === 'fulfilled') { storage = disk.value as ServerStorage; if (!storageFormReady) { storageGb = giga(storage.limit_bytes) ?? 0; policy = storage.full_action; storageFormReady = true; } } else error = message(disk.reason);
      const host = results[1]; if (host?.status === 'fulfilled') { resources = host.value as HostResources; hostError = ''; if (!hostFormReady) initHost(resources); }
      else if (host?.status === 'rejected') { if (host.reason instanceof ApiError && host.reason.code === 'host_admin_only') { hostAdmin = false; resources = null; } else hostError = message(host.reason); }
    } finally { if (alive) { polling = false; loading = false; } }
  }
  async function save() {
    if (!pending || busy) return; busy = true; error = ''; hostError = ''; success = '';
    try {
      if (pending === 'host') { const value = await api.request<HostResources>('/host/resources','PATCH',{server_cores: serverCapped ? Number(serverCores) : null, server_ram_mb: ramCapped ? ramMb(serverRamGb) : null, ai_cores: aiCapped ? Number(aiCores) : null}); if (alive) { resources = value; initHost(value); } }
      else { await api.request('/settings','PATCH',{server_total_mb: ramMb(storageGb), storage_full_action: policy}); if (alive) { storageFormReady = false; acknowledgePrune = false; } }
      if (alive) { pending = null; success = 'Limits saved.'; }
    } catch (cause) { if (alive) error = message(cause); }
    finally { if (alive) { busy = false; void refresh(); } }
  }
  onMount(() => { void refresh(); timer = setInterval(() => { if (document.visibilityState === 'visible') void refresh(); },4000); });
  onDestroy(() => { alive = false; abort.abort(); clearInterval(timer); });
</script>
<p class="small muted">Usage refreshes every four seconds while this dashboard is open. Storage belongs to this community; CPU and memory limits apply to the whole host. 1 GB here means 1,024 MB.</p>
{#if hostAdmin}<UpdateCard {api}/><BackupCard {api}/>{/if}
{#if error}<div class="error" role="alert">{error}</div>{/if}{#if success}<p class="notice" role="status">{success}</p>{/if}
{#if loading}<p role="status">Loading resource usage…</p>{/if}
{#if storage}
  <h3 class="section-heading">Community storage</h3><p class="small muted">For this server only. Other communities on the same host have separate storage limits.</p><div class="resource-grid"><ResourceGauge label="Messages and attachments" value={giga(storage.used_bytes)} limit={giga(storage.limit_bytes)} detail="Message text and uploaded files; AI review copies are counted separately."/><ResourceGauge label="AI review copies" value={giga(storage.ai_bytes)} limit={null} detail="Text retained for AI review. Separate from the community storage limit and model files."/></div>
  <p class="small muted">Attachments: {storageLabel(storage.files_bytes)} · Message text: {storageLabel(storage.messages_bytes)}</p>
  <form onsubmit={event => { event.preventDefault(); pending = 'storage'; }}><fieldset disabled={busy || !!pending}><label>Maximum community storage (GB, 0 = unlimited)<input type="number" min="0" max="97656.25" step="0.001" required bind:value={storageGb}/></label><label>When storage is full<select bind:value={policy}><option value="delete_oldest">Delete oldest messages and their attachments</option><option value="refuse">Refuse new messages and uploads</option></select></label>{#if policy === 'delete_oldest'}<p class="notice">As new content arrives, older messages and their images, videos and files can be permanently removed until usage falls below 90% of the limit.</p><label class="check-label"><input type="checkbox" bind:checked={acknowledgePrune}/>I understand older content may be permanently deleted.</label>{/if}<button class="primary" disabled={policy === 'delete_oldest' && !acknowledgePrune}>Review storage changes</button></fieldset></form>
{/if}
{#if !hostAdmin}<p class="notice">CPU and memory controls are available only to the host administrator, the owner of the host’s first community.</p>{/if}
{#if hostError}<div class="error" role="alert">Host resource monitoring: {hostError}</div>{/if}
{#if resources && hostAdmin}
  <h3 class="section-heading">Chat host</h3><p class="small muted">For the whole host: all communities share these CPU and memory limits.</p><div class="resource-grid"><ResourceGauge label="Server CPU" value={resources.cpu.server.usage_cores} limit={resources.cpu.server.limit_cores ?? resources.cpu.cores_total} unit="cores" detail={`${resources.cpu.server.limit_cores === null ? 'No cap; gauge uses machine capacity. ' : ''}${resources.cpu.server.enforced ? 'Limit enforced.' : 'Not enforced on this system.'}`}/><ResourceGauge label="Server memory" value={giga(resources.ram.server.used_bytes)} limit={giga(resources.ram.server.limit_bytes ?? resources.ram.total_bytes)} detail={`${resources.ram.server.limit_bytes === null ? 'No cap; gauge uses machine capacity. ' : ''}${resources.ram.server.enforced ? 'Limit enforced.' : 'Not enforced on this system.'}`}/></div>
  <p class="small muted">Machine: {resources.cpu.cores_total} cores · {storageLabel(resources.ram.total_bytes)} RAM · {storageLabel(resources.ram.available_bytes)} RAM available</p>
  <h3 class="section-heading">AI service (separate)</h3><p class="small muted">These figures cover {resources.cpu.ai.unit ?? 'no managed AI service'}. Ollama and outside AI services are not included.</p>
  <div class="resource-grid"><ResourceGauge label="Laya CPU" value={resources.cpu.ai.usage_cores} limit={resources.cpu.ai.limit_cores ?? resources.cpu.cores_total} unit="cores" detail={`${resources.cpu.ai.limit_cores === null ? 'No cap; gauge uses machine capacity. ' : ''}${resources.cpu.ai.enforced ? 'Limit enforced.' : 'Not enforced on this system.'}`}/><ResourceGauge label="Laya memory" value={giga(resources.ram.ai.used_bytes)} limit={giga(resources.ram.ai.limit_bytes ?? resources.ram.total_bytes)} detail="AI memory is separate from the server RAM limit. Laya typically needs about 1.5 GB."/><ResourceGauge label="AI model files" value={giga(resources.ai_disk_bytes)} limit={null} detail="Separate model-folder usage, informational only; no model storage cap is enforced."/><ResourceGauge label="Whole data drive" value={giga(resources.disk.total_bytes !== null && resources.disk.free_bytes !== null ? resources.disk.total_bytes - resources.disk.free_bytes : null)} limit={giga(resources.disk.total_bytes)} detail="Includes other applications and the operating system, not just ChitChat."/></div>
  <h3 class="section-heading">Host limits</h3>{#if !resources.cpu.server.enforced || !resources.ram.server.enforced}<p class="notice">Limits marked as not enforced can be saved, but this system will not restrict that resource.</p>{/if}<p class="notice">A server memory limit that is too low can restart the whole chat host. On systemd, the host reclaims memory above the configured value and applies a hard ceiling 25% higher. Other systems may report usage without enforcing limits.</p>
  <form onsubmit={event => { event.preventDefault(); pending = 'host'; }}><fieldset disabled={busy || !!pending}>
    <label class="check-label"><input type="checkbox" bind:checked={serverCapped}/>Limit server CPU</label>{#if serverCapped}<label>Server CPU cores<input type="number" min="0.5" max={resources.cpu.cores_total} step="0.5" required bind:value={serverCores}/></label>{/if}
    <label class="check-label"><input type="checkbox" bind:checked={ramCapped}/>Limit server RAM (excludes AI)</label>{#if ramCapped}<label>Server RAM limit (GB)<input type="number" min="0.25" max={giga(resources.ram.total_bytes) ?? undefined} step="0.25" required bind:value={serverRamGb}/></label>{/if}
    <label class="check-label"><input type="checkbox" bind:checked={aiCapped} disabled={!resources.cpu.ai.unit}/>Limit Laya CPU separately</label>{#if aiCapped}<label>Laya CPU cores<input type="number" min="0.5" max={resources.cpu.cores_total} step="0.5" required bind:value={aiCores}/></label>{/if}
    <button class="primary">Review host limits</button>
  </fieldset></form>
{/if}
{#if pending}<div class="confirm-box" role="alert"><h3>Apply {pending === 'host' ? 'host resource limits' : 'storage policy'}?</h3>{#if pending === 'host'}<p>Server CPU: {serverCapped ? `${serverCores} cores` : 'unlimited'}. Server RAM: {ramCapped ? `${serverRamGb} GB` : 'unlimited'}. Laya CPU: {aiCapped ? `${aiCores} cores` : 'unlimited'}. These affect everyone using the host; a low RAM limit can restart it.</p>{:else}<p>Storage: {storageGb} GB. {policy === 'delete_oldest' ? 'Older messages and their attachments may be permanently removed when new content arrives.' : 'New messages and uploads will be refused when full.'}</p>{/if}<div class="button-row"><button class="secondary" disabled={busy} onclick={() => pending = null}>Cancel</button><button class="primary" disabled={busy} onclick={save}>{busy ? 'Saving…' : 'Confirm limits'}</button></div></div>{/if}
<button class="secondary" disabled={busy || polling} onclick={refresh}>Refresh usage</button>
